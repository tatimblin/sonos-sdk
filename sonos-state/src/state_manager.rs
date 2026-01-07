//! StateManager - main entry point for sonos-state
//!
//! The StateManager coordinates event processing and provides access to the StateStore.
//!
//! # Usage
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, RawEvent};
//!
//! // Create manager with default decoders
//! let mut manager = StateManager::new();
//!
//! // Process incoming events
//! while let Some(event) = event_source.recv().await {
//!     manager.process(event);
//! }
//!
//! // Query state
//! let vol = manager.store().get::<Volume>(&speaker_id);
//!
//! // Watch for changes
//! let mut rx = manager.store().watch::<Volume>(&speaker_id);
//! ```

use std::collections::HashSet;

use sonos_api::Service;

use crate::decoder::{EventDecoder, RawEvent};
use crate::decoders::default_decoders;
use crate::property::Topology;
use crate::store::{StateChange, StateStore};

/// Main state manager that coordinates event processing
///
/// The StateManager is the primary interface for managing Sonos device state.
/// It processes events through decoders and maintains state in the StateStore.
pub struct StateManager {
    /// The state store
    store: StateStore,

    /// Event decoders
    decoders: Vec<Box<dyn EventDecoder>>,
}

impl StateManager {
    /// Create a new StateManager with default decoders
    ///
    /// Default decoders include:
    /// - RenderingControlDecoder (volume, mute, EQ)
    /// - AVTransportDecoder (playback, track, position)
    /// - TopologyDecoder (speakers, groups)
    pub fn new() -> Self {
        Self {
            store: StateStore::new(),
            decoders: default_decoders(),
        }
    }

    /// Create a StateManager with custom decoders
    pub fn with_decoders(decoders: Vec<Box<dyn EventDecoder>>) -> Self {
        Self {
            store: StateStore::new(),
            decoders,
        }
    }

    /// Create a StateManager with an existing store
    ///
    /// Useful for sharing a store between multiple managers or for testing.
    pub fn with_store(store: StateStore) -> Self {
        Self {
            store,
            decoders: default_decoders(),
        }
    }

    /// Initialize from topology data
    ///
    /// This is typically called with data from the first ZoneGroupTopology event
    /// or from discovery to populate initial speaker/group information.
    pub fn initialize(&mut self, topology: Topology) {
        for speaker in &topology.speakers {
            self.store.add_speaker(speaker.clone());
        }
        for group in &topology.groups {
            self.store.add_group(group.clone());
        }
        self.store.set_system(topology);
    }

    /// Process a raw event
    ///
    /// Routes the event to appropriate decoders and applies resulting updates.
    /// Returns the number of property updates that were applied.
    pub fn process(&mut self, event: RawEvent) -> usize {
        let mut update_count = 0;

        for decoder in &self.decoders {
            if decoder.services().contains(&event.service) {
                let updates = decoder.decode(&event, &self.store);
                update_count += updates.len();

                for update in updates {
                    update.apply(&self.store);
                }
            }
        }

        update_count
    }

    /// Get read access to the state store
    ///
    /// Use the store for:
    /// - Querying current values: `store.get::<Volume>(&id)`
    /// - Watching for changes: `store.watch::<Volume>(&id)`
    /// - Iterating speakers/groups: `store.speakers()`
    pub fn store(&self) -> &StateStore {
        &self.store
    }

    /// Get the underlying store (consuming self)
    ///
    /// Useful when you need ownership of the store.
    pub fn into_store(self) -> StateStore {
        self.store
    }

    /// Subscribe to all state changes
    ///
    /// Returns a broadcast receiver that gets all property changes.
    /// Useful for logging, debugging, or building reactive systems.
    pub fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<StateChange> {
        self.store.subscribe_changes()
    }

    /// Get services that have active watchers
    ///
    /// Can be used to determine which UPnP services need subscriptions.
    pub fn active_services(&self) -> HashSet<Service> {
        self.store.active_services()
    }

