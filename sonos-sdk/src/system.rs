//! SonosSystem - Main entry point for the SDK
//!
//! Provides a sync-first, DOM-like API for controlling Sonos devices.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use sonos_api::SonosClient;
use sonos_discovery::{self, Device};
use sonos_event_manager::SonosEventManager;
use sonos_state::{GroupId, SpeakerId, StateManager};

use crate::{Group, SdkError, Speaker};

/// Main system entry point - provides DOM-like API
///
/// SonosSystem is fully synchronous - no async/await required.
///
/// # Example
///
/// ```rust,ignore
/// use sonos_sdk::SonosSystem;
///
/// fn main() -> Result<(), sonos_sdk::SdkError> {
///     let system = SonosSystem::new()?;
///
///     // Get speaker by name
///     let speaker = system.get_speaker_by_name("Living Room")
///         .ok_or_else(|| sonos_sdk::SdkError::SpeakerNotFound("Living Room".to_string()))?;
///
///     // Three methods on each property:
///     let volume = speaker.volume.get();              // Get cached value
///     let fresh_volume = speaker.volume.fetch()?;     // API call + update cache
///     let current = speaker.volume.watch()?;          // Start watching for changes
///
///     // Iterate over changes
///     for event in system.iter() {
///         println!("Property changed: {:?}", event);
///     }
///
///     Ok(())
/// }
/// ```
pub struct SonosSystem {
    /// State manager for property values
    state_manager: Arc<StateManager>,

    /// Event manager for UPnP subscriptions (kept alive)
    _event_manager: Arc<SonosEventManager>,

    /// API client for direct operations (kept for future use)
    _api_client: SonosClient,

    /// Speaker handles by name
    speakers: RwLock<HashMap<String, Speaker>>,
}

impl SonosSystem {
    /// Create a new SonosSystem with automatic device discovery (sync)
    ///
    /// This will:
    /// 1. Discover Sonos devices on the network
    /// 2. Create event manager for UPnP subscriptions
    /// 3. Create state manager for property tracking
    /// 4. Create speaker handles for each device
    pub fn new() -> Result<Self, SdkError> {
        // Discover devices first
        let devices = sonos_discovery::get();

        Self::from_discovered_devices(devices)
    }

    /// Create a new SonosSystem from pre-discovered devices (sync)
    ///
    /// Use this when you already have a list of devices from `sonos_discovery::get()`.
    pub fn from_discovered_devices(devices: Vec<Device>) -> Result<Self, SdkError> {
        // Create event manager (sync)
        let event_manager =
            Arc::new(SonosEventManager::new().map_err(|e| SdkError::EventManager(e.to_string()))?);

        // Create state manager with event manager wired up (sync)
        let state_manager = Arc::new(
            StateManager::builder()
                .with_event_manager(Arc::clone(&event_manager))
                .build()
                .map_err(SdkError::StateError)?,
        );

        // Add devices
        state_manager
            .add_devices(devices.clone())
            .map_err(SdkError::StateError)?;

        let api_client = SonosClient::new();

        // Create speaker handles
        let mut speakers = HashMap::new();
        for device in devices {
            let speaker_id = SpeakerId::new(&device.id);
            let ip = device
                .ip_address
                .parse()
                .map_err(|_| SdkError::InvalidIpAddress)?;

            let speaker = Speaker::new(
                speaker_id,
                device.name.clone(),
                ip,
                device.model_name.clone(),
                Arc::clone(&state_manager),
                api_client.clone(),
            );

            speakers.insert(device.name, speaker);
        }

        Ok(Self {
            state_manager,
            _event_manager: event_manager,
            _api_client: api_client,
            speakers: RwLock::new(speakers),
        })
    }

    /// Get speaker by name (sync)
    ///
    /// Returns `None` if no speaker with that name exists.
    pub fn get_speaker_by_name(&self, name: &str) -> Option<Speaker> {
        self.speakers.read().ok()?.get(name).cloned()
    }

