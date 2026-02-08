//! Sync-first State Management for Sonos devices
//!
//! Provides a synchronous API for managing Sonos device state with
//! background event processing.
//!
//! # Example
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, Volume};
//! use sonos_discovery;
//!
//! // Create state manager (sync)
//! let manager = StateManager::new()?;
//! let devices = sonos_discovery::get();
//! manager.add_devices(devices)?;
//!
//! // Get speakers
//! for info in manager.speaker_infos() {
//!     println!("{}: {}", info.name, info.ip_address);
//! }
//!
//! // Blocking iteration over changes
//! for event in manager.iter() {
//!     println!("Change: {:?}", event);
//! }
//! ```

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sonos_api::Service;
use sonos_discovery::Device;
use sonos_event_manager::SonosEventManager;
use tracing::info;

use crate::event_worker::spawn_state_event_worker;
use crate::iter::ChangeIterator;
use crate::model::{GroupId, SpeakerId, SpeakerInfo};
use crate::property::{GroupInfo, Property, SonosProperty, Topology};
use crate::{Result, StateError};

// ============================================================================
// ChangeEvent - for iter()
// ============================================================================

/// A change event emitted when a watched property changes
#[derive(Debug, Clone)]
pub struct ChangeEvent {
    /// Speaker or entity that changed
    pub speaker_id: SpeakerId,
    /// Property key that changed
    pub property_key: &'static str,
    /// Service the property belongs to
    pub service: Service,
    /// When the change occurred
    pub timestamp: Instant,
}

impl ChangeEvent {
    pub fn new(speaker_id: SpeakerId, property_key: &'static str, service: Service) -> Self {
        Self {
            speaker_id,
            property_key,
            service,
            timestamp: Instant::now(),
        }
    }
}

// ============================================================================
// Internal StateStore
// ============================================================================

/// Internal state storage
pub struct StateStore {
    /// Speaker metadata
    pub(crate) speakers: HashMap<SpeakerId, SpeakerInfo>,
    /// IP to speaker ID mapping
    pub(crate) ip_to_speaker: HashMap<IpAddr, SpeakerId>,
    /// Property values: (speaker_id, property_key) -> type-erased value
    pub(crate) speaker_props: HashMap<SpeakerId, PropertyBag>,
    /// Group metadata
    pub(crate) groups: HashMap<GroupId, GroupInfo>,
    /// Group properties
    pub(crate) group_props: HashMap<GroupId, PropertyBag>,
    /// System properties
    pub(crate) system_props: PropertyBag,
}

impl StateStore {
    pub(crate) fn new() -> Self {
        Self {
            speakers: HashMap::new(),
            ip_to_speaker: HashMap::new(),
            speaker_props: HashMap::new(),
            groups: HashMap::new(),
            group_props: HashMap::new(),
            system_props: PropertyBag::new(),
        }
    }

    pub(crate) fn add_speaker(&mut self, speaker: SpeakerInfo) {
        let id = speaker.id.clone();
        let ip = speaker.ip_address;
        self.ip_to_speaker.insert(ip, id.clone());
        self.speakers.insert(id.clone(), speaker);
        self.speaker_props.entry(id).or_insert_with(PropertyBag::new);
    }

    fn speaker(&self, id: &SpeakerId) -> Option<&SpeakerInfo> {
        self.speakers.get(id)
    }

    fn speakers(&self) -> Vec<SpeakerInfo> {
        self.speakers.values().cloned().collect()
    }

    fn add_group(&mut self, group: GroupInfo) {
        let id = group.id.clone();
        self.groups.insert(id.clone(), group);
        self.group_props.entry(id).or_insert_with(PropertyBag::new);
    }

    pub(crate) fn get<P: Property>(&self, speaker_id: &SpeakerId) -> Option<P> {
        self.speaker_props.get(speaker_id)?.get::<P>()
    }

    pub(crate) fn set<P: Property>(&mut self, speaker_id: &SpeakerId, value: P) -> bool {
        let bag = self.speaker_props.entry(speaker_id.clone()).or_insert_with(PropertyBag::new);
        bag.set(value)
    }

    fn set_system<P: Property>(&mut self, value: P) -> bool {
        self.system_props.set(value)
    }

    fn is_empty(&self) -> bool {
        self.speakers.is_empty()
    }

    fn speaker_count(&self) -> usize {
        self.speakers.len()
    }

    fn group_count(&self) -> usize {
        self.groups.len()
    }
}

// ============================================================================
// PropertyBag - type-erased property storage
// ============================================================================

pub(crate) struct PropertyBag {
    /// Map<TypeId, Box<dyn Any>> where Any is the property value
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl PropertyBag {
    pub(crate) fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    fn get<P: Property>(&self) -> Option<P> {
        let type_id = TypeId::of::<P>();
        self.values
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<P>())
            .cloned()
    }

