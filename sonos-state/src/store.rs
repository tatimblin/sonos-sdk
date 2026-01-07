//! Unified state store with reactive watchers
//!
//! The StateStore is the central repository for all Sonos state. It uses
//! `tokio::sync::watch` channels to enable reactive updates - watchers are
//! notified whenever state changes.
//!
//! # Architecture
//!
//! ```text
//! StateStore
//! ├── speaker_props: HashMap<SpeakerId, PropertyBag>
//! │   └── PropertyBag: HashMap<TypeId, watch::Sender<Option<P>>>
//! ├── group_props: HashMap<GroupId, PropertyBag>
//! ├── system_props: PropertyBag
//! ├── speakers: HashMap<SpeakerId, SpeakerInfo>  (metadata)
//! └── groups: HashMap<GroupId, GroupInfo>        (metadata)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! let store = StateStore::new();
//!
//! // Query current value (instant)
//! if let Some(vol) = store.get::<Volume>(&speaker_id) {
//!     println!("Volume: {}%", vol.0);
//! }
//!
//! // Watch for changes (reactive)
//! let mut rx = store.watch::<Volume>(&speaker_id);
//! tokio::spawn(async move {
//!     loop {
//!         rx.changed().await.unwrap();
//!         let vol = rx.borrow().clone();
//!         println!("Volume changed: {:?}", vol);
//!     }
//! });
//! ```

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use tokio::sync::{broadcast, watch};

use sonos_api::Service;

use crate::model::{GroupId, SpeakerId, SpeakerInfo};
use crate::property::{GroupInfo, Property, Scope};

// ============================================================================
// StateChange (for broadcast)
// ============================================================================

/// Represents a state change event
///
/// Emitted whenever any property changes. Subscribe via `store.subscribe_changes()`.
#[derive(Debug, Clone)]
pub enum StateChange {
    /// A speaker property changed
    SpeakerPropertyChanged {
        speaker_id: SpeakerId,
        property_key: &'static str,
        service: Service,
    },
    /// A group property changed
    GroupPropertyChanged {
        group_id: GroupId,
        property_key: &'static str,
        service: Service,
    },
    /// A system property changed
    SystemPropertyChanged {
        property_key: &'static str,
        service: Service,
    },
    /// A speaker was added
    SpeakerAdded { speaker_id: SpeakerId },
    /// A speaker was removed
    SpeakerRemoved { speaker_id: SpeakerId },
    /// A group was added
    GroupAdded { group_id: GroupId },
    /// A group was removed
    GroupRemoved { group_id: GroupId },
}

impl StateChange {
    /// Create a property changed event for the appropriate scope
    pub fn property_changed<P: Property>(target_id: &str) -> Self {
        match P::SCOPE {
            Scope::Speaker => StateChange::SpeakerPropertyChanged {
                speaker_id: SpeakerId::new(target_id),
                property_key: P::KEY,
                service: P::SERVICE,
            },
            Scope::Group => StateChange::GroupPropertyChanged {
                group_id: GroupId::new(target_id),
                property_key: P::KEY,
                service: P::SERVICE,
            },
            Scope::System => StateChange::SystemPropertyChanged {
                property_key: P::KEY,
                service: P::SERVICE,
            },
        }
    }
}

// ============================================================================
// PropertyBag
// ============================================================================

/// A bag of typed properties, each backed by a watch channel
///
/// Uses TypeId for type-safe heterogeneous storage. Each property type gets
/// its own watch channel, enabling per-property subscriptions.
#[derive(Default)]
struct PropertyBag {
    /// Map<TypeId, Box<dyn Any>> where Any is watch::Sender<Option<P>>
    channels: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl PropertyBag {
    fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    /// Get or create a watch channel for a property type
    fn get_or_create_sender<P: Property>(&mut self) -> &watch::Sender<Option<P>> {
        let type_id = TypeId::of::<P>();

        if !self.channels.contains_key(&type_id) {
            let (tx, _rx) = watch::channel::<Option<P>>(None);
            self.channels.insert(type_id, Box::new(tx));
        }

        self.channels
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<watch::Sender<Option<P>>>())
            .expect("PropertyBag: type mismatch (this is a bug)")
    }

