//! Sync-first Sonos Event Manager
//!
//! Provides a fully synchronous API for managing Sonos event subscriptions.
//! All async operations are hidden in a background worker thread.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread::JoinHandle;

use sonos_api::Service;
use sonos_discovery::Device;
use sonos_stream::events::EnrichedEvent;
use sonos_stream::BrokerConfig;

use crate::error::{EventManagerError, Result};
use crate::iter::EventManagerIterator;
use crate::worker::{spawn_event_worker, Command};

/// Sync-first event manager for Sonos devices
///
/// Provides a fully synchronous API while managing async event subscriptions
/// in a background thread. All methods are blocking.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_event_manager::SonosEventManager;
/// use sonos_api::Service;
///
/// // Create manager (sync - no .await!)
/// let manager = SonosEventManager::new()?;
///
/// // Add discovered devices
/// let devices = sonos_discovery::get();
/// manager.add_devices(devices)?;
///
/// // Subscribe to events (sync)
/// let ip: std::net::IpAddr = "192.168.1.100".parse()?;
/// manager.ensure_service_subscribed(ip, Service::RenderingControl)?;
///
/// // Iterate over events (blocking)
/// for event in manager.iter() {
///     println!("Event: {:?}", event);
/// }
/// ```
pub struct SonosEventManager {
    /// Send commands to background worker
    command_tx: mpsc::Sender<Command>,

    /// Receive events from background worker
    event_rx: Arc<Mutex<mpsc::Receiver<EnrichedEvent>>>,

    /// Device info cache (sync access)
    devices: Arc<RwLock<HashMap<IpAddr, Device>>>,

    /// Service subscription ref counts (sync access)
    service_refs: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>,

    /// Background worker handle (kept alive)
    _worker: JoinHandle<()>,
}

impl SonosEventManager {
    /// Create a new SonosEventManager with default configuration
    ///
    /// This is a synchronous operation - no `.await` required.
    pub fn new() -> Result<Self> {
        Self::with_config(BrokerConfig::default())
    }

    /// Create a new SonosEventManager with custom configuration
    ///
    /// This is a synchronous operation - no `.await` required.
    pub fn with_config(config: BrokerConfig) -> Result<Self> {
        // Create channels for command/event communication
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        // Spawn background worker with its own tokio runtime
        let worker = spawn_event_worker(config, command_rx, event_tx);

        Ok(Self {
            command_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            devices: Arc::new(RwLock::new(HashMap::new())),
            service_refs: Arc::new(RwLock::new(HashMap::new())),
            _worker: worker,
        })
    }

    /// Add discovered devices to the manager (sync)
    ///
    /// Stores device information for later lookup. Does not automatically
    /// subscribe to any services.
    pub fn add_devices(&self, devices: Vec<Device>) -> Result<()> {
        let mut device_map = self
            .devices
            .write()
            .map_err(|_| EventManagerError::LockPoisoned)?;

        for device in devices {
            let ip: IpAddr = device
                .ip_address
                .parse()
                .map_err(|_| EventManagerError::InvalidIpAddress(device.ip_address.clone()))?;

            device_map.insert(ip, device);
        }

        Ok(())
    }

