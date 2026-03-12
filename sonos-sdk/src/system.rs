//! SonosSystem - Main entry point for the SDK
//!
//! Provides a sync-first, DOM-like API for controlling Sonos devices.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use sonos_api::SonosClient;
use sonos_discovery::{self, Device};
use sonos_event_manager::SonosEventManager;
use sonos_state::{GroupId, SpeakerId, StateManager, Topology};

use crate::property::EventInitFn;
use crate::{cache, Group, SdkError, Speaker};

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
///     let speaker = system.speaker("Living Room")
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

    /// Event manager for UPnP subscriptions (lazily initialized on first watch()).
    /// Kept alive here to prevent the Arc from being dropped; the StateManager
    /// holds its own reference via OnceLock for use by watch()/unwatch().
    #[allow(dead_code)]
    event_manager: Mutex<Option<Arc<SonosEventManager>>>,

    /// API client for direct operations
    api_client: SonosClient,

    /// Speaker handles by name
    speakers: RwLock<HashMap<String, Speaker>>,

    /// Timestamp of last rediscovery attempt (seconds since UNIX_EPOCH, 0 = never)
    last_rediscovery: AtomicU64,
}

const REDISCOVERY_COOLDOWN_SECS: u64 = 30;

impl SonosSystem {
    /// Create a new SonosSystem with cache-first device discovery (sync)
    ///
    /// Discovery strategy:
    /// 1. Try loading cached devices from disk (~/.cache/sonos/cache.json)
    /// 2. If cache is fresh (< 24h), use cached devices
    /// 3. If cache is stale, run SSDP; fall back to stale cache if SSDP finds nothing
    /// 4. If no cache exists, run SSDP discovery
    /// 5. If no devices found anywhere, return `Err(SdkError::DiscoveryFailed)`
    pub fn new() -> Result<Self, SdkError> {
        let devices = match cache::load() {
            Some(cached) if !cache::is_stale(&cached) => {
                // Fresh cache — use directly
                cached.devices
            }
            Some(cached) => {
                // Stale cache — try SSDP, fall back to stale data
                let fresh = sonos_discovery::get_with_timeout(Duration::from_secs(3));
                if fresh.is_empty() {
                    tracing::warn!("Cache is stale and SSDP found no devices; using stale cache");
                    cached.devices
                } else {
                    if let Err(e) = cache::save(&fresh) {
                        tracing::warn!("Failed to save discovery cache: {}", e);
                    }
                    fresh
                }
            }
            None => {
                // No cache — full SSDP discovery
                let fresh = sonos_discovery::get_with_timeout(Duration::from_secs(3));
                if fresh.is_empty() {
                    return Err(SdkError::DiscoveryFailed(
                        "no Sonos devices found on the network".to_string(),
                    ));
                }
                if let Err(e) = cache::save(&fresh) {
                    tracing::warn!("Failed to save discovery cache: {}", e);
                }
                fresh
            }
        };

        Self::from_discovered_devices(devices)
    }

    /// Create a new SonosSystem from pre-discovered devices (sync)
    ///
    /// Internal constructor used by `new()` and SDK unit tests.
    /// Also available publicly when the `test-support` feature is enabled
    /// (for integration tests and downstream test code).
    #[cfg(not(feature = "test-support"))]
    pub(crate) fn from_discovered_devices(devices: Vec<Device>) -> Result<Self, SdkError> {
        Self::from_devices_inner(devices)
    }

    /// Create a new SonosSystem from pre-discovered devices (sync)
    ///
    /// Available publicly for integration tests when `test-support` is enabled.
    /// Normal consumers should use [`SonosSystem::new()`] instead.
    #[cfg(feature = "test-support")]
    pub fn from_discovered_devices(devices: Vec<Device>) -> Result<Self, SdkError> {
        Self::from_devices_inner(devices)
    }

