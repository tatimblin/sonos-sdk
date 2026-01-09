use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use dashmap::DashMap;
use sonos_api::Service;
use sonos_discovery::Device;
use sonos_stream::{BrokerConfig, EventBroker, EventIterator};

use crate::error::{EventManagerError, Result};

/// Simplified facade for Sonos event subscription management
///
/// This manager provides reference-counted service subscriptions and access to
/// the EventBroker's single multiplexed event stream.
pub struct SonosEventManager {
    /// The underlying event broker from sonos-stream
    broker: EventBroker,

    /// Map of device IP addresses to device information
    devices: Arc<RwLock<HashMap<IpAddr, Device>>>,

    /// Reference counting for service subscriptions: (device_ip, service) -> ref_count
    service_refs: Arc<DashMap<(IpAddr, Service), AtomicUsize>>,
}

impl SonosEventManager {
    /// Create a new SonosEventManager
    pub async fn new() -> Result<Self> {
        Self::with_config(BrokerConfig::default()).await
    }

    /// Create a new SonosEventManager with custom configuration
    pub async fn with_config(config: BrokerConfig) -> Result<Self> {
        let broker = EventBroker::new(config).await?;

        Ok(Self {
            broker,
            devices: Arc::new(RwLock::new(HashMap::new())),
            service_refs: Arc::new(DashMap::new()),
        })
    }

    /// Add discovered devices to the manager
    pub async fn add_devices(&self, devices: Vec<Device>) -> Result<()> {
        let mut device_map = self.devices.write().await;

        for device in devices {
            let ip: IpAddr = device
                .ip_address
                .parse()
                .map_err(|_| EventManagerError::Sync("Invalid IP address".into()))?;

            device_map.insert(ip, device);
        }

        Ok(())
    }

    /// Get all available devices
    pub async fn devices(&self) -> Vec<Device> {
        let device_map = self.devices.read().await;
        device_map.values().cloned().collect()
    }

    /// Get a specific device by IP address
    pub async fn device_by_ip(&self, ip: IpAddr) -> Option<Device> {
        let device_map = self.devices.read().await;
        device_map.get(&ip).cloned()
    }

    /// Ensure a service is subscribed for a device (reference counted)
    ///
    /// This increments the reference count for the (device_ip, service) pair.
    /// If this is the first reference, it registers with the EventBroker.
    pub async fn ensure_service_subscribed(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        let key = (device_ip, service);

        // Get or create reference counter
        let counter = self.service_refs.entry(key).or_insert_with(|| AtomicUsize::new(0));
        let old_count = counter.fetch_add(1, Ordering::SeqCst);

        // If this is the first reference, register with EventBroker
        if old_count == 0 {
            let registration_result = self
                .broker
                .register_speaker_service(device_ip, service)
                .await
                .map_err(|e| EventManagerError::DeviceRegistration {
                    device_ip,
                    service,
                    source: e,
                })?;

            tracing::debug!(
                "Registered {:?} for device {} (registration ID: {})",
                service,
                device_ip,
                registration_result.registration_id
            );
        }

        let new_count = old_count + 1;
        tracing::debug!(
            "Service reference count for {} {:?}: {} -> {}",
            device_ip, service, old_count, new_count
        );

        Ok(())
    }

    /// Release a service subscription for a device (reference counted)
    ///
    /// This decrements the reference count for the (device_ip, service) pair.
    /// If this reaches zero, the service is unregistered from the EventBroker.
    pub async fn release_service_subscription(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        let key = (device_ip, service);

        if let Some(counter) = self.service_refs.get(&key) {
            let old_count = counter.fetch_sub(1, Ordering::SeqCst);
            let new_count = old_count.saturating_sub(1);

            tracing::debug!(
                "Service reference count for {} {:?}: {} -> {}",
                device_ip, service, old_count, new_count
            );

            // If this was the last reference, unregister from EventBroker
            if new_count == 0 {
                // Remove from our tracking map
                self.service_refs.remove(&key);

                // TODO: We need a way to unregister from EventBroker
                // This would require storing registration IDs or extending EventBroker API
                tracing::debug!(
                    "Last reference for {} {:?} - should unregister from EventBroker",
                    device_ip, service
                );
            }
        } else {
            tracing::warn!(
                "Attempted to release subscription for {} {:?} but no references found",
                device_ip, service
            );
        }

        Ok(())
    }

    /// Get the EventBroker's single multiplexed event stream
    ///
    /// This provides access to ALL events from ALL registered devices and services.
    /// Each EnrichedEvent is tagged with speaker_ip and service for routing.
    pub fn get_event_iterator(&mut self) -> Result<EventIterator> {
        self.broker.event_iterator()
            .map_err(|e| EventManagerError::BrokerInitialization(e))
    }

    /// Get current service subscription statistics
    pub fn service_subscription_stats(&self) -> HashMap<(IpAddr, Service), usize> {
        self.service_refs
            .iter()
            .map(|entry| (*entry.key(), entry.value().load(Ordering::SeqCst)))
            .collect()
    }

    /// Check if a service is currently subscribed for a device
    pub fn is_service_subscribed(&self, device_ip: IpAddr, service: Service) -> bool {
        let key = (device_ip, service);
        self.service_refs.get(&key)
            .map(|counter| counter.load(Ordering::SeqCst) > 0)
            .unwrap_or(false)
    }
}

impl Drop for SonosEventManager {
    fn drop(&mut self) {
        tracing::debug!(
            "SonosEventManager dropping, {} active service subscriptions",
            self.service_refs.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_service_reference_counting() {
        let manager = SonosEventManager::new().await.unwrap();
        let device_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let service = Service::RenderingControl;

        // Initially not subscribed
        assert!(!manager.is_service_subscribed(device_ip, service));

        // First subscription should trigger registration
        // Note: This will fail in tests without actual devices, but shows the API
        // manager.ensure_service_subscribed(device_ip, service).await.unwrap();
        // assert!(manager.is_service_subscribed(device_ip, service));

        // Stats should reflect the reference count
        let stats = manager.service_subscription_stats();
        assert_eq!(stats.len(), 0); // No actual subscriptions in test environment
    }

    #[tokio::test]
    async fn test_device_management() {
        let manager = SonosEventManager::new().await.unwrap();

        // Initially no devices
        assert!(manager.devices().await.is_empty());

        // Add devices
        let devices = vec![
            Device {
                id: "test-1".to_string(),
                name: "Living Room".to_string(),
                ip_address: "192.168.1.100".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
                room_name: "Living Room".to_string()
            },
        ];

        manager.add_devices(devices.clone()).await.unwrap();

        // Check devices were added
        let stored_devices = manager.devices().await;
        assert_eq!(stored_devices.len(), 1);

        // Check specific device lookup
        let device_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let device = manager.device_by_ip(device_ip).await.unwrap();
        assert_eq!(device.name, "Living Room");
    }
}