    /// Set a property value, returns true if the value changed
    fn set<P: Property>(&mut self, value: P) -> bool {
        let sender = self.get_or_create_sender::<P>();
        let current = sender.borrow().clone();

        if current.as_ref() != Some(&value) {
            // Use send_replace() instead of send() because send() fails when there are no receivers.
            // send_replace() always updates the value regardless of receiver count.
            sender.send_replace(Some(value));
            true
        } else {
            false
        }
    }

    /// Get the current value of a property
    fn get<P: Property>(&self) -> Option<P> {
        let type_id = TypeId::of::<P>();
        self.channels
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<watch::Sender<Option<P>>>())
            .and_then(|sender| sender.borrow().clone())
    }

    /// Get a watch receiver for a property
    fn watch<P: Property>(&mut self) -> watch::Receiver<Option<P>> {
        let sender = self.get_or_create_sender::<P>();
        sender.subscribe()
    }

    /// Clear a property (set to None)
    fn clear<P: Property>(&mut self) -> bool {
        let type_id = TypeId::of::<P>();
        if let Some(boxed) = self.channels.get(&type_id) {
            if let Some(sender) = boxed.downcast_ref::<watch::Sender<Option<P>>>() {
                if sender.borrow().is_some() {
                    let _ = sender.send(None);
                    return true;
                }
            }
        }
        false
    }

    /// Get all services that have properties set in this bag
    fn active_services(&self) -> HashSet<Service> {
        // This would require storing service info per property
        // For now, return empty - we'll track this at StateStore level
        HashSet::new()
    }
}

// ============================================================================
// StateStore
// ============================================================================

/// Unified state store with reactive watchers
///
/// All state in sonos-state flows through this store. Properties can be:
/// - Queried instantly via `get::<P>(id)`
/// - Watched reactively via `watch::<P>(id)` (uses tokio::sync::watch)
///
/// The store is thread-safe via interior mutability (RwLock).
pub struct StateStore {
    /// Speaker properties
    speaker_props: Arc<RwLock<HashMap<SpeakerId, PropertyBag>>>,

    /// Group properties
    group_props: Arc<RwLock<HashMap<GroupId, PropertyBag>>>,

    /// System-wide properties (single bag)
    system_props: Arc<RwLock<PropertyBag>>,

    /// Speaker metadata (static info like name, IP, model)
    speakers: Arc<RwLock<HashMap<SpeakerId, SpeakerInfo>>>,

    /// Group metadata
    groups: Arc<RwLock<HashMap<GroupId, GroupInfo>>>,

    /// IP to speaker ID mapping for event routing
    ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>,

    /// Broadcast channel for all state changes
    changes_tx: broadcast::Sender<StateChange>,
}

impl StateStore {
    /// Create a new empty state store
    pub fn new() -> Self {
        let (changes_tx, _) = broadcast::channel(1000);

        Self {
            speaker_props: Arc::new(RwLock::new(HashMap::new())),
            group_props: Arc::new(RwLock::new(HashMap::new())),
            system_props: Arc::new(RwLock::new(PropertyBag::new())),
            speakers: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
            ip_to_speaker: Arc::new(RwLock::new(HashMap::new())),
            changes_tx,
        }
    }

    // ========================================================================
    // Reading (instant, non-async)
    // ========================================================================

    /// Get current value of a speaker property
    pub fn get<P: Property>(&self, id: &SpeakerId) -> Option<P> {
        self.speaker_props
            .read()
            .ok()?
            .get(id)?
            .get::<P>()
    }

    /// Get current value of a group property
    pub fn get_group<P: Property>(&self, id: &GroupId) -> Option<P> {
        self.group_props
            .read()
            .ok()?
            .get(id)?
            .get::<P>()
    }