    fn from_devices_inner(devices: Vec<Device>) -> Result<Self, SdkError> {
        // 1. Create shared state FIRST — no event manager yet (lazy init)
        let state_manager = Arc::new(
            StateManager::new().map_err(SdkError::StateError)?,
        );
        state_manager
            .add_devices(devices.clone())
            .map_err(SdkError::StateError)?;

        let api_client = SonosClient::new();
        let event_manager: Arc<Mutex<Option<Arc<SonosEventManager>>>> =
            Arc::new(Mutex::new(None));

        // 2. Build init closure from the shared Arcs
        let init_fn: EventInitFn = {
            let em_mutex = Arc::clone(&event_manager);
            let sm = Arc::clone(&state_manager);
            Arc::new(move || {
                let mut guard = em_mutex.lock().map_err(|_| SdkError::LockPoisoned)?;
                if guard.is_some() {
                    return Ok(());
                }
                let em = Arc::new(
                    SonosEventManager::new()
                        .map_err(|e| SdkError::EventManager(e.to_string()))?,
                );
                sm.set_event_manager(Arc::clone(&em))
                    .map_err(SdkError::StateError)?;
                *guard = Some(em);
                Ok(())
            })
        };

        // 3. Build speakers WITH the init closure
        let speakers =
            Self::build_speakers_with_init(&devices, &state_manager, &api_client, Some(&init_fn))?;

        // 4. Assemble struct from the SAME Arcs
        Ok(Self {
            state_manager,
            event_manager: Arc::try_unwrap(event_manager)
                .unwrap_or_else(|arc| {
                    let inner = arc.lock().unwrap().clone();
                    Mutex::new(inner)
                }),
            api_client,
            speakers: RwLock::new(speakers),
            last_rediscovery: AtomicU64::new(0),
        })
    }

    /// Create a test SonosSystem with named speakers and no network access.
    ///
    /// Builds an in-memory system with synthetic speaker data. No SSDP discovery,
    /// no event manager socket binding, no cache reads. Speakers get sequential
    /// IPs starting at `192.168.1.100`.
    ///
    /// Only available when the `test-support` feature is enabled.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let system = SonosSystem::with_speakers(&["Kitchen", "Bedroom"]);
    /// assert_eq!(system.speakers().len(), 2);
    /// assert!(system.speaker("Kitchen").is_some());
    /// ```
    #[cfg(feature = "test-support")]
    pub fn with_speakers(names: &[&str]) -> Self {
        let devices: Vec<Device> = names
            .iter()
            .enumerate()
            .map(|(i, name)| Device {
                id: format!("RINCON_{:03}", i),
                name: name.to_string(),
                room_name: name.to_string(),
                ip_address: format!("192.168.1.{}", 100 + i),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();

        let state_manager = Arc::new(
            StateManager::new().expect("StateManager::new() should not fail"),
        );

        state_manager
            .add_devices(devices.clone())
            .expect("add_devices should not fail with valid test data");

        let api_client = SonosClient::new();
        let speakers = Self::build_speakers_with_init(&devices, &state_manager, &api_client, None)
            .expect("build_speakers should not fail with valid test data");

        Self {
            state_manager,
            event_manager: Mutex::new(None),
            api_client,
            speakers: RwLock::new(speakers),
            last_rediscovery: AtomicU64::new(0),
        }
    }

    /// Build Speaker handles from a list of devices.
    ///
    /// If `event_init` is provided, speakers will trigger lazy event manager
    /// initialization on first `watch()` call.
    fn build_speakers_with_init(
        devices: &[Device],
        state_manager: &Arc<StateManager>,
        api_client: &SonosClient,
        event_init: Option<&EventInitFn>,
    ) -> Result<HashMap<String, Speaker>, SdkError> {
        let mut speakers = HashMap::new();
        for device in devices {
            let speaker_id = SpeakerId::new(&device.id);
            let ip = device
                .ip_address
                .parse()
                .map_err(|_| SdkError::InvalidIpAddress)?;

            let speaker = Speaker::new_with_event_init(
                speaker_id,
                device.name.clone(),
                ip,
                device.model_name.clone(),
                Arc::clone(state_manager),
                api_client.clone(),
                event_init.cloned(),
            );

            speakers.insert(device.name.clone(), speaker);
        }
        Ok(speakers)
    }

    /// Get speaker by name (sync)
    ///
    /// If the speaker isn't in the current map, triggers an SSDP
    /// rediscovery (rate-limited to once per 30s) before returning `None`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let kitchen = sonos.speaker("Kitchen").unwrap();
    /// kitchen.play()?;
    /// ```
    pub fn speaker(&self, name: &str) -> Option<Speaker> {
        if let Some(speaker) = self.speakers.read().ok()?.get(name).cloned() {
            return Some(speaker);
        }
        // Not found — try rediscovery (cooldown-limited)
        self.try_rediscover(name);
        self.speakers.read().ok()?.get(name).cloned()
    }

    /// Get speaker by name (sync)
    #[deprecated(since = "0.2.0", note = "renamed to `speaker()`")]
    pub fn get_speaker_by_name(&self, name: &str) -> Option<Speaker> {
        self.speaker(name)
    }

    /// Run SSDP rediscovery with cooldown. Updates internal speaker map and cache.
    fn try_rediscover(&self, name: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = self.last_rediscovery.load(Ordering::Relaxed);
        if last > 0 && now - last < REDISCOVERY_COOLDOWN_SECS {
            return; // Cooldown period not elapsed
        }
        self.last_rediscovery.store(now, Ordering::Relaxed);

        // 1. SSDP runs WITHOUT holding any lock (3s)
        tracing::info!("speaker '{}' not found, running auto-rediscovery...", name);
        let devices = sonos_discovery::get_with_timeout(Duration::from_secs(3));
        if devices.is_empty() {
            return;
        }

        // 2. Register devices with state manager (required for property tracking)
        if let Err(e) = self.state_manager.add_devices(devices.clone()) {
            tracing::warn!("Failed to register rediscovered devices: {}", e);
            return;
        }

        // 3. Build new Speaker handles (no lock needed)
        let new_speakers = match Self::build_speakers_with_init(&devices, &self.state_manager, &self.api_client, None) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to build speakers from rediscovery: {}", e);
                return;
            }
        };

        // 4. Acquire write lock BRIEFLY for map swap only
        if let Ok(mut map) = self.speakers.write() {
            *map = new_speakers;
        }

        // 5. Save cache (non-fatal on failure)
        if let Err(e) = cache::save(&devices) {
            tracing::warn!("Failed to save discovery cache: {}", e);
        }
    }

