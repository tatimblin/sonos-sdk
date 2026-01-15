//! Background worker thread for event processing
//!
//! Spawns a thread with its own tokio runtime to manage the async EventBroker
//! while exposing a sync API to the parent SonosEventManager.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sonos_api::Service;
use sonos_stream::events::EnrichedEvent;
use sonos_stream::registry::RegistrationId;
use sonos_stream::{BrokerConfig, EventBroker};

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
    command_rx: mpsc::Receiver<Command>,
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
                tracing::error!("Failed to create tokio runtime for event worker: {}", e);
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
    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<EnrichedEvent>,
) {
    // Create EventBroker (async)
    let mut broker = match EventBroker::new(config).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to create EventBroker: {}", e);
            return;
        }
    };

    // Get event iterator
    let mut events = match broker.event_iterator() {
        Ok(iter) => iter,
        Err(e) => {
            tracing::error!("Failed to get event iterator: {}", e);
            return;
        }
    };

    // Track registration IDs for each (ip, service) pair
    let mut registration_ids: HashMap<(IpAddr, Service), RegistrationId> = HashMap::new();

    tracing::info!("Event worker started");

    loop {
        tokio::select! {
            // Forward events to sync channel
            event = events.next_async() => {
                match event {
                    Some(e) => {
                        if event_tx.send(e).is_err() {
                            tracing::debug!("Event receiver dropped, shutting down worker");
                            break;
                        }
                    }
                    None => {
                        tracing::info!("Event stream ended, shutting down worker");
                        break;
                    }
                }
            }

            // Process commands (poll periodically)
            _ = tokio::time::sleep(Duration::from_millis(10)) => {
                // Process all pending commands
                while let Ok(cmd) = command_rx.try_recv() {
                    match cmd {
                        Command::Subscribe { ip, service } => {
                            tracing::debug!("Worker: Subscribing to {}:{:?}", ip, service);
                            match broker.register_speaker_service(ip, service).await {
                                Ok(result) => {
                                    registration_ids.insert((ip, service), result.registration_id);
                                    tracing::debug!(
                                        "Registered speaker service {}:{:?} with ID {}",
                                        ip, service, result.registration_id
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to register speaker service {}:{:?}: {}",
                                        ip, service, e
                                    );
                                }
                            }
                        }
                        Command::Unsubscribe { ip, service } => {
                            tracing::debug!("Worker: Unsubscribing from {}:{:?}", ip, service);
                            if let Some(reg_id) = registration_ids.remove(&(ip, service)) {
                                if let Err(e) = broker.unregister_speaker_service(reg_id).await {
                                    tracing::warn!(
                                        "Failed to unregister speaker service {}:{:?}: {}",
                                        ip, service, e
                                    );
                                }
                            } else {
                                tracing::warn!(
                                    "No registration ID found for {}:{:?}",
                                    ip, service
                                );
                            }
                        }
                        Command::Shutdown => {
                            tracing::info!("Worker received shutdown command");
                            return;
                        }
                    }
                }
            }
        }
    }

    tracing::info!("Event worker shut down");
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
        assert!(format!("{:?}", cmd).contains("Subscribe"));
    }
}
