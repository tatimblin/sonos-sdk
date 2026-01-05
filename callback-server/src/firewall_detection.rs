//! Firewall detection coordinator for callback server.
//!
//! This module implements a per-device firewall detection system that monitors
//! real UPnP event delivery to determine whether callback servers can receive
//! external requests from Sonos devices on the local network.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, RwLock};

/// Status of firewall detection for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallStatus {
    /// Detection has not been performed yet
    Unknown,
    /// Server can receive external requests from this device
    Accessible,
    /// Server appears to be blocked by firewall for this device
    Blocked,
    /// Detection failed due to other errors
    Error,
}

impl Default for FirewallStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Configuration for firewall detection behavior.
#[derive(Debug, Clone)]
pub struct FirewallDetectionConfig {
    /// Timeout for waiting for first event from a device
    pub event_wait_timeout: Duration,
    /// Enable per-device caching of firewall status
    pub enable_caching: bool,
    /// Maximum number of cached device states
    pub max_cached_devices: usize,
}

impl Default for FirewallDetectionConfig {
    fn default() -> Self {
        Self {
            event_wait_timeout: Duration::from_secs(15),
            enable_caching: true,
            max_cached_devices: 100,
        }
    }
}

/// Per-device firewall detection state.
#[derive(Debug, Clone)]
pub struct DeviceFirewallState {
    pub device_ip: IpAddr,
    pub status: FirewallStatus,
    pub first_subscription_time: SystemTime,
    pub first_event_time: Option<SystemTime>,
    pub detection_completed: bool,
    pub timeout_duration: Duration,
}

/// Result of a firewall detection operation.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub device_ip: IpAddr,
    pub status: FirewallStatus,
    pub reason: DetectionReason,
}

/// Reason for detection completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionReason {
    /// First event arrived within timeout
    EventReceived,
    /// No events received within timeout
    Timeout,
    /// Subscription creation failed
    SubscriptionFailed,
}

/// Coordinates per-device firewall detection by monitoring real UPnP event delivery.
///
/// The coordinator tracks firewall status on a per-device basis, triggering detection
/// when the first subscription is created for a device and monitoring for event arrivals
/// to determine connectivity status.
pub struct FirewallDetectionCoordinator {
    /// Per-device detection states
    device_states: Arc<RwLock<HashMap<IpAddr, Arc<RwLock<DeviceFirewallState>>>>>,

    /// Configuration for detection behavior
    config: FirewallDetectionConfig,

    /// Channel for notifying when detection completes
    detection_complete_tx: mpsc::UnboundedSender<DetectionResult>,

    /// Handle for the timeout monitoring task
    _timeout_task_handle: tokio::task::JoinHandle<()>,
}

impl FirewallDetectionCoordinator {
    /// Create a new firewall detection coordinator.
    pub fn new(config: FirewallDetectionConfig) -> Self {
        let (detection_complete_tx, mut detection_complete_rx) = mpsc::unbounded_channel();

        let device_states = Arc::new(RwLock::new(HashMap::new()));

        // Spawn background task for timeout monitoring
        let timeout_task_handle = {
            let device_states = device_states.clone();
            let detection_complete_tx = detection_complete_tx.clone();
            tokio::spawn(async move {
                Self::monitor_timeouts(device_states, detection_complete_tx).await;
            })
        };

        // Spawn task to handle detection results (logging for now)
        tokio::spawn(async move {
            while let Some(result) = detection_complete_rx.recv().await {
                match result.reason {
                    DetectionReason::EventReceived => {
                        eprintln!("✅ Firewall detection: Events accessible from {} (reason: {:?})",
                                 result.device_ip, result.reason);
                    }
                    DetectionReason::Timeout => {
                        eprintln!("🚫 Firewall detection: No events from {} within timeout (reason: {:?})",
                                 result.device_ip, result.reason);
                    }
                    DetectionReason::SubscriptionFailed => {
                        eprintln!("❌ Firewall detection: Subscription failed for {} (reason: {:?})",
                                 result.device_ip, result.reason);
                    }
                }
            }
        });

        Self {
            device_states,
            config,
            detection_complete_tx,
            _timeout_task_handle: timeout_task_handle,
        }
    }