    /// Check if the manager has been initialized with any speakers
    pub fn is_initialized(&self) -> bool {
        !self.store.is_empty()
    }

    /// Get count of speakers
    pub fn speaker_count(&self) -> usize {
        self.store.speaker_count()
    }

    /// Get count of groups
    pub fn group_count(&self) -> usize {
        self.store.group_count()
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for StateManager {
    fn clone(&self) -> Self {
        // Note: decoders are shared via the same instances,
        // but state store is cloned (shares underlying data)
        Self {
            store: self.store.clone(),
            decoders: default_decoders(), // Create new decoder instances
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{AVTransportData, EventData, RenderingControlData};
    use crate::model::{SpeakerId, SpeakerInfo};
    use crate::property::{GroupInfo, PlaybackState, Volume};

    fn create_test_speaker(id: &str, ip: &str) -> SpeakerInfo {
        SpeakerInfo {
            id: SpeakerId::new(id),
            name: format!("Speaker {}", id),
            room_name: format!("Room {}", id),
            ip_address: ip.parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_new_manager() {
        let manager = StateManager::new();
        assert!(!manager.is_initialized());
        assert_eq!(manager.speaker_count(), 0);
    }

    #[test]
    fn test_initialize() {
        let mut manager = StateManager::new();

        let topology = Topology::new(
            vec![
                create_test_speaker("RINCON_1", "192.168.1.100"),
                create_test_speaker("RINCON_2", "192.168.1.101"),
            ],
            vec![GroupInfo::new(
                crate::model::GroupId::new("GROUP_1"),
                SpeakerId::new("RINCON_1"),
                vec![SpeakerId::new("RINCON_1"), SpeakerId::new("RINCON_2")],
            )],
        );

        manager.initialize(topology);

        assert!(manager.is_initialized());
        assert_eq!(manager.speaker_count(), 2);
        assert_eq!(manager.group_count(), 1);
    }

    #[test]
    fn test_process_event() {
        let mut manager = StateManager::new();

        // Add speaker first
        let speaker = create_test_speaker("RINCON_1", "192.168.1.100");
        manager.store.add_speaker(speaker);

        // Process volume event
        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            EventData::RenderingControl(RenderingControlData::new().with_volume(75)),
        );

        let count = manager.process(event);
        assert_eq!(count, 1);

        // Verify state was updated
        let id = SpeakerId::new("RINCON_1");
        assert_eq!(manager.store().get::<Volume>(&id), Some(Volume::new(75)));
    }

    #[test]
    fn test_process_multiple_events() {
        let mut manager = StateManager::new();

        let speaker = create_test_speaker("RINCON_1", "192.168.1.100");
        manager.store.add_speaker(speaker);

        // Process volume event
        let vol_event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            EventData::RenderingControl(RenderingControlData::new().with_volume(50)),
        );
        manager.process(vol_event);

        // Process playback event
        let play_event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::AVTransport,
            EventData::AVTransport(AVTransportData::new().with_transport_state("PLAYING")),
        );
        manager.process(play_event);

        let id = SpeakerId::new("RINCON_1");
        assert_eq!(manager.store().get::<Volume>(&id), Some(Volume::new(50)));
        assert_eq!(
            manager.store().get::<PlaybackState>(&id),
            Some(PlaybackState::Playing)
        );
    }

    #[tokio::test]
    async fn test_subscribe_changes() {
        let mut manager = StateManager::new();

        let speaker = create_test_speaker("RINCON_1", "192.168.1.100");
        manager.store.add_speaker(speaker);

        let mut rx = manager.subscribe_changes();

        // Process event
        let event = RawEvent::new(
            "192.168.1.100".parse().unwrap(),
            Service::RenderingControl,
            EventData::RenderingControl(RenderingControlData::new().with_volume(50)),
        );
        manager.process(event);

        // Should receive change notification
        let change = rx.try_recv();
        assert!(matches!(
            change,
            Ok(StateChange::SpeakerPropertyChanged {
                property_key: "volume",
                ..
            })
        ));
    }
}