    /// Get current value of a system property
    pub fn get_system<P: Property>(&self) -> Option<P> {
        self.system_props.read().ok()?.get::<P>()
    }

    // ========================================================================
    // Watching (reactive)
    // ========================================================================

    /// Get a watch receiver for a speaker property
    ///
    /// The receiver can:
    /// - `.borrow()` to get current value (instant)
    /// - `.changed().await` to wait for changes
    ///
    /// Creates the property slot if it doesn't exist.
    pub fn watch<P: Property>(&self, id: &SpeakerId) -> watch::Receiver<Option<P>> {
        let mut props = self.speaker_props.write().unwrap();
        let bag = props.entry(id.clone()).or_insert_with(PropertyBag::new);
        bag.watch::<P>()
    }

    /// Get a watch receiver for a group property
    pub fn watch_group<P: Property>(&self, id: &GroupId) -> watch::Receiver<Option<P>> {
        let mut props = self.group_props.write().unwrap();
        let bag = props.entry(id.clone()).or_insert_with(PropertyBag::new);
        bag.watch::<P>()
    }

    /// Get a watch receiver for a system property
    pub fn watch_system<P: Property>(&self) -> watch::Receiver<Option<P>> {
        let mut props = self.system_props.write().unwrap();
        props.watch::<P>()
    }

    // ========================================================================
    // Writing (called by decoders)
    // ========================================================================

    /// Set a speaker property, notifies watchers if value changed
    pub fn set<P: Property>(&self, id: &SpeakerId, value: P) {
        let changed = {
            let mut props = self.speaker_props.write().unwrap();
            let bag = props.entry(id.clone()).or_insert_with(PropertyBag::new);
            bag.set(value)
        };

        if changed {
            let _ = self.changes_tx.send(StateChange::SpeakerPropertyChanged {
                speaker_id: id.clone(),
                property_key: P::KEY,
                service: P::SERVICE,
            });
        }
    }

    /// Set a group property, notifies watchers if value changed
    pub fn set_group<P: Property>(&self, id: &GroupId, value: P) {
        let changed = {
            let mut props = self.group_props.write().unwrap();
            let bag = props.entry(id.clone()).or_insert_with(PropertyBag::new);
            bag.set(value)
        };

        if changed {
            let _ = self.changes_tx.send(StateChange::GroupPropertyChanged {
                group_id: id.clone(),
                property_key: P::KEY,
                service: P::SERVICE,
            });
        }
    }

    /// Set a system property, notifies watchers if value changed
    pub fn set_system<P: Property>(&self, value: P) {
        let changed = {
            let mut props = self.system_props.write().unwrap();
            props.set(value)
        };

        if changed {
            let _ = self.changes_tx.send(StateChange::SystemPropertyChanged {
                property_key: P::KEY,
                service: P::SERVICE,
            });
        }
    }

    // ========================================================================
    // Metadata management
    // ========================================================================

    /// Add or update speaker metadata
    pub fn add_speaker(&self, speaker: SpeakerInfo) {
        let id = speaker.id.clone();
        let ip = speaker.ip_address;

        // Add to speakers map
        {
            let mut speakers = self.speakers.write().unwrap();
            let is_new = !speakers.contains_key(&id);
            speakers.insert(id.clone(), speaker);

            if is_new {
                let _ = self
                    .changes_tx
                    .send(StateChange::SpeakerAdded { speaker_id: id.clone() });
            }
        }

        // Update IP mapping
        {
            let mut ip_map = self.ip_to_speaker.write().unwrap();
            ip_map.insert(ip, id);
        }
    }

