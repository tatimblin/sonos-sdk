//! Background worker thread for event processing
//!
//! Spawns a thread with its own tokio runtime to manage the async EventBroker
//! while exposing a sync API to the parent SonosEventManager.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use sonos_api::Service;
use sonos_stream::events::EnrichedEvent;
use sonos_stream::registry::RegistrationId;
use sonos_stream::{BrokerConfig, EventBroker};
use tokio::sync::mpsc as tokio_mpsc;

/// Commands sent from the sync SonosEventManager to the background worker
#[derive(Debug)]
pub enum Command {
    /// Subscribe to a service on a device
    Subscribe { ip: IpAddr, service: Service },
    /// Unsubscribe from a service on a device
    Unsubscribe { ip: IpAddr, service: Service },
    /// Shutdown the worker
    Shutdown,
}

/// Spawns the background event worker thread
///
/// The worker owns its own tokio runtime and manages:
/// - The EventBroker (async)
/// - Subscription management
/// - Event forwarding to sync channels
pub fn spawn_event_worker(
    config: BrokerConfig,
    command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    event_tx: mpsc::Sender<EnrichedEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // Create a new single-threaded tokio runtime for this worker
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!(error = %e, "failed to create tokio runtime for event worker; no events will be delivered");
                return;
            }
        };

        rt.block_on(async {
            run_event_loop(config, command_rx, event_tx).await;
        });
    })
}

/// Main event loop running inside the tokio runtime
async fn run_event_loop(
    config: BrokerConfig,
    mut command_rx: tokio_mpsc::UnboundedReceiver<Command>,
    event_tx: mpsc::Sender<EnrichedEvent>,
) {
    // Create EventBroker (async)
    let mut broker = match EventBroker::new(config).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "failed to create event broker; no events will be delivered");
            return;
        }
    };

    // Get event iterator
    let mut events = match broker.event_iterator() {
        Ok(iter) => iter,
        Err(e) => {
            tracing::error!(error = %e, "failed to get event iterator; no events will be delivered");
            return;
        }
    };

    // Track registration IDs for each (ip, service) pair
    let mut registration_ids: HashMap<(IpAddr, Service), RegistrationId> = HashMap::new();

    tracing::info!("event worker started");

    loop {
        tokio::select! {
            biased;

            // Process commands first (deterministic priority)
            cmd = command_rx.recv() => {
                match cmd {
                    Some(Command::Subscribe { ip, service }) => {
                        tracing::debug!(%ip, ?service, "worker subscribing");
                        match broker.register_speaker_service(ip, service).await {
                            Ok(result) => {
                                registration_ids.insert((ip, service), result.registration_id);
                                tracing::debug!(
                                    %ip,
                                    ?service,
                                    registration_id = %result.registration_id,
                                    "registered speaker service"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    %ip,
                                    ?service,
                                    error = %e,
                                    "failed to register speaker service; no events or polling for it"
                                );
                            }
                        }
                    }
                    Some(Command::Unsubscribe { ip, service }) => {
                        tracing::debug!(%ip, ?service, "worker unsubscribing");
                        if let Some(reg_id) = registration_ids.remove(&(ip, service)) {
                            if let Err(e) = broker.unregister_speaker_service(reg_id).await {
                                tracing::debug!(
                                    %ip,
                                    ?service,
                                    error = %e,
                                    "failed to unregister speaker service"
                                );
                            }
                        } else {
                            // Expected when the matching Subscribe failed to register.
                            tracing::debug!(
                                %ip,
                                ?service,
                                "no registration to unsubscribe"
                            );
                        }
                    }
                    Some(Command::Shutdown) => {
                        tracing::debug!("worker received shutdown command");
                        return;
                    }
                    None => {
                        tracing::debug!("command channel closed, shutting down worker");
                        return;
                    }
                }
            }

            // Forward events to sync channel
            event = events.next_async() => {
                match event {
                    Some(e) => {
                        if event_tx.send(e).is_err() {
                            tracing::debug!("event receiver dropped, shutting down worker");
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("event stream ended, shutting down worker");
                        break;
                    }
                }
            }
        }
    }

    tracing::info!("event worker shut down");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_debug() {
        let cmd = Command::Subscribe {
            ip: "192.168.1.100".parse().unwrap(),
            service: Service::RenderingControl,
        };
        assert!(format!("{cmd:?}").contains("Subscribe"));
    }
}