    /// Get all speakers (sync)
    pub fn speakers(&self) -> Vec<Speaker> {
        self.speakers
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get speaker by ID (sync)
    pub fn speaker_by_id(&self, speaker_id: &SpeakerId) -> Option<Speaker> {
        let speakers = self.speakers.read().ok()?;
        speakers.values().find(|s| s.id == *speaker_id).cloned()
    }

    /// Get speaker by ID (sync)
    #[deprecated(since = "0.2.0", note = "renamed to `speaker_by_id()`")]
    pub fn get_speaker_by_id(&self, speaker_id: &SpeakerId) -> Option<Speaker> {
        self.speaker_by_id(speaker_id)
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
    // Topology Fetch
    // ========================================================================

    /// Ensure group topology has been fetched.
    ///
    /// If the state manager has no groups (e.g., no ZoneGroupTopology subscription
    /// events have been received yet), this method makes a direct GetZoneGroupState
    /// call to the first available speaker and initializes the state manager with
    /// the result. This is a one-shot operation: once groups are populated,
    /// subsequent calls are a no-op.
    fn ensure_topology(&self) {
        // Fast path: groups already present
        if self.state_manager.group_count() > 0 {
            return;
        }

        // Pick the first speaker IP to query
        let speaker_ip = {
            let speakers = match self.speakers.read() {
                Ok(s) => s,
                Err(_) => return,
            };
            match speakers.values().next() {
                Some(speaker) => speaker.ip.to_string(),
                None => return,
            }
        };

        // Call GetZoneGroupState on that speaker
        let topology_state =
            match sonos_api::services::zone_group_topology::state::poll(&self.api_client, &speaker_ip) {
                Ok(state) => state,
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch zone group topology from {}: {}",
                        speaker_ip,
                        e
                    );
                    return;
                }
            };

        // Decode the API-level topology into state-level GroupInfo values
        let topology_changes = sonos_state::decode_topology_event(&topology_state);

        // Build a Topology with existing speaker data and the freshly fetched groups
        let topology = Topology::new(self.state_manager.speaker_infos(), topology_changes.groups);
        self.state_manager.initialize(topology);

        tracing::debug!(
            "Fetched zone group topology on-demand ({} groups)",
            self.state_manager.group_count()
        );
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
        self.ensure_topology();
        self.state_manager
            .groups()
            .into_iter()
            .filter_map(|info| {
                Group::from_info(
                    info,
                    Arc::clone(&self.state_manager),
                    self.api_client.clone(),
                )
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
    /// if let Some(group) = system.group_by_id(&group_id) {
    ///     println!("Found group with {} members", group.member_count());
    /// }
    /// ```
    pub fn group_by_id(&self, group_id: &GroupId) -> Option<Group> {
        self.ensure_topology();
        let info = self.state_manager.get_group(group_id)?;
        Group::from_info(
            info,
            Arc::clone(&self.state_manager),
            self.api_client.clone(),
        )
    }

    /// Get a specific group by ID (sync)
    #[deprecated(since = "0.2.0", note = "renamed to `group_by_id()`")]
    pub fn get_group_by_id(&self, group_id: &GroupId) -> Option<Group> {
        self.group_by_id(group_id)
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
    /// if let Some(speaker) = system.speaker("Living Room") {
    ///     if let Some(group) = system.group_for_speaker(&speaker.id) {
    ///         println!("{} is in a group with {} speakers",
    ///             speaker.name, group.member_count());
    ///     }
    /// }
    /// ```
    pub fn group_for_speaker(&self, speaker_id: &SpeakerId) -> Option<Group> {
        self.ensure_topology();
        let info = self.state_manager.get_group_for_speaker(speaker_id)?;
        Group::from_info(
            info,
            Arc::clone(&self.state_manager),
            self.api_client.clone(),
        )
    }

    /// Get the group a speaker belongs to (sync)
    #[deprecated(since = "0.2.0", note = "use `speaker.group()` or `group_for_speaker()` instead")]
    pub fn get_group_for_speaker(&self, speaker_id: &SpeakerId) -> Option<Group> {
        self.group_for_speaker(speaker_id)
    }

    /// Get a group by its coordinator speaker name (sync)
    ///
    /// Sonos groups don't have independent names — they are identified by the
    /// coordinator speaker's friendly name. This method matches groups by looking
    /// up the coordinator's name in the state manager.
    ///
    /// Returns `None` if no group's coordinator matches the given name.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(group) = system.group("Living Room") {
    ///     println!("Found group with {} members", group.member_count());
    /// }
    /// ```
    pub fn group(&self, name: &str) -> Option<Group> {
        self.ensure_topology();
        self.state_manager
            .groups()
            .into_iter()
            .find(|info| {
                self.state_manager
                    .speaker_info(&info.coordinator_id)
                    .is_some_and(|si| si.name == name)
            })
            .and_then(|info| {
                Group::from_info(
                    info,
                    Arc::clone(&self.state_manager),
                    self.api_client.clone(),
                )
            })
    }

    /// Get a group by its coordinator speaker name (sync)
    #[deprecated(since = "0.2.0", note = "renamed to `group()`")]
    pub fn get_group_by_name(&self, name: &str) -> Option<Group> {
        self.group(name)
    }

    /// Create a new group with the specified coordinator and members
    ///
    /// Adds each member speaker to the coordinator's current group.
    /// Attempts every speaker even if some fail, returning per-speaker results.
    /// After calling this, re-fetch groups via `groups()` to see the updated topology.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let living_room = system.speaker("Living Room").unwrap();
    /// let kitchen = system.speaker("Kitchen").unwrap();
    /// let bedroom = system.speaker("Bedroom").unwrap();
    ///
    /// let result = system.create_group(&living_room, &[&kitchen, &bedroom])?;
    /// if !result.is_success() {
    ///     for (id, err) in &result.failed {
    ///         eprintln!("Failed to add {}: {}", id, err);
    ///     }
    /// }
    /// ```
    pub fn create_group(
        &self,
        coordinator: &Speaker,
        members: &[&Speaker],
    ) -> Result<crate::group::GroupChangeResult, SdkError> {
        let coord_group = self
            .group_for_speaker(&coordinator.id)
            .ok_or_else(|| SdkError::SpeakerNotFound(coordinator.id.as_str().to_string()))?;

        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for member in members {
            match coord_group.add_speaker(member) {
                Ok(()) => succeeded.push(member.id.clone()),
                Err(e) => failed.push((member.id.clone(), e)),
            }
        }

        Ok(crate::group::GroupChangeResult { succeeded, failed })
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

        let topology = Topology::new(system.state_manager.speaker_infos(), vec![group1, group2]);
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
    fn test_group_by_id_returns_correct_group() {
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
        let group = GroupInfo::new(group_id.clone(), speaker.clone(), vec![speaker.clone()]);

        let topology = Topology::new(system.state_manager.speaker_infos(), vec![group]);
        system.state_manager.initialize(topology);

        // Verify group_by_id returns the correct group
        let found = system.group_by_id(&group_id);
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.id.as_str(), "RINCON_111:1");
        assert_eq!(found.coordinator_id.as_str(), "RINCON_111");
        assert_eq!(found.member_ids.len(), 1);
    }

    #[test]
    fn test_group_by_id_returns_none_for_unknown() {
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
        let found = system.group_by_id(&unknown_id);
        assert!(found.is_none());
    }

    #[test]
    fn test_group_for_speaker_returns_correct_group() {
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

        let topology = Topology::new(system.state_manager.speaker_infos(), vec![group]);
        system.state_manager.initialize(topology);

        // Verify group_for_speaker returns the correct group for both speakers
        let found1 = system.group_for_speaker(&speaker1);
        assert!(found1.is_some());
        let found1 = found1.unwrap();
        assert_eq!(found1.id.as_str(), "RINCON_111:1");
        assert_eq!(found1.member_ids.len(), 2);

        let found2 = system.group_for_speaker(&speaker2);
        assert!(found2.is_some());
        let found2 = found2.unwrap();
        assert_eq!(found2.id.as_str(), "RINCON_111:1");
        assert_eq!(found2.member_ids.len(), 2);
    }

    #[test]
    fn test_group_for_speaker_returns_none_for_unknown() {
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
        let found = system.group_for_speaker(&unknown_speaker);
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
        let group = GroupInfo::new(group_id.clone(), speaker.clone(), vec![speaker.clone()]);

        let topology = Topology::new(system.state_manager.speaker_infos(), vec![group]);
        system.state_manager.initialize(topology);

        // Verify all three methods return consistent data
        let groups = system.groups();
        assert_eq!(groups.len(), 1);

        let by_id = system.group_by_id(&group_id);
        assert!(by_id.is_some());

        let by_speaker = system.group_for_speaker(&speaker);
        assert!(by_speaker.is_some());

        // All should return the same group
        assert_eq!(groups[0].id.as_str(), by_id.as_ref().unwrap().id.as_str());
        assert_eq!(
            groups[0].id.as_str(),
            by_speaker.as_ref().unwrap().id.as_str()
        );
        assert_eq!(
            groups[0].coordinator_id.as_str(),
            by_id.as_ref().unwrap().coordinator_id.as_str()
        );
        assert_eq!(
            groups[0].coordinator_id.as_str(),
            by_speaker.as_ref().unwrap().coordinator_id.as_str()
        );
    }

    #[test]
    fn test_group_by_name_returns_correct_group() {
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

        let topology = Topology::new(system.state_manager.speaker_infos(), vec![group1, group2]);
        system.state_manager.initialize(topology);

        // Find by coordinator name
        let found = system.group("Living Room");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id.as_str(), "RINCON_111:1");

        let found = system.group("Kitchen");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id.as_str(), "RINCON_222:1");

        // Unknown name returns None
        assert!(system.group("Nonexistent").is_none());
    }

    #[test]
    fn test_create_group_method_exists() {
        // Compile-time assertion that method signature is correct
        fn assert_change_result(_r: Result<crate::group::GroupChangeResult, SdkError>) {}

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

        // Initialize topology so group_for_speaker works
        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");
        let group = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            speaker1.clone(),
            vec![speaker1.clone()],
        );
        let topology = Topology::new(system.state_manager.speaker_infos(), vec![group]);
        system.state_manager.initialize(topology);

        let coordinator = system.speaker_by_id(&speaker1).unwrap();
        let member = system.speaker_by_id(&speaker2).unwrap();

        // Will fail at network level but proves signature compiles
        assert_change_result(system.create_group(&coordinator, &[&member]));
    }
}
