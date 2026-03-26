//! Sync-first Sonos Event Manager
//!
//! Provides a fully synchronous API for managing Sonos event subscriptions.
//! All async operations are hidden in a background worker thread.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::mpsc as tokio_mpsc;

use sonos_api::{Service, SpeakerId};
use sonos_discovery::Device;
use sonos_stream::events::EnrichedEvent;
use sonos_stream::BrokerConfig;

use crate::error::{EventManagerError, Result};
use crate::iter::EventManagerIterator;
use crate::worker::{spawn_event_worker, Command};

/// Grace period duration before unsubscribing after last guard drops
const GRACE_PERIOD: Duration = Duration::from_millis(50);

// ============================================================================
// WatchRegistry trait
// ============================================================================

/// Trait for managing the watched-property set.
///
/// Defined in sonos-event-manager, implemented by StateManager in sonos-state.
/// Bridges the two crates without circular dependencies.
pub trait WatchRegistry: Send + Sync + 'static {
    /// Register a property as watched (called during acquire_watch)
    fn register_watch(&self, speaker_id: &SpeakerId, key: &'static str, service: Service);

    /// Unregister all watched properties for a given service on a device.
    /// Called when the grace period expires and the subscription is actually torn down.
    fn unregister_watches_for_service(&self, ip: IpAddr, service: Service);
}

// ============================================================================
// WatchGuard
// ============================================================================

/// RAII guard holding one subscription ref count.
///
/// Each guard represents exactly one reference. When dropped, the ref count is
/// decremented. If it reaches zero, a 50ms grace period starts — if no new
/// `watch()` arrives, the UPnP subscription is torn down.
///
/// Not `Clone`, not `Copy`. Each guard is one hold.
///
/// `WatchGuard` is `Send` but not necessarily `Sync`. This is acceptable for
/// TUI single-thread rendering use cases.
#[must_use = "dropping the guard immediately starts the grace period"]
pub struct WatchGuard {
    event_manager: Arc<SonosEventManager>,
    speaker_id: SpeakerId,
    property_key: &'static str,
    ip: IpAddr,
    service: Service,
}

// Compile-time assertion: WatchGuard must be Send
const _: () = {
    fn assert_send<T: Send>() {}
    fn _check() {
        assert_send::<WatchGuard>();
    }
};

impl fmt::Debug for WatchGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatchGuard")
            .field("speaker_id", &self.speaker_id)
            .field("property_key", &self.property_key)
            .field("ip", &self.ip)
            .field("service", &self.service)
            .finish()
    }
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        // release_watch returns () — panic-free by design
        self.event_manager.release_watch(
            &self.speaker_id,
            self.property_key,
            self.ip,
            self.service,
        );
    }
}