    /// Called when the first subscription is created for a device.
    ///
    /// Returns the cached status if already known, otherwise starts monitoring
    /// and returns Unknown status while detection is in progress.
    pub async fn on_first_subscription(&self, device_ip: IpAddr) -> FirewallStatus {
        if !self.config.enable_caching {
            // Caching disabled - always return Unknown and start fresh detection
            self.start_detection_for_device(device_ip).await;
            return FirewallStatus::Unknown;
        }

        let device_states = self.device_states.read().await;

        // Check if we already have cached status
        if let Some(state_arc) = device_states.get(&device_ip) {
            let state = state_arc.read().await;
            if state.detection_completed {
                eprintln!("ℹ️  Firewall detection: Using cached status {:?} for {}",
                         state.status, device_ip);
                return state.status;
            }
        }

        drop(device_states); // Release read lock before starting detection

        // First subscription for this device - start monitoring
        self.start_detection_for_device(device_ip).await;

        eprintln!("🔍 Firewall detection: Started monitoring {} for events (timeout: {:?})",
                 device_ip, self.config.event_wait_timeout);

        FirewallStatus::Unknown
    }

    /// Called when any event is received from a device.
    ///
    /// If detection is in progress for this device, marks it as accessible.
    pub async fn on_event_received(&self, device_ip: IpAddr) {
        let device_states = self.device_states.read().await;

        if let Some(state_arc) = device_states.get(&device_ip) {
            let mut state = state_arc.write().await;

            if !state.detection_completed {
                // First event received - mark as accessible
                state.first_event_time = Some(SystemTime::now());
                state.status = FirewallStatus::Accessible;
                state.detection_completed = true;

                let elapsed = SystemTime::now()
                    .duration_since(state.first_subscription_time)
                    .unwrap_or(Duration::ZERO);

                // Notify completion
                let _ = self.detection_complete_tx.send(DetectionResult {
                    device_ip,
                    status: FirewallStatus::Accessible,
                    reason: DetectionReason::EventReceived,
                });

                eprintln!("✅ Firewall detection: Event received from {} after {:?}, marking as ACCESSIBLE",
                         device_ip, elapsed);
            }
        }
    }

    /// Get the current cached status for a device.
    pub async fn get_device_status(&self, device_ip: IpAddr) -> FirewallStatus {
        let device_states = self.device_states.read().await;

        if let Some(state_arc) = device_states.get(&device_ip) {
            let state = state_arc.read().await;
            state.status
        } else {
            FirewallStatus::Unknown
        }
    }

    /// Clear cached status for a device (useful for testing).
    pub async fn clear_device_cache(&self, device_ip: IpAddr) {
        let mut device_states = self.device_states.write().await;
        device_states.remove(&device_ip);
        eprintln!("🧹 Firewall detection: Cleared cache for {}", device_ip);
    }

    /// Start detection monitoring for a specific device.
    async fn start_detection_for_device(&self, device_ip: IpAddr) {
        let mut device_states = self.device_states.write().await;

        // Create new detection state
        let new_state = Arc::new(RwLock::new(DeviceFirewallState {
            device_ip,
            status: FirewallStatus::Unknown,
            first_subscription_time: SystemTime::now(),
            first_event_time: None,
            detection_completed: false,
            timeout_duration: self.config.event_wait_timeout,
        }));

        // Enforce maximum cache size
        if device_states.len() >= self.config.max_cached_devices {
            // Remove oldest entry (this is a simple LRU-like behavior)
            if let Some(oldest_ip) = device_states.keys().next().copied() {
                device_states.remove(&oldest_ip);
                eprintln!("🧹 Firewall detection: Removed oldest cached entry for {} (cache full)", oldest_ip);
            }
        }

        device_states.insert(device_ip, new_state);
    }

    /// Background task that monitors for timeouts.
    async fn monitor_timeouts(
        device_states: Arc<RwLock<HashMap<IpAddr, Arc<RwLock<DeviceFirewallState>>>>>,
        detection_complete_tx: mpsc::UnboundedSender<DetectionResult>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            interval.tick().await;

            let device_states_read = device_states.read().await;

            for (device_ip, state_arc) in device_states_read.iter() {
                let mut state = state_arc.write().await;

                if !state.detection_completed {
                    let elapsed = SystemTime::now()
                        .duration_since(state.first_subscription_time)
                        .unwrap_or(Duration::ZERO);

                    if elapsed >= state.timeout_duration {
                        // Timeout reached - mark as blocked
                        state.status = FirewallStatus::Blocked;
                        state.detection_completed = true;

                        // Notify completion
                        let _ = detection_complete_tx.send(DetectionResult {
                            device_ip: *device_ip,
                            status: FirewallStatus::Blocked,
                            reason: DetectionReason::Timeout,
                        });

                        eprintln!("🚫 Firewall detection: No events from {} within {:?}, marking as BLOCKED",
                                 device_ip, state.timeout_duration);
                    }
                }
            }
        }
    }

    /// Get statistics about the coordinator state.
    pub async fn get_stats(&self) -> CoordinatorStats {
        let device_states = self.device_states.read().await;

        let mut stats = CoordinatorStats {
            total_devices: device_states.len(),
            accessible_devices: 0,
            blocked_devices: 0,
            unknown_devices: 0,
            error_devices: 0,
        };

        for state_arc in device_states.values() {
            let state = state_arc.read().await;
            match state.status {
                FirewallStatus::Accessible => stats.accessible_devices += 1,
                FirewallStatus::Blocked => stats.blocked_devices += 1,
                FirewallStatus::Unknown => stats.unknown_devices += 1,
                FirewallStatus::Error => stats.error_devices += 1,
            }
        }

        stats
    }
}