    /// Get all speakers (sync)
    pub fn speakers(&self) -> Vec<Speaker> {
        self.speakers
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get speaker by ID (sync)
    pub fn get_speaker_by_id(&self, speaker_id: &SpeakerId) -> Option<Speaker> {
        let speakers = self.speakers.read().ok()?;
        speakers.values().find(|s| s.id == *speaker_id).cloned()
    }

    /// Get all speaker names (sync)
    pub fn speaker_names(&self) -> Vec<String> {
        self.speakers
            .read()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get the state manager for advanced usage
    pub fn state_manager(&self) -> &Arc<StateManager> {
        &self.state_manager
    }

    /// Get a blocking iterator over property change events
    ///
    /// Only emits events for properties that have been `watch()`ed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // First, watch some properties
    /// speaker.volume.watch()?;
    /// speaker.playback_state.watch()?;
    ///
    /// // Then iterate over changes (blocking)
    /// for event in system.iter() {
    ///     println!("Changed: {} on {}", event.property_key, event.speaker_id);
    /// }
    /// ```
    pub fn iter(&self) -> sonos_state::ChangeIterator {
        self.state_manager.iter()
    }

    // ========================================================================
    // Group Methods
    // ========================================================================

    /// Get all current groups (sync)
    ///
    /// Returns all groups in the system. Every speaker is always in a group,
    /// so a single speaker forms a group of one.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for group in system.groups() {
    ///     println!("Group: {} ({} members)", group.id, group.member_count());
    ///     if let Some(coordinator) = group.coordinator() {
    ///         println!("  Coordinator: {}", coordinator.name);
    ///     }
    /// }
    /// ```
    pub fn groups(&self) -> Vec<Group> {
        self.state_manager
            .groups()
            .into_iter()
            .filter_map(|info| {
                Group::from_info(info, Arc::clone(&self.state_manager), self._api_client.clone())
            })
            .collect()
    }

    /// Get a specific group by ID (sync)
    ///
    /// Returns `None` if no group with that ID exists.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(group) = system.get_group_by_id(&group_id) {
    ///     println!("Found group with {} members", group.member_count());
    /// }
    /// ```
    pub fn get_group_by_id(&self, group_id: &GroupId) -> Option<Group> {
        let info = self.state_manager.get_group(group_id)?;
        Group::from_info(
            info,
            Arc::clone(&self.state_manager),
            self._api_client.clone(),
        )
    }

    /// Get the group a speaker belongs to (sync)
    ///
    /// Returns `None` if the speaker is not found or has no group.
    /// Since all speakers are always in a group, this typically only returns
    /// `None` if the speaker ID is invalid.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(speaker) = system.get_speaker_by_name("Living Room") {
    ///     if let Some(group) = system.get_group_for_speaker(&speaker.id) {
    ///         println!("{} is in a group with {} speakers",
    ///             speaker.name, group.member_count());
    ///     }
    /// }
    /// ```
    pub fn get_group_for_speaker(&self, speaker_id: &SpeakerId) -> Option<Group> {
        let info = self.state_manager.get_group_for_speaker(speaker_id)?;
        Group::from_info(
            info,
            Arc::clone(&self.state_manager),
            self._api_client.clone(),
        )
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use sonos_state::{GroupInfo, Topology};

    /// Create a test SonosSystem with the given devices
    /// 
    /// Note: This requires network access for the event manager.
    /// Tests using this helper should be run with actual network connectivity
    /// or mocked appropriately.
    fn create_test_system(devices: Vec<Device>) -> Result<SonosSystem, SdkError> {
        SonosSystem::from_discovered_devices(devices)
    }

    #[test]
    fn test_groups_returns_all_groups() {
        let devices = vec![
            Device {
                id: "RINCON_111".to_string(),
                name: "Living Room".to_string(),
                room_name: "Living Room".to_string(),
                ip_address: "192.168.1.100".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
            Device {
                id: "RINCON_222".to_string(),
                name: "Kitchen".to_string(),
                room_name: "Kitchen".to_string(),
                ip_address: "192.168.1.101".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
        ];

        let system = create_test_system(devices).unwrap();

        // Initialize with topology containing groups
        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");
        let group1 = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            speaker1.clone(),
            vec![speaker1.clone()],
        );
        let group2 = GroupInfo::new(
            GroupId::new("RINCON_222:1"),
            speaker2.clone(),
            vec![speaker2.clone()],
        );

        let topology = Topology::new(
            system.state_manager.speaker_infos(),
            vec![group1, group2],
        );
        system.state_manager.initialize(topology);

        // Verify groups() returns all groups
        let groups = system.groups();
        assert_eq!(groups.len(), 2);

        let group_ids: Vec<_> = groups.iter().map(|g| g.id.as_str().to_string()).collect();
        assert!(group_ids.contains(&"RINCON_111:1".to_string()));
        assert!(group_ids.contains(&"RINCON_222:1".to_string()));
    }

    #[test]
    fn test_groups_returns_empty_when_no_groups() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = create_test_system(devices).unwrap();

        // No topology initialized, so no groups
        let groups = system.groups();
        assert!(groups.is_empty());
    }

    #[test]
    fn test_get_group_by_id_returns_correct_group() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = create_test_system(devices).unwrap();

        // Initialize with topology
        let speaker = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");
        let group = GroupInfo::new(
            group_id.clone(),
            speaker.clone(),
            vec![speaker.clone()],
        );

        let topology = Topology::new(
            system.state_manager.speaker_infos(),
            vec![group],
        );
        system.state_manager.initialize(topology);

        // Verify get_group_by_id returns the correct group
        let found = system.get_group_by_id(&group_id);
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id.as_str(), "RINCON_111:1");
        assert_eq!(found.coordinator_id.as_str(), "RINCON_111");
        assert_eq!(found.member_ids.len(), 1);
    }

    #[test]
    fn test_get_group_by_id_returns_none_for_unknown() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = create_test_system(devices).unwrap();

        // No groups initialized
        let unknown_id = GroupId::new("RINCON_UNKNOWN:1");
        let found = system.get_group_by_id(&unknown_id);
        assert!(found.is_none());
    }