    fn set<P: Property>(&mut self, value: P) -> bool {
        let type_id = TypeId::of::<P>();
        let current = self.values
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<P>());

        if current != Some(&value) {
            self.values.insert(type_id, Box::new(value));
            true
        } else {
            false
        }
    }
}

// ============================================================================
// StateManager - main entry point
// ============================================================================

/// Core state manager with sync-first API
///
/// All public methods are synchronous. Background event processing
/// happens in a dedicated thread.
pub struct StateManager {
    /// Property values storage
    store: Arc<RwLock<StateStore>>,

    /// Watched properties for iter() filtering
    watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,

    /// Service subscription ref counts: (speaker_ip, service) -> count
    subscriptions: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>,

    /// IP to speaker ID mapping (for event worker)
    ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>,

    /// Event manager (optional - enables live events)
    event_manager: Option<Arc<SonosEventManager>>,

    /// Channel for sending change events to iter()
    event_tx: mpsc::Sender<ChangeEvent>,

    /// Receiver for iter() - wrapped in Arc<Mutex> for cloning
    event_rx: Arc<Mutex<mpsc::Receiver<ChangeEvent>>>,

    /// Background event processor handle (kept alive)
    _worker: Option<JoinHandle<()>>,

    /// Cleanup timeout for subscriptions
    cleanup_timeout: Duration,
}

impl StateManager {
    /// Create a new StateManager with default settings (sync)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let manager = StateManager::new()?;
    /// ```
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Create a StateManager builder for custom configuration
    pub fn builder() -> StateManagerBuilder {
        StateManagerBuilder::default()
    }

    /// Add discovered devices (sync)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let devices = sonos_discovery::get();
    /// manager.add_devices(devices)?;
    /// ```
    pub fn add_devices(&self, devices: Vec<Device>) -> Result<()> {
        let mut store = self.store.write().map_err(|_| StateError::LockPoisoned)?;
        let mut ip_map = self
            .ip_to_speaker
            .write()
            .map_err(|_| StateError::LockPoisoned)?;

        for device in devices {
            let speaker_id = SpeakerId::new(&device.id);
            let ip: IpAddr = device
                .ip_address
                .parse()
                .map_err(|_| StateError::InvalidIpAddress(device.ip_address.clone()))?;

            let info = SpeakerInfo {
                id: speaker_id.clone(),
                name: device.name.clone(),
                room_name: device.name.clone(),
                ip_address: ip,
                port: device.port,
                model_name: device.model_name.clone(),
                software_version: "unknown".to_string(),
                satellites: vec![],
            };

            // Update ip_to_speaker mapping
            ip_map.insert(ip, speaker_id.clone());
            tracing::debug!(
                "Added speaker {} at IP {} to ip_to_speaker map",
                speaker_id.as_str(),
                ip
            );

            store.add_speaker(info);
        }

        // Also add devices to event manager if present
        drop(store);
        drop(ip_map);

        if let Some(em) = &self.event_manager {
            let devices_for_em: Vec<_> = self
                .speaker_infos()
                .iter()
                .map(|info| sonos_discovery::Device {
                    id: info.id.as_str().to_string(),
                    name: info.name.clone(),
                    room_name: info.room_name.clone(),
                    ip_address: info.ip_address.to_string(),
                    port: info.port,
                    model_name: info.model_name.clone(),
                })
                .collect();

            if let Err(e) = em.add_devices(devices_for_em) {
                tracing::warn!("Failed to add devices to event manager: {}", e);
            }
        }

        Ok(())
    }