// ============================================================================
// SonosEventManager
// ============================================================================

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
    /// Send commands to background worker (tokio unbounded — send() is sync)
    command_tx: tokio_mpsc::UnboundedSender<Command>,

    /// Receive events from background worker
    event_rx: Arc<Mutex<mpsc::Receiver<EnrichedEvent>>>,

    /// Device info cache (sync access)
    devices: Arc<RwLock<HashMap<IpAddr, Device>>>,

    /// Service subscription ref counts (sync access)
    service_refs: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>,

    /// Pending grace-period timers: cancelled via AtomicBool when re-acquired
    pending_unsubscribes: parking_lot::Mutex<HashMap<(IpAddr, Service), Arc<AtomicBool>>>,

    /// Watch registry for managing the watched-property set (set once)
    watch_registry: OnceLock<Arc<dyn WatchRegistry>>,

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
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel();

        // Spawn background worker with its own tokio runtime
        let worker = spawn_event_worker(config, command_rx, event_tx);

        Ok(Self {
            command_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            devices: Arc::new(RwLock::new(HashMap::new())),
            service_refs: Arc::new(RwLock::new(HashMap::new())),
            pending_unsubscribes: parking_lot::Mutex::new(HashMap::new()),
            watch_registry: OnceLock::new(),
            _worker: worker,
        })
    }

    /// Set the watch registry (called once by StateManager during initialization).
    ///
    /// Subsequent calls are no-ops.
    pub fn set_watch_registry(&self, registry: Arc<dyn WatchRegistry>) {
        let _ = self.watch_registry.set(registry);
    }

    // ========================================================================
    // Watch lifecycle (grace period API)
    // ========================================================================

    /// Acquire a watch on a property, returning an RAII guard.
    ///
    /// Increments the service ref count. If this is the first reference (and no
    /// grace period is pending), sends a Subscribe command to the worker. If a
    /// grace period is active for this (ip, service), cancels it instead.
    ///
    /// Also registers the (speaker_id, key) pair in the WatchRegistry so that
    /// change events are forwarded for this property.
    pub fn acquire_watch(
        self: &Arc<Self>,
        speaker_id: &SpeakerId,
        property_key: &'static str,
        ip: IpAddr,
        service: Service,
    ) -> Result<WatchGuard> {
        // 1. Register in watched set via WatchRegistry
        if let Some(registry) = self.watch_registry.get() {
            registry.register_watch(speaker_id, property_key, service);
        }

        // 2. Increment ref count + check if we need to subscribe
        let should_subscribe = {
            let mut refs = self.service_refs.write();
            let count = refs.entry((ip, service)).or_insert(0);
            let was_zero = *count == 0;
            *count += 1;

            tracing::debug!(
                "acquire_watch: ref count for {}:{:?}: {} -> {}",
                ip,
                service,
                if was_zero { 0 } else { *count - 1 },
                *count
            );

            was_zero
        };

        if should_subscribe {
            // 3. Check for pending grace period to cancel
            let cancelled = self
                .pending_unsubscribes
                .lock()
                .remove(&(ip, service))
                .map(|flag| {
                    flag.store(true, Ordering::SeqCst);
                    true
                })
                .unwrap_or(false);

            if cancelled {
                tracing::debug!(
                    "acquire_watch: cancelled grace period for {}:{:?}",
                    ip,
                    service
                );
            } else {
                // No pending grace period — actually subscribe
                self.command_tx
                    .send(Command::Subscribe { ip, service })
                    .map_err(|_| EventManagerError::WorkerDisconnected)?;
            }
        }

        Ok(WatchGuard {
            event_manager: Arc::clone(self),
            speaker_id: speaker_id.clone(),
            property_key,
            ip,
            service,
        })
    }

    /// Release a watch (called from WatchGuard::Drop). Must never panic.
    ///
    /// Decrements the service ref count. If it hits zero, starts a grace period:
    /// spawns a thread that sleeps for 50ms, then sends Unsubscribe if not
    /// cancelled.
    pub(crate) fn release_watch(
        &self,
        _speaker_id: &SpeakerId,
        _property_key: &'static str,
        ip: IpAddr,
        service: Service,
    ) {
        let should_start_grace = {
            let mut refs = self.service_refs.write();

            if let Some(count) = refs.get_mut(&(ip, service)) {
                *count = count.saturating_sub(1);

                tracing::debug!(
                    "release_watch: ref count for {}:{:?}: {} -> {}",
                    ip,
                    service,
                    *count + 1,
                    *count
                );

                if *count == 0 {
                    refs.remove(&(ip, service));
                    true
                } else {
                    false
                }
            } else {
                tracing::warn!("release_watch: no ref count for {}:{:?}", ip, service);
                false
            }
        };

        if should_start_grace {
            let cancelled = Arc::new(AtomicBool::new(false));
            self.pending_unsubscribes
                .lock()
                .insert((ip, service), Arc::clone(&cancelled));

            let tx = self.command_tx.clone();
            let registry = self.watch_registry.get().cloned();

            std::thread::spawn(move || {
                std::thread::sleep(GRACE_PERIOD);

                if !cancelled.load(Ordering::SeqCst) {
                    tracing::debug!(
                        "Grace period expired for {}:{:?}, unsubscribing",
                        ip,
                        service
                    );

                    // Unsubscribe from UPnP service
                    let _ = tx.send(Command::Unsubscribe { ip, service });

                    // Clean up watched set
                    if let Some(registry) = registry {
                        registry.unregister_watches_for_service(ip, service);
                    }
                }
            });
        }
    }

    // ========================================================================
    // Device management
    // ========================================================================

    /// Add discovered devices to the manager (sync)
    ///
    /// Stores device information for later lookup. Does not automatically
    /// subscribe to any services.
    pub fn add_devices(&self, devices: Vec<Device>) -> Result<()> {
        let mut device_map = self.devices.write();

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
        self.devices.read().values().cloned().collect()
    }

    /// Get a specific device by IP address (sync)
    pub fn device_by_ip(&self, ip: IpAddr) -> Option<Device> {
        self.devices.read().get(&ip).cloned()
    }

    // ========================================================================
    // Direct subscription management (used by existing code paths)
    // ========================================================================

    /// Ensure a service is subscribed for a device (sync, ref-counted)
    ///
    /// Increments the reference count for the (device_ip, service) pair.
    /// If this is the first reference, triggers a subscription via the background worker.
    pub fn ensure_service_subscribed(&self, device_ip: IpAddr, service: Service) -> Result<()> {
        let should_subscribe = {
            let mut refs = self.service_refs.write();

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
            let mut refs = self.service_refs.write();

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

    // ========================================================================
    // Event iteration
    // ========================================================================

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

    // ========================================================================
    // Stats / introspection
    // ========================================================================

    /// Get current service subscription statistics (sync)
    pub fn service_subscription_stats(&self) -> HashMap<(IpAddr, Service), usize> {
        self.service_refs.read().clone()
    }

    /// Check if a service is currently subscribed for a device (sync)
    pub fn is_service_subscribed(&self, device_ip: IpAddr, service: Service) -> bool {
        self.service_refs
            .read()
            .get(&(device_ip, service))
            .is_some_and(|&c| c > 0)
    }

    /// Get the current reference count for a service subscription
    pub fn service_ref_count(&self, device_ip: IpAddr, service: Service) -> usize {
        self.service_refs
            .read()
            .get(&(device_ip, service))
            .copied()
            .unwrap_or(0)
    }

    /// Shutdown the background worker
    ///
    /// Called automatically on drop, but can be called manually for graceful shutdown.
    pub fn shutdown(&self) {
        // Cancel all pending grace timers
        let pending: Vec<_> = self.pending_unsubscribes.lock().drain().collect();
        for ((ip, service), flag) in pending {
            flag.store(true, Ordering::SeqCst);
            // Send unsubscribe immediately (no grace period on shutdown)
            let _ = self.command_tx.send(Command::Unsubscribe { ip, service });
            // Clean up watched set
            if let Some(registry) = self.watch_registry.get() {
                registry.unregister_watches_for_service(ip, service);
            }
        }

        let _ = self.command_tx.send(Command::Shutdown);
    }
}

impl Drop for SonosEventManager {
    fn drop(&mut self) {
        tracing::debug!(
            "SonosEventManager dropping, {} active service subscriptions",
            self.service_refs.read().len()
        );

        // Cancel all pending grace timers
        let pending: Vec<_> = self.pending_unsubscribes.lock().drain().collect();
        for (_, flag) in &pending {
            flag.store(true, Ordering::SeqCst);
        }

        // Send shutdown command to worker
        let _ = self.command_tx.send(Command::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Mock WatchRegistry for testing
    struct MockRegistry {
        register_count: AtomicUsize,
        unregister_count: AtomicUsize,
    }

    impl MockRegistry {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                register_count: AtomicUsize::new(0),
                unregister_count: AtomicUsize::new(0),
            })
        }

        fn registers(&self) -> usize {
            self.register_count.load(Ordering::SeqCst)
        }

        fn unregisters(&self) -> usize {
            self.unregister_count.load(Ordering::SeqCst)
        }
    }

    impl WatchRegistry for MockRegistry {
        fn register_watch(&self, _speaker_id: &SpeakerId, _key: &'static str, _service: Service) {
            self.register_count.fetch_add(1, Ordering::SeqCst);
        }

        fn unregister_watches_for_service(&self, _ip: IpAddr, _service: Service) {
            self.unregister_count.fetch_add(1, Ordering::SeqCst);
        }
    }

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

    #[test]
    fn test_acquire_release_watch_ref_counting() {
        let manager = Arc::new(SonosEventManager::new().unwrap());
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire first watch
        let guard1 = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 1);

        // Acquire second watch (same service)
        let guard2 = manager
            .acquire_watch(&speaker_id, "mute", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 2);

        // Drop first guard
        drop(guard1);
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 1);

        // Drop second guard — ref count hits 0, grace period starts
        drop(guard2);
        assert_eq!(manager.service_ref_count(ip, Service::RenderingControl), 0);
    }

    #[test]
    fn test_grace_period_cancelled_by_reacquire() {
        let manager = Arc::new(SonosEventManager::new().unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire and drop — starts grace period
        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        drop(guard);

        // Verify grace timer is pending
        assert!(manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));

        // Re-acquire within grace period — should cancel the timer
        let _guard2 = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();

        // Pending should be cleared
        assert!(!manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));

        // Registry should NOT have unregistered (grace period was cancelled)
        assert_eq!(registry.unregisters(), 0);
    }

    #[test]
    fn test_grace_period_fires_after_timeout() {
        let manager = Arc::new(SonosEventManager::new().unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire and drop
        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(registry.registers(), 1);
        drop(guard);

        // Wait for grace period to expire
        std::thread::sleep(Duration::from_millis(100));

        // Registry should have unregistered
        assert_eq!(registry.unregisters(), 1);
    }

    #[test]
    fn test_watch_registry_integration() {
        let manager = Arc::new(SonosEventManager::new().unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire registers in watched set
        let _guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        assert_eq!(registry.registers(), 1);
        assert_eq!(registry.unregisters(), 0);
    }

    #[test]
    fn test_guard_drop_with_disconnected_worker() {
        let manager = Arc::new(SonosEventManager::new().unwrap());
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();

        // Shutdown the worker
        manager.shutdown();

        // Dropping guard should not panic even with disconnected worker
        drop(guard);
    }

    #[test]
    fn test_multiple_services_independent_grace_periods() {
        let manager = Arc::new(SonosEventManager::new().unwrap());
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire watches on different services
        let guard_rc = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        let guard_av = manager
            .acquire_watch(&speaker_id, "playback_state", ip, Service::AVTransport)
            .unwrap();

        // Drop RC — starts grace period for RenderingControl only
        drop(guard_rc);
        assert!(manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));
        assert!(!manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::AVTransport)));

        // AVTransport still has ref count 1
        assert_eq!(manager.service_ref_count(ip, Service::AVTransport), 1);

        drop(guard_av);
    }

    #[test]
    fn test_shutdown_drains_pending_grace_timers() {
        let manager = Arc::new(SonosEventManager::new().unwrap());
        let registry = MockRegistry::new();
        manager.set_watch_registry(registry.clone());

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        // Acquire and drop to start grace period
        let guard = manager
            .acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)
            .unwrap();
        drop(guard);

        // Grace timer should be pending
        assert!(manager
            .pending_unsubscribes
            .lock()
            .contains_key(&(ip, Service::RenderingControl)));

        // Shutdown should drain and cancel pending timers
        manager.shutdown();

        // Pending should be cleared
        assert!(manager.pending_unsubscribes.lock().is_empty());
    }
}