    #[test]
    fn test_get_group_for_speaker_returns_correct_group() {
        let devices = vec![
            Device {
                id: "RINCON_111".to_string(),
                name: "Living Room".to_string(),
                room_name: "Living Room".to_string(),
                ip_address: "192.168.1.100".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
            Device {
                id: "RINCON_222".to_string(),
                name: "Kitchen".to_string(),
                room_name: "Kitchen".to_string(),
                ip_address: "192.168.1.101".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
        ];

        let system = create_test_system(devices).unwrap();

        // Initialize with a group containing both speakers
        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");
        let group = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            speaker1.clone(),
            vec![speaker1.clone(), speaker2.clone()],
        );

        let topology = Topology::new(
            system.state_manager.speaker_infos(),
            vec![group],
        );
        system.state_manager.initialize(topology);

        // Verify get_group_for_speaker returns the correct group for both speakers
        let found1 = system.get_group_for_speaker(&speaker1);
        assert!(found1.is_some());
        let found1 = found1.unwrap();
        assert_eq!(found1.id.as_str(), "RINCON_111:1");
        assert_eq!(found1.member_ids.len(), 2);

        let found2 = system.get_group_for_speaker(&speaker2);
        assert!(found2.is_some());
        let found2 = found2.unwrap();
        assert_eq!(found2.id.as_str(), "RINCON_111:1");
        assert_eq!(found2.member_ids.len(), 2);
    }

    #[test]
    fn test_get_group_for_speaker_returns_none_for_unknown() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = create_test_system(devices).unwrap();

        // No groups initialized
        let unknown_speaker = SpeakerId::new("RINCON_UNKNOWN");
        let found = system.get_group_for_speaker(&unknown_speaker);
        assert!(found.is_none());
    }

    #[test]
    fn test_group_methods_consistency() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = create_test_system(devices).unwrap();

        // Initialize with topology
        let speaker = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");
        let group = GroupInfo::new(
            group_id.clone(),
            speaker.clone(),
            vec![speaker.clone()],
        );

        let topology = Topology::new(
            system.state_manager.speaker_infos(),
            vec![group],
        );
        system.state_manager.initialize(topology);

        // Verify all three methods return consistent data
        let groups = system.groups();
        assert_eq!(groups.len(), 1);

        let by_id = system.get_group_by_id(&group_id);
        assert!(by_id.is_some());

        let by_speaker = system.get_group_for_speaker(&speaker);
        assert!(by_speaker.is_some());

        // All should return the same group
        assert_eq!(groups[0].id.as_str(), by_id.as_ref().unwrap().id.as_str());
        assert_eq!(groups[0].id.as_str(), by_speaker.as_ref().unwrap().id.as_str());
        assert_eq!(groups[0].coordinator_id.as_str(), by_id.as_ref().unwrap().coordinator_id.as_str());
        assert_eq!(groups[0].coordinator_id.as_str(), by_speaker.as_ref().unwrap().coordinator_id.as_str());
    }
}