    /// Get all speaker info
    pub fn speaker_infos(&self) -> Vec<SpeakerInfo> {
        let store = match self.store.read() {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        store.speakers()
    }

    /// Get a specific speaker info by ID
    pub fn speaker_info(&self, speaker_id: &SpeakerId) -> Option<SpeakerInfo> {
        let store = self.store.read().ok()?;
        store.speaker(speaker_id).cloned()
    }

    /// Get speaker IP by ID
    pub fn get_speaker_ip(&self, speaker_id: &SpeakerId) -> Option<IpAddr> {
        let store = self.store.read().ok()?;
        store.speaker(speaker_id).map(|s| s.ip_address)
    }

    /// Create a blocking iterator over change events
    ///
    /// Only emits events for properties that have been watched.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // First, watch some properties
    /// speaker.volume.watch()?;
    ///
    /// // Then iterate over changes
    /// for event in manager.iter() {
    ///     println!("Changed: {} on {}", event.property_key, event.speaker_id);
    /// }
    /// ```
    pub fn iter(&self) -> ChangeIterator {
        ChangeIterator::new(Arc::clone(&self.event_rx))
    }

    /// Get current property value (sync, no subscription)
    pub fn get_property<P: Property>(&self, speaker_id: &SpeakerId) -> Option<P> {
        let store = self.store.read().ok()?;
        store.get::<P>(speaker_id)
    }

    /// Set a property value
    ///
    /// Updates the property value in the store and emits a change event
    /// if the property is being watched.
    pub fn set_property<P: SonosProperty>(&self, speaker_id: &SpeakerId, value: P) {
        let changed = {
            let mut store = match self.store.write() {
                Ok(s) => s,
                Err(_) => return,
            };
            store.set::<P>(speaker_id, value)
        };

        if changed {
            self.maybe_emit_change(speaker_id, P::KEY, P::SERVICE);
        }
    }

    /// Register a property as watched (called by PropertyHandle::watch)
    pub fn register_watch(&self, speaker_id: &SpeakerId, property_key: &'static str) {
        if let Ok(mut watched) = self.watched.write() {
            watched.insert((speaker_id.clone(), property_key));
        }
    }

    /// Unregister a property watch
    pub fn unregister_watch(&self, speaker_id: &SpeakerId, property_key: &'static str) {
        if let Ok(mut watched) = self.watched.write() {
            watched.remove(&(speaker_id.clone(), property_key));
        }
    }

    /// Watch a property with automatic UPnP subscription (recommended API)
    ///
    /// This is the preferred method for watching properties as it:
    /// 1. Registers the property for change notifications
    /// 2. Subscribes to the UPnP service via the event manager
    ///
    /// Returns the current cached value if available.
    pub fn watch_property_with_subscription<P: SonosProperty>(
        &self,
        speaker_id: &SpeakerId,
    ) -> Result<Option<P>> {
        // Register for change notifications
        self.register_watch(speaker_id, P::KEY);

        // Subscribe via event manager if available
        if let Some(em) = &self.event_manager {
            // Get speaker IP from store
            if let Some(ip) = self.get_speaker_ip(speaker_id) {
                if let Err(e) = em.ensure_service_subscribed(ip, P::SERVICE) {
                    tracing::warn!(
                        "Failed to subscribe to {:?} for {}: {}",
                        P::SERVICE,
                        speaker_id.as_str(),
                        e
                    );
                }
            }
        }

        Ok(self.get_property::<P>(speaker_id))
    }

    /// Unwatch a property and release UPnP subscription
    pub fn unwatch_property_with_subscription<P: SonosProperty>(&self, speaker_id: &SpeakerId) {
        // Unregister from change notifications
        self.unregister_watch(speaker_id, P::KEY);

        // Release subscription via event manager if available
        if let Some(em) = &self.event_manager {
            if let Some(ip) = self.get_speaker_ip(speaker_id) {
                if let Err(e) = em.release_service_subscription(ip, P::SERVICE) {
                    tracing::warn!(
                        "Failed to unsubscribe from {:?} for {}: {}",
                        P::SERVICE,
                        speaker_id.as_str(),
                        e
                    );
                }
            }
        }
    }

    /// Check if a property is being watched
    pub fn is_watched(&self, speaker_id: &SpeakerId, property_key: &'static str) -> bool {
        self.watched.read()
            .map(|w| w.contains(&(speaker_id.clone(), property_key)))
            .unwrap_or(false)
    }

    /// Emit a change event if the property is being watched
    fn maybe_emit_change(&self, speaker_id: &SpeakerId, property_key: &'static str, service: Service) {
        let is_watched = self.watched.read()
            .map(|w| w.contains(&(speaker_id.clone(), property_key)))
            .unwrap_or(false);

        if is_watched {
            let event = ChangeEvent::new(speaker_id.clone(), property_key, service);
            let _ = self.event_tx.send(event);
        }
    }

    /// Initialize from topology data
    pub fn initialize(&self, topology: Topology) {
        if let Ok(mut store) = self.store.write() {
            for speaker in &topology.speakers {
                store.add_speaker(speaker.clone());
            }
            for group in &topology.groups {
                store.add_group(group.clone());
            }
            store.set_system(topology);
        }
    }

    /// Check if initialized with any speakers
    pub fn is_initialized(&self) -> bool {
        self.store.read()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    /// Get number of speakers
    pub fn speaker_count(&self) -> usize {
        self.store.read()
            .map(|s| s.speaker_count())
            .unwrap_or(0)
    }

    /// Get number of groups
    pub fn group_count(&self) -> usize {
        self.store.read()
            .map(|s| s.group_count())
            .unwrap_or(0)
    }

    /// Get access to the event manager (if configured)
    ///
    /// This allows PropertyHandle::watch() to trigger UPnP subscriptions
    /// via the event manager's ensure_service_subscribed() method.
    pub fn event_manager(&self) -> Option<&Arc<SonosEventManager>> {
        self.event_manager.as_ref()
    }

}

impl Clone for StateManager {
    fn clone(&self) -> Self {
        Self {
            store: Arc::clone(&self.store),
            watched: Arc::clone(&self.watched),
            subscriptions: Arc::clone(&self.subscriptions),
            ip_to_speaker: Arc::clone(&self.ip_to_speaker),
            event_manager: self.event_manager.clone(),
            event_tx: self.event_tx.clone(),
            event_rx: Arc::clone(&self.event_rx),
            _worker: None,
            cleanup_timeout: self.cleanup_timeout,
        }
    }
}

// ============================================================================
// StateManagerBuilder
// ============================================================================

/// Builder for StateManager configuration
pub struct StateManagerBuilder {
    cleanup_timeout: Duration,
    event_manager: Option<Arc<SonosEventManager>>,
}

impl Default for StateManagerBuilder {
    fn default() -> Self {
        Self {
            cleanup_timeout: Duration::from_secs(5),
            event_manager: None,
        }
    }
}

impl StateManagerBuilder {
    /// Set the cleanup timeout for subscriptions
    pub fn cleanup_timeout(mut self, timeout: Duration) -> Self {
        self.cleanup_timeout = timeout;
        self
    }

    /// Set the event manager for live event processing
    ///
    /// When an event manager is provided, the StateManager will:
    /// - Spawn a background worker to process events
    /// - Automatically subscribe/unsubscribe via `watch()`/`unwatch()` on properties
    /// - Update state from incoming events
    pub fn with_event_manager(mut self, em: Arc<SonosEventManager>) -> Self {
        self.event_manager = Some(em);
        self
    }

    /// Build the StateManager
    pub fn build(self) -> Result<StateManager> {
        let (event_tx, event_rx) = mpsc::channel();

        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(HashSet::new()));
        let ip_to_speaker = Arc::new(RwLock::new(HashMap::new()));

        // Spawn event worker if event_manager provided
        let worker = if let Some(ref em) = self.event_manager {
            let worker_handle = spawn_state_event_worker(
                Arc::clone(em),
                Arc::clone(&store),
                Arc::clone(&watched),
                event_tx.clone(),
                Arc::clone(&ip_to_speaker),
            );
            info!("StateManager event worker started");
            Some(worker_handle)
        } else {
            None
        };

        let manager = StateManager {
            store,
            watched,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            ip_to_speaker,
            event_manager: self.event_manager,
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            _worker: worker,
            cleanup_timeout: self.cleanup_timeout,
        };

        info!("StateManager created (sync-first mode)");
        Ok(manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property::Volume;

    #[test]
    fn test_state_manager_creation() {
        let manager = StateManager::new().unwrap();
        assert!(!manager.is_initialized());
        assert_eq!(manager.speaker_count(), 0);
    }

    #[test]
    fn test_add_devices() {
        let manager = StateManager::new().unwrap();

        let devices = vec![Device {
            id: "RINCON_123".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        manager.add_devices(devices).unwrap();
        assert_eq!(manager.speaker_count(), 1);
    }

    #[test]
    fn test_property_storage() {
        let manager = StateManager::new().unwrap();

        let devices = vec![Device {
            id: "RINCON_123".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        manager.add_devices(devices).unwrap();

        let speaker_id = SpeakerId::new("RINCON_123");

        // Initially None
        assert!(manager.get_property::<Volume>(&speaker_id).is_none());

        // Set value
        manager.set_property(&speaker_id, Volume::new(50));
        assert_eq!(manager.get_property::<Volume>(&speaker_id), Some(Volume::new(50)));
    }

    #[test]
    fn test_watch_registration() {
        let manager = StateManager::new().unwrap();

        let devices = vec![Device {
            id: "RINCON_123".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        manager.add_devices(devices).unwrap();

        let speaker_id = SpeakerId::new("RINCON_123");

        // Not watched initially
        assert!(!manager.is_watched(&speaker_id, "volume"));

        // Register watch
        manager.register_watch(&speaker_id, "volume");
        assert!(manager.is_watched(&speaker_id, "volume"));

        // Unregister watch
        manager.unregister_watch(&speaker_id, "volume");
        assert!(!manager.is_watched(&speaker_id, "volume"));
    }

    #[test]
    fn test_change_event_emission() {
        let manager = StateManager::new().unwrap();

        let devices = vec![Device {
            id: "RINCON_123".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        manager.add_devices(devices).unwrap();

        let speaker_id = SpeakerId::new("RINCON_123");

        // Register watch
        manager.register_watch(&speaker_id, "volume");

        // Set property (should emit event)
        manager.set_property(&speaker_id, Volume::new(75));

        // Get event via iter
        let iter = manager.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.speaker_id.as_str(), "RINCON_123");
        assert_eq!(event.property_key, "volume");
    }
}