    /// Get all available devices (sync)
    pub fn devices(&self) -> Vec<Device> {
        self.devices
            .read()
            .map(|d| d.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get a specific device by IP address (sync)
    pub fn device_by_ip(&self, ip: IpAddr) -> Option<Device> {
        self.devices.read().ok()?.get(&ip).cloned()
    }

    /// Ensure a service is subscribed for a device (sync, ref-counted)
    ///
    /// Increments the reference count for the (device_ip, service) pair.
    /// If this is the first reference, triggers a subscription via the background worker.
    pub fn ensure_service_subscribed(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        let should_subscribe = {
            let mut refs = self
                .service_refs
                .write()
                .map_err(|_| EventManagerError::LockPoisoned)?;

            let count = refs.entry((device_ip, service)).or_insert(0);
            let was_zero = *count == 0;
            *count += 1;

            tracing::debug!(
                "Service reference count for {}:{:?}: {} -> {}",
                device_ip,
                service,
                if was_zero { 0 } else { *count - 1 },
                *count
            );

            was_zero
        };

        if should_subscribe {
            self.command_tx
                .send(Command::Subscribe {
                    ip: device_ip,
                    service,
                })
                .map_err(|_| EventManagerError::WorkerDisconnected)?;
        }

        Ok(())
    }

    /// Release a service subscription for a device (sync, ref-counted)
    ///
    /// Decrements the reference count for the (device_ip, service) pair.
    /// If this reaches zero, triggers an unsubscription via the background worker.
    pub fn release_service_subscription(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        let should_unsubscribe = {
            let mut refs = self
                .service_refs
                .write()
                .map_err(|_| EventManagerError::LockPoisoned)?;

            if let Some(count) = refs.get_mut(&(device_ip, service)) {
                let old_count = *count;
                *count = count.saturating_sub(1);

                tracing::debug!(
                    "Service reference count for {}:{:?}: {} -> {}",
                    device_ip,
                    service,
                    old_count,
                    *count
                );

                if *count == 0 {
                    refs.remove(&(device_ip, service));
                    true
                } else {
                    false
                }
            } else {
                tracing::warn!(
                    "Attempted to release subscription for {}:{:?} but no references found",
                    device_ip,
                    service
                );
                false
            }
        };

        if should_unsubscribe {
            self.command_tx
                .send(Command::Unsubscribe {
                    ip: device_ip,
                    service,
                })
                .map_err(|_| EventManagerError::WorkerDisconnected)?;
        }

        Ok(())
    }

    /// Get a blocking iterator over events
    ///
    /// Returns an iterator that blocks on `next()` until an event is available.
    /// Use `try_recv()` for non-blocking access.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Blocking iteration
    /// for event in manager.iter() {
    ///     println!("Event: {:?}", event);
    /// }
    ///
    /// // Non-blocking check
    /// let iter = manager.iter();
    /// if let Some(event) = iter.try_recv() {
    ///     println!("Got event: {:?}", event);
    /// }
    /// ```
    pub fn iter(&self) -> EventManagerIterator {
        EventManagerIterator::new(Arc::clone(&self.event_rx))
    }

    /// Get current service subscription statistics (sync)
    pub fn service_subscription_stats(&self) -> HashMap<(IpAddr, Service), usize> {
        self.service_refs
            .read()
            .map(|refs| refs.clone())
            .unwrap_or_default()
    }

    /// Check if a service is currently subscribed for a device (sync)
    pub fn is_service_subscribed(&self, device_ip: IpAddr, service: Service) -> bool {
        self.service_refs
            .read()
            .map(|refs| refs.get(&(device_ip, service)).map_or(false, |&c| c > 0))
            .unwrap_or(false)
    }

    /// Get the current reference count for a service subscription
    pub fn service_ref_count(&self, device_ip: IpAddr, service: Service) -> usize {
        self.service_refs
            .read()
            .map(|refs| refs.get(&(device_ip, service)).copied().unwrap_or(0))
            .unwrap_or(0)
    }

    /// Shutdown the background worker
    ///
    /// Called automatically on drop, but can be called manually for graceful shutdown.
    pub fn shutdown(&self) {
        let _ = self.command_tx.send(Command::Shutdown);
    }
}

impl Drop for SonosEventManager {
    fn drop(&mut self) {
        tracing::debug!(
            "SonosEventManager dropping, {} active service subscriptions",
            self.service_refs
                .read()
                .map(|r| r.len())
                .unwrap_or(0)
        );

        // Send shutdown command to worker
        let _ = self.command_tx.send(Command::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_management() {
        let manager = SonosEventManager::new().unwrap();

        // Initially no devices
        assert!(manager.devices().is_empty());

        // Add devices
        let devices = vec![Device {
            id: "test-1".to_string(),
            name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            room_name: "Living Room".to_string(),
        }];

        manager.add_devices(devices).unwrap();

        // Check devices were added
        let stored_devices = manager.devices();
        assert_eq!(stored_devices.len(), 1);

        // Check specific device lookup
        let device_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let device = manager.device_by_ip(device_ip).unwrap();
        assert_eq!(device.name, "Living Room");
    }

    #[test]
    fn test_reference_counting() {
        let manager = SonosEventManager::new().unwrap();
        let device_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = Service::RenderingControl;

        // Initially not subscribed
        assert!(!manager.is_service_subscribed(device_ip, service));
        assert_eq!(manager.service_ref_count(device_ip, service), 0);

        // First subscription
        manager
            .ensure_service_subscribed(device_ip, service)
            .unwrap();
        assert!(manager.is_service_subscribed(device_ip, service));
        assert_eq!(manager.service_ref_count(device_ip, service), 1);

        // Second subscription (increments ref count)
        manager
            .ensure_service_subscribed(device_ip, service)
            .unwrap();
        assert_eq!(manager.service_ref_count(device_ip, service), 2);

        // Release one subscription
        manager
            .release_service_subscription(device_ip, service)
            .unwrap();
        assert_eq!(manager.service_ref_count(device_ip, service), 1);
        assert!(manager.is_service_subscribed(device_ip, service));

        // Release last subscription
        manager
            .release_service_subscription(device_ip, service)
            .unwrap();
        assert_eq!(manager.service_ref_count(device_ip, service), 0);
        assert!(!manager.is_service_subscribed(device_ip, service));
    }

    #[test]
    fn test_stats() {
        let manager = SonosEventManager::new().unwrap();

        // Initially empty stats
        let stats = manager.service_subscription_stats();
        assert!(stats.is_empty());
    }
}