    /// Remove a speaker
    pub fn remove_speaker(&self, id: &SpeakerId) {
        // Remove from speakers map
        let removed_speaker = {
            let mut speakers = self.speakers.write().unwrap();
            speakers.remove(id)
        };

        if let Some(speaker) = removed_speaker {
            // Remove IP mapping
            {
                let mut ip_map = self.ip_to_speaker.write().unwrap();
                ip_map.remove(&speaker.ip_address);
            }

            // Remove properties
            {
                let mut props = self.speaker_props.write().unwrap();
                props.remove(id);
            }

            let _ = self
                .changes_tx
                .send(StateChange::SpeakerRemoved { speaker_id: id.clone() });
        }
    }

    /// Add or update group metadata
    pub fn add_group(&self, group: GroupInfo) {
        let id = group.id.clone();
        let mut groups = self.groups.write().unwrap();
        let is_new = !groups.contains_key(&id);
        groups.insert(id.clone(), group);

        if is_new {
            let _ = self
                .changes_tx
                .send(StateChange::GroupAdded { group_id: id });
        }
    }

    /// Remove a group
    pub fn remove_group(&self, id: &GroupId) {
        let removed = {
            let mut groups = self.groups.write().unwrap();
            groups.remove(id).is_some()
        };

        if removed {
            // Remove properties
            {
                let mut props = self.group_props.write().unwrap();
                props.remove(id);
            }

            let _ = self
                .changes_tx
                .send(StateChange::GroupRemoved { group_id: id.clone() });
        }
    }

    // ========================================================================
    // Queries
    // ========================================================================

    /// Get speaker metadata by ID
    pub fn speaker(&self, id: &SpeakerId) -> Option<SpeakerInfo> {
        self.speakers.read().ok()?.get(id).cloned()
    }

    /// Get group metadata by ID
    pub fn group(&self, id: &GroupId) -> Option<GroupInfo> {
        self.groups.read().ok()?.get(id).cloned()
    }

