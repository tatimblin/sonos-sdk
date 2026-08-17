//! SonosSystem - Main entry point for the SDK
//!
//! Provides a sync-first, DOM-like API for controlling Sonos devices.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use sonos_api::SonosClient;
use sonos_discovery::{self, Device};
use sonos_event_manager::SonosEventManager;

#[cfg(feature = "test-support")]
use sonos_state::GroupInfo;
use sonos_state::{EventInitFn, GroupId, SpeakerId, StateManager, Topology};

use crate::{cache, Group, SdkError, Speaker};

/// Compute the display name for a device.
///
/// Prefers `room_name` (user-assigned in the Sonos app, e.g., "Kitchen").
/// Falls back to `name` (UPnP `friendlyName`) when `room_name` is absent or unknown.
fn display_name(device: &Device) -> String {
    if device.room_name.is_empty() || device.room_name == "Unknown" {
        device.name.clone()
    } else {
        device.room_name.clone()
    }
}

/// Find a speaker by name with case-insensitive fallback.
///
/// Tries an exact O(1) HashMap lookup first, then falls back to
/// case-insensitive iteration (O(n), typically n < 50).
fn find_speaker_by_name(speakers: &HashMap<String, Speaker>, name: &str) -> Option<Speaker> {
    if let Some(speaker) = speakers.get(name) {
        return Some(speaker.clone());
    }
    speakers
        .values()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .cloned()
}

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
    /// State manager for property values.
    ///
    /// Also the sole owner of the lazily-created `SonosEventManager`, which it
    /// holds in a `OnceLock`. `SonosSystem` deliberately keeps no second handle:
    /// the field that used to sit here claimed to be "kept alive here to prevent
    /// the Arc from being dropped" but was permanently `None`, because the
    /// `Arc::try_unwrap` that populated it could never succeed while the
    /// init closure held the other reference. Since `state_manager` outlives
    /// every `watch()` anyway, one owner is all that was ever needed.
    state_manager: Arc<StateManager>,

    /// API client for direct operations
    api_client: SonosClient,

    /// Speaker handles by name
    speakers: RwLock<HashMap<String, Speaker>>,

    /// Timestamp of last rediscovery attempt (seconds since UNIX_EPOCH, 0 = never)
    last_rediscovery: AtomicU64,

    /// When true, this system never touches the network on its own: topology
    /// prefetch (`ensure_topology`) and lookup-miss rediscovery
    /// (`try_rediscover`) both become no-ops.
    ///
    /// Set by the test constructors only; production paths leave it `false` so
    /// behavior is unchanged.
    offline: bool,
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

    /// Create a SonosSystem from pre-discovered devices WITHOUT any network I/O.
    ///
    /// Runs the same construction sequence as the normal constructor, except
    /// that the topology *poll* is skipped and the system is marked `offline` so
    /// a lookup miss cannot trigger SSDP rediscovery. The topology-dependent
    /// steps still execute; with no topology they simply have nothing to do
    /// (the satellite set is empty, and no IPs have changed).
    ///
    /// Use [`Self::from_devices_offline_with_topology`] to supply the topology
    /// the poll would have returned.
    ///
    /// Exists because the two network paths in the normal constructor
    /// (topology SOAP poll, rediscovery SSDP) dominate test wall time: each
    /// unreachable speaker IP costs a 5s connect + 10s read timeout, and a
    /// single lookup miss costs a 3s SSDP sweep. Tests that only exercise
    /// in-memory bookkeeping should pay none of that.
    ///
    /// Only available when the `test-support` feature is enabled (or when
    /// compiling this crate's own test harness).
    #[cfg(any(feature = "test-support", test))]
    pub fn from_devices_offline(devices: Vec<Device>) -> Result<Self, SdkError> {
        Self::construct(devices, true, |_| {})
    }

    /// Construct offline, but inject the topology `ensure_topology` would have
    /// polled, so the post-topology construction steps still run.
    ///
    /// `seed` is called after the state manager exists and before the
    /// satellite-aware re-key, which is exactly the window `ensure_topology`
    /// occupies in production. It is the only way a test can exercise satellite
    /// filtering — the behavior is defined entirely by topology, and topology
    /// otherwise arrives only over the network.
    ///
    /// Tests go through [`Self::construct`] rather than poking the speaker map
    /// afterwards on purpose: filtering that runs in the wrong *order* is the
    /// entire bug, so a test that reproduces the steps itself could not detect
    /// the production sequence regressing.
    #[cfg(any(feature = "test-support", test))]
    pub fn from_devices_offline_with_topology(
        devices: Vec<Device>,
        seed: impl FnOnce(&Self),
    ) -> Result<Self, SdkError> {
        Self::construct(devices, true, seed)
    }

    fn from_devices_inner(devices: Vec<Device>) -> Result<Self, SdkError> {
        Self::construct(devices, false, |_| {})
    }

    /// The single construction sequence: in-memory wiring, topology, then the
    /// steps that depend on topology.
    ///
    /// Production and the offline test constructors share this body so the
    /// *order* of the topology-dependent steps has exactly one definition.
    fn construct(
        devices: Vec<Device>,
        offline: bool,
        seed: impl FnOnce(&Self),
    ) -> Result<Self, SdkError> {
        let system = Self::assemble(devices.clone(), offline)?;

        // Tests inject the topology that `ensure_topology` would have fetched.
        seed(&system);

        // Prefetch topology before any subscriptions can start.
        // This ensures group structure is known when the first AVTransport
        // events arrive, so PerCoordinator suppression/propagation works
        // from the very first event.
        // No-op when offline, or when `seed` already supplied groups.
        system.ensure_topology();

        // Re-key the speaker map now that satellites are known.
        system.rebuild_speakers_excluding_satellites(&devices)?;

        // Refresh Speaker handle IPs from state store (topology may have updated them)
        if let Ok(mut speakers) = system.speakers.write() {
            for speaker in speakers.values_mut() {
                if let Some(info) = system.state_manager.speaker_info(&speaker.id) {
                    speaker.ip = info.ip_address;
                }
            }
        }

        Ok(system)
    }

    /// Build the in-memory system: state manager, lazy event-init closure,
    /// API client and Speaker handles. Performs no network I/O.
    ///
    /// Shared by [`Self::from_devices_inner`] and [`Self::from_devices_offline`]
    /// so the Arc wiring below has exactly one definition.
    ///
    /// # Why the closure holds a `Weak<StateManager>`
    ///
    /// The closure below is *stored on the very `StateManager` it needs to call*
    /// (`set_event_init` puts it in a `OnceLock` on the manager). Capturing a
    /// strong `Arc<StateManager>` therefore closed a reference cycle: manager →
    /// `OnceLock<EventInitFn>` → closure → manager. Neither end could ever reach
    /// zero, so dropping a `SonosSystem` freed nothing — a measured
    /// `Arc::strong_count` of 2 after `drop(system)` where 1 was expected. Each
    /// construction permanently leaked the `StateManager`, its `StateStore`, the
    /// event-worker thread, the `SonosEventManager` with its tokio runtime, and
    /// the callback server's UDP/TCP socket.
    ///
    /// A `Weak` breaks the cycle without changing the happy path: while the
    /// system is alive the upgrade always succeeds, and the only way it can fail
    /// is a `watch()` racing teardown, where doing nothing is exactly right.
    fn assemble(devices: Vec<Device>, offline: bool) -> Result<Self, SdkError> {
        // 1. Create shared state FIRST — no event manager yet (lazy init)
        let state_manager = Arc::new(StateManager::new().map_err(SdkError::StateError)?);
        state_manager
            .add_devices(devices.clone())
            .map_err(SdkError::StateError)?;

        let api_client = SonosClient::new();

        // 2. Build init closure and store on StateManager (single source of truth)
        let init_fn: EventInitFn = {
            // Serializes concurrent first-`watch()` calls so at most one
            // SonosEventManager is ever constructed. `set_event_manager` is
            // itself idempotent, but without this lock a race would still bind
            // two callback sockets and spawn two runtimes before one lost.
            let init_lock: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
            let weak_sm = Arc::downgrade(&state_manager);
            Arc::new(
                move || -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
                    let mut initialized = init_lock.lock().map_err(|_| SdkError::LockPoisoned)?;
                    if *initialized {
                        tracing::trace!(
                            "Event manager init closure called but already initialized"
                        );
                        return Ok(());
                    }
                    // A failed upgrade means the SonosSystem is being torn down
                    // while a watch() is in flight. There is nothing left to
                    // wire an event manager into, so decline quietly rather than
                    // building a runtime and a socket for a dead system.
                    let Some(sm) = weak_sm.upgrade() else {
                        tracing::debug!(
                            "Event manager init skipped: SonosSystem has already been dropped"
                        );
                        return Ok(());
                    };
                    tracing::info!("Lazy-initializing event manager (first watch() call)");
                    let em = Arc::new(SonosEventManager::new().map_err(|e| {
                        tracing::error!("Failed to create SonosEventManager: {}", e);
                        SdkError::EventManager(e.to_string())
                    })?);
                    tracing::debug!("SonosEventManager created, wiring into StateManager");
                    // The StateManager owns the only lasting reference, in its
                    // own OnceLock. SonosSystem deliberately keeps none: a
                    // second copy of this handle bought nothing and previously
                    // pretended to be the thing keeping it alive.
                    sm.set_event_manager(em).map_err(SdkError::StateError)?;
                    *initialized = true;
                    tracing::info!("Event manager initialization complete");
                    Ok(())
                },
            )
        };
        state_manager.set_event_init(init_fn);

        // 3. Build speakers (init fn is on StateManager — no per-speaker threading needed).
        //
        // No topology has been fetched yet, so satellite identity is unknown and
        // every device is a candidate. `from_devices_inner` rebuilds this map
        // once `ensure_topology` has run; the offline constructors have no
        // topology to consult and keep this provisional map as final.
        let speakers =
            Self::build_speakers(&devices, &HashSet::new(), &state_manager, &api_client)?;

        // 4. Assemble struct from the SAME Arcs
        Ok(Self {
            state_manager,
            api_client,
            speakers: RwLock::new(speakers),
            last_rediscovery: AtomicU64::new(0),
            offline,
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
                id: format!("RINCON_{i:03}"),
                name: name.to_string(),
                room_name: name.to_string(),
                ip_address: format!("192.168.1.{}", 100 + i),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();

        let state_manager =
            Arc::new(StateManager::new().expect("StateManager::new() should not fail"));

        state_manager
            .add_devices(devices.clone())
            .expect("add_devices should not fail with valid test data");

        let api_client = SonosClient::new();
        let speakers = Self::build_speakers(&devices, &HashSet::new(), &state_manager, &api_client)
            .expect("build_speakers should not fail with valid test data");

        Self {
            state_manager,
            api_client,
            speakers: RwLock::new(speakers),
            last_rediscovery: AtomicU64::new(0),
            offline: true,
        }
    }

    /// Create a test SonosSystem with speakers AND group topology.
    ///
    /// Each speaker gets a standalone group (coordinator = self, members = [self]).
    /// This makes `system.groups()` and `system.group("name")` work in tests.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let system = SonosSystem::with_groups(&["Kitchen", "Bedroom"]);
    /// assert_eq!(system.groups().len(), 2);
    /// assert!(system.group("Kitchen").is_some());
    /// ```
    #[cfg(feature = "test-support")]
    pub fn with_groups(names: &[&str]) -> Self {
        let system = Self::with_speakers(names);

        let groups: Vec<GroupInfo> = names
            .iter()
            .enumerate()
            .map(|(i, _name)| {
                let speaker_id = SpeakerId::new(format!("RINCON_{i:03}"));
                let group_id = GroupId::new(format!("RINCON_{i:03}:1"));
                GroupInfo::new(group_id, speaker_id.clone(), vec![speaker_id])
            })
            .collect();

        let topology = Topology::new(system.state_manager.speaker_infos(), groups);
        system.state_manager.initialize(topology);

        system
    }

    /// Re-key the name-keyed speaker map with satellite devices excluded.
    ///
    /// Satellite identity comes from topology, which is only available *after*
    /// `ensure_topology()`; the map key comes from the device list, which is
    /// available from the start. This method is the one place both facts are
    /// known, so it is where the map can first be built correctly — which is why
    /// it rebuilds from `devices` instead of filtering the provisional map.
    ///
    /// Filtering the provisional map in place was the bug. A bonded home theater
    /// is one visible coordinator plus N invisible satellites, all reporting the
    /// same `room_name`, so all of them hash to one key and only one survives
    /// insertion. When the survivor was a satellite, the filter then deleted it
    /// and the whole room disappeared — the controllable coordinator having
    /// already been overwritten. Rebuilding with satellites skipped up front
    /// means the coordinator is the only candidate for the key.
    ///
    /// No-op when no satellites are known, which keeps the offline constructors
    /// (no topology fetch, empty satellite set) on their existing behavior.
    fn rebuild_speakers_excluding_satellites(&self, devices: &[Device]) -> Result<(), SdkError> {
        let satellite_ids: HashSet<SpeakerId> =
            self.state_manager.get_satellite_ids().into_iter().collect();
        if satellite_ids.is_empty() {
            return Ok(());
        }

        let rebuilt = Self::build_speakers(
            devices,
            &satellite_ids,
            &self.state_manager,
            &self.api_client,
        )?;
        if let Ok(mut speakers) = self.speakers.write() {
            *speakers = rebuilt;
        }
        tracing::debug!("Excluded {} satellite speakers", satellite_ids.len());
        Ok(())
    }

    /// Build the name-keyed Speaker map from a list of devices.
    ///
    /// `satellite_ids` are devices marked `Invisible="1"` in the topology
    /// (home-theater surrounds and subs). They are skipped **before** insertion,
    /// not filtered afterwards, because insertion is what collides: every device
    /// in a bonded set reports the same `room_name`, so all of them produce the
    /// same map key. Filtering after the fact can only inspect whichever device
    /// happened to win that collision, and if the winner was a satellite the
    /// entire room is deleted along with it. Skipping first guarantees the
    /// visible coordinator — the one that accepts playback and volume commands —
    /// is the device that reaches the map.
    ///
    /// Pass an empty set when satellite identity is not yet known; every device
    /// is then a candidate, which is the pre-topology status quo.
    ///
    /// Two *genuinely visible* devices sharing a room name remain a real
    /// conflict. Sonos itself prevents this in the app, so it means unusual
    /// state (a rename mid-discovery, a stale cache entry for a replaced unit).
    /// Rather than silently discarding one, both are kept: the first-seen device
    /// holds the plain room name and later ones are suffixed with their speaker
    /// ID, so `speaker("Basement")` stays stable and
    /// `speaker("Basement (RINCON_2)")` reaches the other. Nothing is lost, and
    /// `speakers()` / `speaker_by_id()` see the true device count.
    fn build_speakers(
        devices: &[Device],
        satellite_ids: &HashSet<SpeakerId>,
        state_manager: &Arc<StateManager>,
        api_client: &SonosClient,
    ) -> Result<HashMap<String, Speaker>, SdkError> {
        let mut speakers = HashMap::new();
        for device in devices {
            let speaker_id = SpeakerId::new(&device.id);

            // Skip satellites before they can claim the name key.
            if satellite_ids.contains(&speaker_id) {
                tracing::debug!(
                    "skipping satellite speaker {} in room \"{}\"",
                    device.id,
                    display_name(device)
                );
                continue;
            }

            let ip = device
                .ip_address
                .parse()
                .map_err(|_| SdkError::InvalidIpAddress)?;

            let base_name = display_name(device);
            let key = if speakers.contains_key(&base_name) {
                let disambiguated = format!("{base_name} ({})", device.id);
                tracing::warn!(
                    "two visible speakers report the name \"{}\"; registering the second as \"{}\"",
                    base_name,
                    disambiguated
                );
                disambiguated
            } else {
                base_name
            };

            let speaker = Speaker::new(
                speaker_id,
                key.clone(),
                ip,
                device.model_name.clone(),
                Arc::clone(state_manager),
                api_client.clone(),
            );

            speakers.insert(key, speaker);
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
        {
            let speakers = self.speakers.read().ok()?;
            if let Some(speaker) = find_speaker_by_name(&speakers, name) {
                return Some(speaker);
            }
        }
        // Not found — try rediscovery (cooldown-limited)
        self.try_rediscover(name);
        let speakers = self.speakers.read().ok()?;
        find_speaker_by_name(&speakers, name)
    }

    /// Get speaker by name (sync)
    #[deprecated(since = "0.2.0", note = "renamed to `speaker()`")]
    pub fn get_speaker_by_name(&self, name: &str) -> Option<Speaker> {
        self.speaker(name)
    }

    /// Run SSDP rediscovery with cooldown. Updates internal speaker map and cache.
    ///
    /// No-op for offline systems (test constructors) so a lookup miss never
    /// costs a 3s SSDP sweep.
    fn try_rediscover(&self, name: &str) {
        if self.offline {
            return;
        }

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

        // 3. Build new Speaker handles (no lock needed).
        //
        // Satellites must be excluded here too: this map *replaces* the one
        // built at construction, so rebuilding without the exclusion would
        // resurrect every filtered surround and re-lose its room to the name
        // collision. Any topology already fetched is reused; before the first
        // fetch the set is empty, which is the same state construction starts in.
        let satellite_ids: HashSet<SpeakerId> =
            self.state_manager.get_satellite_ids().into_iter().collect();
        let new_speakers = match Self::build_speakers(
            &devices,
            &satellite_ids,
            &self.state_manager,
            &self.api_client,
        ) {
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

    /// A non-owning handle to the internal `StateManager`, for leak assertions.
    ///
    /// Exists so a test can outlive the system and check that dropping it
    /// actually freed the manager. `state_manager()` cannot do that job: it
    /// borrows from `&self`, so nothing observable survives the drop, and
    /// cloning the `Arc` first would itself keep the manager alive. A `Weak`
    /// is the only handle that answers "was this really released?".
    ///
    /// Only available when the `test-support` feature is enabled (or when
    /// compiling this crate's own test harness), matching
    /// [`Self::from_devices_offline`].
    #[cfg(any(feature = "test-support", test))]
    pub fn state_manager_weak(&self) -> std::sync::Weak<StateManager> {
        Arc::downgrade(&self.state_manager)
    }

    /// Get a blocking iterator over property change events
    ///
    /// Only emits events for properties that have been `watch()`ed.
    ///
    /// Each call returns an **independent** iterator, and every iterator
    /// receives every event. Two event loops — say a UI thread and a logger —
    /// therefore both see the whole stream instead of splitting it between them.
    ///
    /// An iterator only receives events emitted *after* it was created, so take
    /// it before the writes you want to observe. For current state rather than
    /// changes, use `speaker.volume.get()` and friends.
    ///
    /// Each iterator owns an unbounded queue: a slow consumer never loses an
    /// event and never blocks a fast one, but one that never drains will grow.
    /// Drop iterators you no longer read.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // First, watch some properties
    /// speaker.volume.watch()?;
    /// speaker.playback_state.watch()?;
    ///
    /// // Then iterate over changes (blocking). Each event carries the new
    /// // value, so draining a backlog shows every value the property passed
    /// // through rather than the latest one repeated.
    /// for event in system.iter() {
    ///     match &event.change {
    ///         PropertyChange::Volume(v) => println!("volume -> {}%", v.value()),
    ///         other => println!("{} changed on {}", other.key(), event.speaker_id),
    ///     }
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
    /// Tries all known speaker IPs sequentially until one responds with topology.
    /// Topology data is identical from any speaker, so first success wins.
    /// Also refreshes speaker IPs and records satellite IDs from the topology.
    ///
    /// No-op for offline systems (test constructors), which supply topology
    /// directly via `state_manager.initialize()` instead of polling speakers.
    fn ensure_topology(&self) {
        if self.offline || self.state_manager.group_count() > 0 {
            return;
        }

        let speaker_ips: Vec<String> = {
            let speakers = match self.speakers.read() {
                Ok(s) => s,
                Err(_) => return,
            };
            speakers.values().map(|s| s.ip.to_string()).collect()
        };

        for speaker_ip in &speaker_ips {
            let topology_state = match sonos_api::services::zone_group_topology::state::poll(
                &self.api_client,
                speaker_ip,
            ) {
                Ok(state) => state,
                Err(e) => {
                    tracing::debug!("Topology fetch failed for {}: {}", speaker_ip, e);
                    continue;
                }
            };

            let topology_changes = sonos_state::decode_topology_event(&topology_state);

            // Apply IP updates from topology before initializing groups
            for (speaker_id, new_ip) in &topology_changes.speaker_ips {
                self.state_manager.update_speaker_ip(speaker_id, *new_ip);
            }

            // Build topology with existing speaker data and freshly fetched groups
            let topology =
                Topology::new(self.state_manager.speaker_infos(), topology_changes.groups);
            self.state_manager.initialize(topology);

            // Store satellite IDs for later filtering
            self.state_manager
                .set_satellite_ids(topology_changes.satellite_ids);

            tracing::debug!(
                "Fetched zone group topology on-demand ({} groups)",
                self.state_manager.group_count()
            );
            return;
        }

        tracing::warn!("ensure_topology: no speakers responded");
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
    #[deprecated(
        since = "0.2.0",
        note = "use `speaker.group()` or `group_for_speaker()` instead"
    )]
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
                    .is_some_and(|si| si.name.eq_ignore_ascii_case(name))
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
    use sonos_state::GroupInfo;

    /// Create a test SonosSystem with the given devices.
    ///
    /// Uses the offline constructor: no topology SOAP poll, no SSDP
    /// rediscovery. Tests below supply topology explicitly via
    /// `state_manager.initialize()`, which is what the online path would have
    /// fetched anyway.
    fn create_test_system(devices: Vec<Device>) -> Result<SonosSystem, SdkError> {
        SonosSystem::from_devices_offline(devices)
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

    /// Compile-time assertion that `create_group`'s signature is correct.
    ///
    /// Never called: `create_group` forwards to `Group::add_speaker`, which
    /// would open a real TCP connection and wait out soap-client's 5s connect
    /// timeout, yet the assertion is purely about types. Type-checking a
    /// never-called function still fails the build if the signature changes, at
    /// zero runtime cost.
    #[allow(dead_code)]
    fn _assert_create_group_signature(
        system: &SonosSystem,
        coordinator: &Speaker,
        member: &Speaker,
    ) {
        fn assert_change_result(_r: Result<crate::group::GroupChangeResult, SdkError>) {}

        assert_change_result(system.create_group(coordinator, &[member]));
    }

    /// Guards the whole point of `from_devices_offline`: no network I/O.
    ///
    /// The device IP is in RFC 5737 TEST-NET-3, which is guaranteed
    /// unroutable. If construction ever polls it again, soap-client's 5s
    /// connect timeout blows the bound; a lookup miss re-enabling SSDP costs
    /// 3s more. A wall-clock bound is the only way to assert absence of I/O
    /// without a mock transport.
    #[test]
    fn test_from_devices_offline_makes_no_network_calls() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "203.0.113.1".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let start = std::time::Instant::now();
        let system = SonosSystem::from_devices_offline(devices).unwrap();
        assert!(system.speaker("Living Room").is_some());
        assert!(system.speaker("Nonexistent").is_none());
        assert!(system.groups().is_empty());
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(500),
            "offline construction and lookups should not touch the network, took {elapsed:?}"
        );
    }

    /// Dropping a `SonosSystem` must actually free its `StateManager`.
    ///
    /// The init closure is stored *on* the manager, so capturing a strong
    /// `Arc<StateManager>` in it made the manager own a closure that owned the
    /// manager. The cycle was invisible from the outside — construction and
    /// teardown both "worked" — but every `SonosSystem::new()` permanently
    /// leaked the manager, its store, the event-worker thread, the event
    /// manager's tokio runtime, and the callback socket. Only a `Weak` that
    /// outlives the system can observe the difference.
    #[test]
    fn test_dropping_system_releases_state_manager() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "203.0.113.1".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = SonosSystem::from_devices_offline(devices).unwrap();
        let weak = system.state_manager_weak();

        // Alive: reachable. The live count is deliberately not asserted — each
        // Speaker handle legitimately holds its own Arc, so the number tracks
        // the device count rather than anything about the cycle.
        assert!(weak.upgrade().is_some());

        drop(system);

        // Dropped: the system owned the speakers too, so nothing legitimate is
        // left holding the manager. A surviving strong reference can only be the
        // init closure the manager itself stores.
        assert_eq!(
            weak.strong_count(),
            0,
            "StateManager outlived its SonosSystem — the event-init closure is \
             holding a strong Arc to the manager that stores it"
        );
        assert!(weak.upgrade().is_none());
    }

    /// The same, but after `watch()` has run the lazy event-manager init, which
    /// is the path that actually exercises the closure's capture.
    ///
    /// Speakers hold `Arc`s to the manager, so the strong count is >1 here; the
    /// assertion is the one that matters — once every handle is gone, nothing
    /// keeps the manager alive.
    #[test]
    fn test_dropping_system_after_watch_releases_state_manager() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "203.0.113.1".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = SonosSystem::from_devices_offline(devices).unwrap();
        let weak = system.state_manager_weak();

        {
            let speaker = system.speaker("Living Room").unwrap();
            // Runs the init closure. No event manager can bind here (offline
            // test host may or may not permit it), so the mode is whatever the
            // environment allows — the point is that the closure executed.
            let _watch = speaker.volume.watch().unwrap();
        }

        drop(system);

        assert!(
            weak.upgrade().is_none(),
            "StateManager outlived its SonosSystem after watch() ran the lazy \
             event-init closure"
        );
    }

    #[test]
    fn test_display_name_prefers_room_name() {
        let device = Device {
            id: "RINCON_111".to_string(),
            name: "192.168.1.100 - Sonos One - RINCON_111".to_string(),
            room_name: "Kitchen".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        };
        assert_eq!(display_name(&device), "Kitchen");
    }

    #[test]
    fn test_display_name_falls_back_to_friendly_name() {
        let device = Device {
            id: "RINCON_111".to_string(),
            name: "192.168.1.100 - Sonos One - RINCON_111".to_string(),
            room_name: "Unknown".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        };
        assert_eq!(
            display_name(&device),
            "192.168.1.100 - Sonos One - RINCON_111"
        );

        let device_empty = Device {
            id: "RINCON_222".to_string(),
            name: "192.168.1.101 - Sonos One".to_string(),
            room_name: "".to_string(),
            ip_address: "192.168.1.101".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        };
        assert_eq!(display_name(&device_empty), "192.168.1.101 - Sonos One");
    }

    #[test]
    fn test_speaker_lookup_case_insensitive() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Kitchen".to_string(),
            room_name: "Kitchen".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        let system = create_test_system(devices).unwrap();
        assert!(system.speaker("Kitchen").is_some());
        assert!(system.speaker("kitchen").is_some());
        assert!(system.speaker("KITCHEN").is_some());
        assert!(system.speaker("Nonexistent").is_none());
    }

    #[test]
    fn test_speaker_uses_room_name() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "192.168.1.100 - Sonos One - RINCON_111".to_string(),
            room_name: "Kitchen".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = create_test_system(devices).unwrap();
        let spk = system.speaker("Kitchen");
        assert!(spk.is_some());
        assert_eq!(spk.unwrap().name, "Kitchen");

        // Verbose friendlyName should NOT match
        assert!(system
            .speaker("192.168.1.100 - Sonos One - RINCON_111")
            .is_none());
    }

    // ========================================================================
    // Bonded home theater: satellite exclusion vs. name-key collision
    // ========================================================================

    /// The real hardware that exposed the bug: a Playbar with two Sonos One
    /// surrounds bonded as one home theater, all three reporting
    /// `room_name = "Basement"`.
    ///
    /// IPs are RFC 5737 TEST-NET-3 so nothing here can reach a real device.
    fn basement_home_theater() -> Vec<Device> {
        vec![
            Device {
                id: "RINCON_PLAYBAR".to_string(),
                name: "Basement".to_string(),
                room_name: "Basement".to_string(),
                ip_address: "203.0.113.10".to_string(),
                port: 1400,
                model_name: "Sonos Playbar".to_string(),
            },
            Device {
                id: "RINCON_SURROUND_L".to_string(),
                name: "Basement".to_string(),
                room_name: "Basement".to_string(),
                ip_address: "203.0.113.11".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
            Device {
                id: "RINCON_SURROUND_R".to_string(),
                name: "Basement".to_string(),
                room_name: "Basement".to_string(),
                ip_address: "203.0.113.12".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
        ]
    }

    /// Every permutation of a device list, to stand in for arbitrary SSDP
    /// response order. Device counts here are 2-3, so the factorial is fine.
    fn all_orderings(devices: &[Device]) -> Vec<Vec<Device>> {
        if devices.len() <= 1 {
            return vec![devices.to_vec()];
        }
        let mut out = Vec::new();
        for i in 0..devices.len() {
            let mut rest = devices.to_vec();
            let head = rest.remove(i);
            for mut tail in all_orderings(&rest) {
                let mut one = vec![head.clone()];
                one.append(&mut tail);
                out.push(one);
            }
        }
        out
    }

    /// Build a system through the real construction sequence, with the topology
    /// that `ensure_topology` would have polled injected instead.
    ///
    /// Deliberately does *not* re-key the map itself: ordering is the bug, so
    /// the test must let production decide when filtering happens.
    fn system_with_satellites(devices: &[Device], satellites: &[&str]) -> SonosSystem {
        SonosSystem::from_devices_offline_with_topology(devices.to_vec(), |system| {
            system
                .state_manager
                .set_satellite_ids(satellites.iter().map(|id| SpeakerId::new(*id)).collect());
        })
        .unwrap()
    }

    /// A bonded home theater must appear as exactly one controllable speaker.
    ///
    /// Hardware symptom this pins down: `sonos speakers` logged two "duplicate
    /// speaker name" warnings and then listed no Basement at all, while `sonos
    /// groups` showed Basement with a live volume. The name-keyed map collapsed
    /// all three devices onto one key, and satellite filtering — which ran
    /// afterwards — deleted whichever one had won, taking the room with it.
    #[test]
    fn test_bonded_home_theater_keeps_visible_coordinator() {
        // SSDP response order is arbitrary — on the real system a surround
        // answered before the Playbar — and which device wins the name key
        // depends entirely on it. Assert over every permutation: a fix that only
        // holds when the coordinator happens to arrive first is not a fix.
        for order in all_orderings(&basement_home_theater()) {
            let ids: Vec<&str> = order.iter().map(|d| d.id.as_str()).collect();
            let system =
                system_with_satellites(&order, &["RINCON_SURROUND_L", "RINCON_SURROUND_R"]);

            // The room survives, exactly once.
            assert_eq!(
                system.speaker_names(),
                vec!["Basement".to_string()],
                "bonded home theater should be exactly one speaker named after its room \
                 (discovery order {ids:?})"
            );

            // And it is the Playbar — the visible coordinator that accepts
            // commands — not a surround. This is what `sonos -s Basement volume
            // 30` reaches.
            let basement = system
                .speaker("Basement")
                .unwrap_or_else(|| panic!("Basement must be reachable by name (order {ids:?})"));
            assert_eq!(
                basement.id,
                SpeakerId::new("RINCON_PLAYBAR"),
                "survivor must be the visible coordinator, not a satellite (order {ids:?})"
            );
            assert_eq!(basement.model_name, "Sonos Playbar");
            assert_eq!(basement.ip.to_string(), "203.0.113.10");

            // Satellites are not addressable as speakers in their own right.
            assert!(system
                .speaker_by_id(&SpeakerId::new("RINCON_SURROUND_L"))
                .is_none());
            assert!(system
                .speaker_by_id(&SpeakerId::new("RINCON_SURROUND_R"))
                .is_none());
        }
    }

    /// No regression for the ordinary case: a room with one visible speaker and
    /// no satellites anywhere is untouched.
    #[test]
    fn test_single_visible_speaker_room_unaffected() {
        let devices = vec![
            Device {
                id: "RINCON_KITCHEN".to_string(),
                name: "Kitchen".to_string(),
                room_name: "Kitchen".to_string(),
                ip_address: "203.0.113.20".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
            Device {
                id: "RINCON_OFFICE".to_string(),
                name: "Office".to_string(),
                room_name: "Office".to_string(),
                ip_address: "203.0.113.21".to_string(),
                port: 1400,
                model_name: "Sonos Roam".to_string(),
            },
        ];

        // No satellites: the rebuild is a no-op and both rooms stand.
        let system = system_with_satellites(&devices, &[]);

        assert_eq!(system.speakers().len(), 2);
        assert_eq!(
            system.speaker("Kitchen").unwrap().id,
            SpeakerId::new("RINCON_KITCHEN")
        );
        assert_eq!(
            system.speaker("Office").unwrap().id,
            SpeakerId::new("RINCON_OFFICE")
        );
    }

    /// A visible speaker sharing a room with satellites keeps its plain name.
    ///
    /// Guards against over-correcting: satellite exclusion must not push the
    /// coordinator onto a disambiguated key, which would break `sonos -s
    /// Basement`.
    #[test]
    fn test_coordinator_keeps_plain_room_name_not_disambiguated() {
        let devices = basement_home_theater();
        let system = system_with_satellites(&devices, &["RINCON_SURROUND_L", "RINCON_SURROUND_R"]);

        assert!(
            system.speaker("Basement").is_some(),
            "coordinator must hold the plain room name"
        );
        for name in system.speaker_names() {
            assert!(
                !name.contains('('),
                "coordinator should not be disambiguated when the collision was satellites: {name}"
            );
        }
    }

    /// Two *genuinely visible* speakers sharing a room name: both are kept, the
    /// first under the plain name and the second suffixed with its ID.
    ///
    /// The old behavior silently dropped one — real data loss with only a log
    /// line to show for it. Sonos prevents duplicate room names in the app, so
    /// this state means something unusual (a rename mid-discovery, a stale cache
    /// entry for a replaced unit); dropping a controllable speaker is the worse
    /// answer in every such case.
    #[test]
    fn test_two_visible_speakers_same_room_are_both_kept() {
        let devices = vec![
            Device {
                id: "RINCON_DUPE_1".to_string(),
                name: "Basement".to_string(),
                room_name: "Basement".to_string(),
                ip_address: "203.0.113.30".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
            Device {
                id: "RINCON_DUPE_2".to_string(),
                name: "Basement".to_string(),
                room_name: "Basement".to_string(),
                ip_address: "203.0.113.31".to_string(),
                port: 1400,
                model_name: "Sonos Five".to_string(),
            },
        ];

        // Neither is a satellite.
        let system = system_with_satellites(&devices, &[]);

        // Nothing is lost.
        assert_eq!(
            system.speakers().len(),
            2,
            "both visible speakers must be retained, not silently overwritten"
        );

        // The first-seen keeps the plain name, so existing scripts keep working.
        assert_eq!(
            system.speaker("Basement").unwrap().id,
            SpeakerId::new("RINCON_DUPE_1")
        );

        // The second is reachable under an ID-suffixed name.
        let second = system
            .speaker("Basement (RINCON_DUPE_2)")
            .expect("second visible speaker must be reachable by disambiguated name");
        assert_eq!(second.id, SpeakerId::new("RINCON_DUPE_2"));
        assert_eq!(second.model_name, "Sonos Five");

        // Both remain addressable by ID.
        assert!(system
            .speaker_by_id(&SpeakerId::new("RINCON_DUPE_1"))
            .is_some());
        assert!(system
            .speaker_by_id(&SpeakerId::new("RINCON_DUPE_2"))
            .is_some());
    }

    #[test]
    fn test_group_lookup_case_insensitive() {
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = create_test_system(devices).unwrap();

        let speaker = SpeakerId::new("RINCON_111");
        let group = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            speaker.clone(),
            vec![speaker.clone()],
        );

        let topology = Topology::new(system.state_manager.speaker_infos(), vec![group]);
        system.state_manager.initialize(topology);

        assert!(system.group("Living Room").is_some());
        assert!(system.group("living room").is_some());
        assert!(system.group("LIVING ROOM").is_some());
        assert!(system.group("Nonexistent").is_none());
    }
}