/// Statistics about the firewall detection coordinator.
#[derive(Debug, Clone)]
pub struct CoordinatorStats {
    pub total_devices: usize,
    pub accessible_devices: usize,
    pub blocked_devices: usize,
    pub unknown_devices: usize,
    pub error_devices: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_coordinator_creation() {
        let config = FirewallDetectionConfig::default();
        let _coordinator = FirewallDetectionCoordinator::new(config);
        // Just verify it doesn't panic
    }

    #[tokio::test]
    async fn test_first_subscription_starts_monitoring() {
        let config = FirewallDetectionConfig::default();
        let coordinator = FirewallDetectionCoordinator::new(config);

        let device_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let status = coordinator.on_first_subscription(device_ip).await;

        // Should return Unknown while monitoring
        assert_eq!(status, FirewallStatus::Unknown);

        // Should have cached status
        let cached_status = coordinator.get_device_status(device_ip).await;
        assert_eq!(cached_status, FirewallStatus::Unknown);
    }

    #[tokio::test]
    async fn test_event_received_marks_accessible() {
        let config = FirewallDetectionConfig::default();
        let coordinator = FirewallDetectionCoordinator::new(config);

        let device_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));

        // Start monitoring
        coordinator.on_first_subscription(device_ip).await;

        // Simulate event received
        coordinator.on_event_received(device_ip).await;

        // Should now be accessible
        let status = coordinator.get_device_status(device_ip).await;
        assert_eq!(status, FirewallStatus::Accessible);
    }

    #[tokio::test]
    async fn test_timeout_marks_blocked() {
        let config = FirewallDetectionConfig {
            event_wait_timeout: Duration::from_millis(100), // Very short timeout for testing
            ..Default::default()
        };
        let coordinator = FirewallDetectionCoordinator::new(config);

        let device_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));

        // Start monitoring
        coordinator.on_first_subscription(device_ip).await;

        // Wait for timeout + monitoring task to run (monitoring runs every 1 second)
        tokio::time::sleep(Duration::from_millis(1200)).await;

        // Should now be blocked
        let status = coordinator.get_device_status(device_ip).await;
        assert_eq!(status, FirewallStatus::Blocked);
    }

    #[tokio::test]
    async fn test_cached_status_reused() {
        let config = FirewallDetectionConfig::default();
        let coordinator = FirewallDetectionCoordinator::new(config);

        let device_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));

        // Start monitoring and mark as accessible
        coordinator.on_first_subscription(device_ip).await;
        coordinator.on_event_received(device_ip).await;

        // Second subscription should return cached status
        let status = coordinator.on_first_subscription(device_ip).await;
        assert_eq!(status, FirewallStatus::Accessible);
    }

    #[tokio::test]
    async fn test_clear_device_cache() {
        let config = FirewallDetectionConfig::default();
        let coordinator = FirewallDetectionCoordinator::new(config);

        let device_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));

        // Create cached entry
        coordinator.on_first_subscription(device_ip).await;
        coordinator.on_event_received(device_ip).await;

        // Verify cached
        assert_eq!(coordinator.get_device_status(device_ip).await, FirewallStatus::Accessible);

        // Clear cache
        coordinator.clear_device_cache(device_ip).await;

        // Should be unknown again
        assert_eq!(coordinator.get_device_status(device_ip).await, FirewallStatus::Unknown);
    }

    #[tokio::test]
    async fn test_stats() {
        let config = FirewallDetectionConfig::default();
        let coordinator = FirewallDetectionCoordinator::new(config);

        let device1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let device2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 101));

        // One accessible, one unknown
        coordinator.on_first_subscription(device1).await;
        coordinator.on_event_received(device1).await;
        coordinator.on_first_subscription(device2).await;

        let stats = coordinator.get_stats().await;
        assert_eq!(stats.total_devices, 2);
        assert_eq!(stats.accessible_devices, 1);
        assert_eq!(stats.unknown_devices, 1);
        assert_eq!(stats.blocked_devices, 0);
        assert_eq!(stats.error_devices, 0);
    }
}