    /// Get all speaker metadata
    pub fn speakers(&self) -> Vec<SpeakerInfo> {
        self.speakers
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all group metadata
    pub fn groups(&self) -> Vec<GroupInfo> {
        self.groups
            .read()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get speaker ID for an IP address
    pub fn speaker_id_for_ip(&self, ip: IpAddr) -> Option<SpeakerId> {
        self.ip_to_speaker.read().ok()?.get(&ip).cloned()
    }

    /// Get number of speakers
    pub fn speaker_count(&self) -> usize {
        self.speakers.read().map(|s| s.len()).unwrap_or(0)
    }

    /// Get number of groups
    pub fn group_count(&self) -> usize {
        self.groups.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.speaker_count() == 0
    }

    // ========================================================================
    // Subscription management hints
    // ========================================================================

    /// Subscribe to all state changes (firehose for logging/debugging)
    pub fn subscribe_changes(&self) -> broadcast::Receiver<StateChange> {
        self.changes_tx.subscribe()
    }

    /// Get services that have properties being watched
    ///
    /// This can be used to determine which UPnP services need subscriptions.
    /// Returns services that have at least one active watcher.
    pub fn active_services(&self) -> HashSet<Service> {
        // For now, return all services that have any speakers
        // In the future, we can track which properties are actually being watched
        let mut services = HashSet::new();

        if !self.is_empty() {
            services.insert(Service::RenderingControl);
            services.insert(Service::AVTransport);
            services.insert(Service::ZoneGroupTopology);
        }

        services
    }
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for StateStore {
    fn clone(&self) -> Self {
        Self {
            speaker_props: self.speaker_props.clone(),
            group_props: self.group_props.clone(),
            system_props: self.system_props.clone(),
            speakers: self.speakers.clone(),
            groups: self.groups.clone(),
            ip_to_speaker: self.ip_to_speaker.clone(),
            changes_tx: self.changes_tx.clone(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property::{Mute, Volume};

    #[test]
    fn test_property_bag_direct() {
        let mut bag = PropertyBag::new();

        // Initially None
        assert!(bag.get::<Volume>().is_none(), "Initial value should be None");

        // Set value
        let changed = bag.set(Volume::new(50));
        assert!(changed, "set should return true for new value");

        // Should be able to get it back
        let retrieved = bag.get::<Volume>();
        assert_eq!(retrieved, Some(Volume::new(50)), "Should retrieve set value");
    }

    fn create_test_speaker_info() -> SpeakerInfo {
        SpeakerInfo {
            id: SpeakerId::new("RINCON_123"),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "56.0".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_store_creation() {
        let store = StateStore::new();
        assert!(store.is_empty());
        assert_eq!(store.speaker_count(), 0);
    }

    #[test]
    fn test_add_speaker() {
        let store = StateStore::new();
        let speaker = create_test_speaker_info();
        let id = speaker.id.clone();
        let ip = speaker.ip_address;

        store.add_speaker(speaker);

        assert_eq!(store.speaker_count(), 1);
        assert!(store.speaker(&id).is_some());
        assert_eq!(store.speaker_id_for_ip(ip), Some(id));
    }

    #[test]
    fn test_set_and_get_property() {
        let store = StateStore::new();
        let speaker = create_test_speaker_info();
        let id = speaker.id.clone();
        store.add_speaker(speaker);

        // Initially None
        assert!(store.get::<Volume>(&id).is_none());

        // Set volume
        store.set(&id, Volume::new(50));
        assert_eq!(store.get::<Volume>(&id), Some(Volume::new(50)));

        // Update volume
        store.set(&id, Volume::new(75));
        assert_eq!(store.get::<Volume>(&id), Some(Volume::new(75)));
    }

    #[test]
    fn test_change_detection() {
        let store = StateStore::new();
        let speaker = create_test_speaker_info();
        let id = speaker.id.clone();
        store.add_speaker(speaker);

        let mut rx = store.subscribe_changes();

        // Set property
        store.set(&id, Volume::new(50));

        // Should receive change
        let change = rx.try_recv();
        assert!(matches!(
            change,
            Ok(StateChange::SpeakerPropertyChanged { property_key: "volume", .. })
        ));

        // Set same value - should not emit change
        store.set(&id, Volume::new(50));

        // Try_recv should fail (no message)
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_multiple_properties() {
        let store = StateStore::new();
        let speaker = create_test_speaker_info();
        let id = speaker.id.clone();
        store.add_speaker(speaker);

        store.set(&id, Volume::new(50));
        store.set(&id, Mute::new(true));

        assert_eq!(store.get::<Volume>(&id), Some(Volume::new(50)));
        assert_eq!(store.get::<Mute>(&id), Some(Mute::new(true)));
    }

    #[tokio::test]
    async fn test_watch_property() {
        let store = StateStore::new();
        let speaker = create_test_speaker_info();
        let id = speaker.id.clone();
        store.add_speaker(speaker);

        // Get a watcher before setting
        let mut rx = store.watch::<Volume>(&id);

        // Initial value is None
        assert_eq!(*rx.borrow(), None);

        // Set value
        store.set(&id, Volume::new(50));

        // Should detect change
        assert!(rx.changed().await.is_ok());
        assert_eq!(*rx.borrow(), Some(Volume::new(50)));
    }

    #[test]
    fn test_remove_speaker() {
        let store = StateStore::new();
        let speaker = create_test_speaker_info();
        let id = speaker.id.clone();
        let ip = speaker.ip_address;

        store.add_speaker(speaker);
        store.set(&id, Volume::new(50));

        assert_eq!(store.speaker_count(), 1);

        store.remove_speaker(&id);

        assert_eq!(store.speaker_count(), 0);
        assert!(store.speaker(&id).is_none());
        assert!(store.speaker_id_for_ip(ip).is_none());
        assert!(store.get::<Volume>(&id).is_none());
    }

    #[test]
    fn test_system_properties() {
        use crate::property::Topology;

        let store = StateStore::new();

        assert!(store.get_system::<Topology>().is_none());

        let topology = Topology::new(vec![], vec![]);
        store.set_system(topology.clone());

        assert_eq!(store.get_system::<Topology>(), Some(topology));
    }
}
