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
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use sonos_api::{Service, ServiceScope};
use sonos_discovery::Device;
use sonos_event_manager::{SonosEventManager, WatchRegistry};
use tracing::info;

use crate::decoder::PropertyChange;
use crate::event_worker::spawn_state_event_worker;
use crate::iter::{ChangeIterator, EventFanout};
use crate::model::{GroupId, SpeakerId, SpeakerInfo};
use crate::property::{GroupInfo, Property, Scope, SonosProperty, Topology};
use crate::{Result, StateError};

/// Closure type for lazy event manager initialization.
///
/// Stored on `StateManager` as the single source of truth. Called by
/// `PropertyHandle::watch()` to trigger event manager creation on first use.
/// Uses `Box<dyn Error>` to avoid circular dependency on `sonos-sdk` error types.
pub type EventInitFn = Arc<
    dyn Fn() -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync,
>;

// ============================================================================
// Write provenance - ChangeSource / WriteStamp / WriteOutcome
// ============================================================================

/// Where a property value came from.
///
/// Recorded on every write and carried on every [`ChangeEvent`], for two
/// reasons: it breaks ties between writes that share an `Instant` (the variant
/// order below is the tie-break order, most authoritative first), and it lets a
/// consumer tell a device-pushed value apart from one this process just wrote
/// optimistically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    /// Decoded from a UPnP NOTIFY, or from a poll standing in for one.
    ///
    /// The most authoritative source: the device volunteered this value, so it
    /// was true at the moment it was sent.
    Event,
    /// Written locally immediately after a control action succeeded.
    ///
    /// The device acknowledged the action, so the value is true as of the
    /// acknowledgement — but it is our inference, not the device's report.
    LocalAction,
    /// Returned by an explicit `fetch()` SOAP read.
    ///
    /// The least authoritative, because a `fetch()` observes the device at
    /// *request* time but lands at *response* time — see [`WriteStamp`].
    Fetch,
}

impl ChangeSource {
    /// Tie-break rank for writes bearing the same `observed_at`.
    ///
    /// Only consulted on an exact `Instant` collision, which in practice means
    /// two writes derived from one observation. Higher wins.
    fn rank(self) -> u8 {
        match self {
            ChangeSource::Event => 2,
            ChangeSource::LocalAction => 1,
            ChangeSource::Fetch => 0,
        }
    }
}

/// When a value was *observed*, and by what.
///
/// The critical word is **observed**, not *written*. A `fetch()` reads the
/// device at request time but only calls `set_property` when the SOAP response
/// comes back, which can be hundreds of milliseconds later. If the stamp were
/// taken at write time, a slow `fetch()` would always look newer than an event
/// that arrived while it was in flight, and would overwrite a fresher value
/// with a stale one — the speaker would visibly snap back to its old volume.
///
/// So the caller stamps the moment the observation was made
/// ([`WriteStamp::observed_at`]) and the store rejects any write that is older
/// than the one already recorded. Event and local-action writes have no such
/// gap and use [`WriteStamp::now`].
#[derive(Debug, Clone, Copy)]
pub struct WriteStamp {
    /// When the underlying observation was made — *not* when it was written.
    pub observed_at: Instant,
    /// What produced the observation.
    pub source: ChangeSource,
}

impl WriteStamp {
    /// Stamp an observation made right now.
    ///
    /// Correct for [`ChangeSource::Event`] and [`ChangeSource::LocalAction`],
    /// where observation and write happen in the same breath.
    pub fn now(source: ChangeSource) -> Self {
        Self {
            observed_at: Instant::now(),
            source,
        }
    }

    /// Stamp an observation made at a known earlier instant.
    ///
    /// Use this for `fetch()`: pass the `Instant` captured *before* the SOAP
    /// request, so a response that lands after a newer event loses to it.
    pub fn observed_at(source: ChangeSource, observed_at: Instant) -> Self {
        Self {
            observed_at,
            source,
        }
    }

    /// Whether this observation is at least as recent as `prev`.
    ///
    /// Strictly newer wins outright. On an exact `Instant` collision the more
    /// authoritative [`ChangeSource`] wins, so an event cannot be displaced by
    /// a `fetch()` that happens to share its timestamp.
    fn supersedes(&self, prev: &WriteStamp) -> bool {
        self.observed_at > prev.observed_at
            || (self.observed_at == prev.observed_at && self.source.rank() >= prev.source.rank())
    }
}

/// Result of attempting to write a property.
///
/// Three outcomes rather than the previous `bool`, because "the value is
/// different" and "this write was allowed to happen at all" are separate
/// questions once writes are ordered. Only `Changed` emits a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Accepted, and the stored value is different than before.
    Changed,
    /// Accepted, but the value was already what was written.
    Unchanged,
    /// Rejected: a strictly newer observation is already stored.
    Stale,
}

impl WriteOutcome {
    /// Whether this write changed the stored value (and so should notify).
    pub fn changed(self) -> bool {
        matches!(self, WriteOutcome::Changed)
    }
}

// ============================================================================
// ChangeEvent - for iter()
// ============================================================================

/// A change event emitted when a watched property changes.
///
/// Carries the new value as a typed [`PropertyChange`], so a consumer draining
/// a backlog observes *every* value the property passed through rather than
/// whatever the store happens to hold by the time it looks. That distinction
/// matters: a `Playing -> Transitioning -> Playing` sequence is three queued
/// events but only one final store value, and re-reading the store would make
/// the middle state — and the fact that anything moved at all — invisible.
///
/// `property_key()` and `service()` are derived from the payload rather than
/// stored beside it, so the two cannot drift apart.
#[derive(Debug, Clone)]
pub struct ChangeEvent {
    /// Speaker or entity that changed
    pub speaker_id: SpeakerId,
    /// The new value, typed
    pub change: PropertyChange,
    /// What produced this value
    pub source: ChangeSource,
    /// When the change was observed
    pub timestamp: Instant,
}

impl ChangeEvent {
    pub fn new(speaker_id: SpeakerId, change: PropertyChange, stamp: WriteStamp) -> Self {
        Self {
            speaker_id,
            change,
            source: stamp.source,
            timestamp: stamp.observed_at,
        }
    }

    /// The key of the property that changed.
    pub fn property_key(&self) -> &'static str {
        self.change.key()
    }

    /// The UPnP service the changed property belongs to.
    pub fn service(&self) -> Service {
        self.change.service()
    }
}

// ============================================================================
// Watch bookkeeping
// ============================================================================

/// The holds on one watched `(speaker_id, property_key)` pair.
///
/// A watch is a *hold*, not a flag: several independent watchers can claim the
/// same pair, and it stays watched until the last of them lets go. The two
/// fields are separate — rather than one counter — because the two kinds of hold
/// are released by completely different events, on different schedules:
///
/// - **`direct`** holds come from [`StateManager::register_watch`]: the SDK's
///   polling-fallback and cache-only paths, group-member notification
///   forwarding, `watch_property_with_subscription`, and tests. Each is released
///   individually by [`StateManager::unregister_watch`], normally from a
///   `CacheOnlyGuard::drop`. These are what need counting: *n* watchers of one
///   property must survive *n-1* drops.
/// - **`subscription`** is a single flag covering every `WatchGuard` acquired
///   through [`WatchRegistry::register_watch`]. It cannot be a counter, because
///   nothing decrements it one at a time: `WatchGuard::drop` only decrements
///   `sonos-event-manager`'s per-`(ip, service)` subscription ref count, and
///   `unregister_watches_for_service` fires once, later, when *that* count hits
///   zero — at which point every contributing guard is provably gone. A counter
///   incremented per guard but cleared only in bulk would either leak (a watch
///   nobody holds emitting forever) or, if decremented by one, drop while other
///   guards are still alive.
///
/// The pair stops being watched, and the entry leaves the map, only when the
/// flag is clear *and* the count is zero. Keeping them apart is the actual fix:
/// a subscription teardown must not take the individually-held `direct` watches
/// of its sibling properties with it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WatchHolds {
    /// Whether any `WatchGuard` is registered for this pair.
    subscription: bool,
    /// Number of outstanding `register_watch` holds.
    direct: usize,
}

impl WatchHolds {
    fn is_held(&self) -> bool {
        self.subscription || self.direct > 0
    }
}

/// Watch holds per `(speaker_id, property_key)` pair.
pub(crate) type WatchCounts = HashMap<(SpeakerId, &'static str), WatchHolds>;

/// Mark `(speaker_id, key)` as held by a `WatchGuard`.
fn retain_subscription_watch(
    watched: &RwLock<WatchCounts>,
    speaker_id: &SpeakerId,
    key: &'static str,
) {
    watched
        .write()
        .entry((speaker_id.clone(), key))
        .or_default()
        .subscription = true;
}

/// Add one `direct` hold on `(speaker_id, key)`.
pub(crate) fn retain_direct_watch(
    watched: &RwLock<WatchCounts>,
    speaker_id: &SpeakerId,
    key: &'static str,
) {
    watched
        .write()
        .entry((speaker_id.clone(), key))
        .or_default()
        .direct += 1;
}

/// Release one `direct` hold, dropping the entry once no holds remain.
///
/// Releasing a pair that is not held is a no-op: an over-release must not wrap
/// around and resurrect the watch.
fn release_direct_watch(watched: &RwLock<WatchCounts>, speaker_id: &SpeakerId, key: &'static str) {
    let mut guard = watched.write();
    let entry_key = (speaker_id.clone(), key);
    if let Some(holds) = guard.get_mut(&entry_key) {
        holds.direct = holds.direct.saturating_sub(1);
        if !holds.is_held() {
            guard.remove(&entry_key);
        }
    }
}

/// Clear the subscription hold on `(speaker_id, key)`, keeping `direct` holds.
///
/// Called when a UPnP subscription is finally torn down. `direct` holders are
/// deliberately untouched: they are tracked per watcher and released by their
/// own guards, and their property may not even be the one that was subscribed.
fn release_subscription_watch(
    watched: &RwLock<WatchCounts>,
    speaker_id: &SpeakerId,
    key: &'static str,
) {
    let mut guard = watched.write();
    let entry_key = (speaker_id.clone(), key);
    if let Some(holds) = guard.get_mut(&entry_key) {
        holds.subscription = false;
        if !holds.is_held() {
            guard.remove(&entry_key);
        }
    }
}

/// Whether `(speaker_id, key)` currently has any hold on it.
pub(crate) fn is_pair_watched(
    watched: &WatchCounts,
    speaker_id: &SpeakerId,
    key: &'static str,
) -> bool {
    watched.contains_key(&(speaker_id.clone(), key))
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
    /// Speaker to group mapping for quick lookups
    pub(crate) speaker_to_group: HashMap<SpeakerId, GroupId>,
    /// Satellite speaker IDs (Invisible="1") from topology
    pub(crate) satellite_ids: HashSet<SpeakerId>,
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
            speaker_to_group: HashMap::new(),
            satellite_ids: HashSet::new(),
        }
    }

    pub(crate) fn add_speaker(&mut self, speaker: SpeakerInfo) {
        let id = speaker.id.clone();
        let ip = speaker.ip_address;
        self.ip_to_speaker.insert(ip, id.clone());
        self.speakers.insert(id.clone(), speaker);
        self.speaker_props
            .entry(id)
            .or_insert_with(PropertyBag::new);
    }

    fn speaker(&self, id: &SpeakerId) -> Option<&SpeakerInfo> {
        self.speakers.get(id)
    }

    fn speakers(&self) -> Vec<SpeakerInfo> {
        self.speakers.values().cloned().collect()
    }

    pub(crate) fn add_group(&mut self, group: GroupInfo) {
        let id = group.id.clone();
        // Update speaker_to_group mapping for all members
        for member_id in &group.member_ids {
            self.speaker_to_group.insert(member_id.clone(), id.clone());
        }
        self.groups.insert(id.clone(), group);
        self.group_props.entry(id).or_insert_with(PropertyBag::new);
    }

    /// Get the group a speaker belongs to
    #[allow(dead_code)]
    pub(crate) fn get_group_for_speaker(&self, speaker_id: &SpeakerId) -> Option<&GroupInfo> {
        let group_id = self.speaker_to_group.get(speaker_id)?;
        self.groups.get(group_id)
    }

    /// Clear all groups and speaker_to_group mappings
    ///
    /// Used when processing topology updates to replace all group data
    pub(crate) fn clear_groups(&mut self) {
        self.groups.clear();
        self.group_props.clear();
        self.speaker_to_group.clear();
    }

    /// Resolve the coordinator speaker for the given speaker.
    ///
    /// Looks up `speaker_to_group → groups → coordinator_id`.
    /// Returns the speaker's own ID if no group info exists (safe default).
    pub(crate) fn resolve_coordinator(&self, speaker_id: &SpeakerId) -> SpeakerId {
        self.speaker_to_group
            .get(speaker_id)
            .and_then(|gid| self.groups.get(gid))
            .map(|group| group.coordinator_id.clone())
            .unwrap_or_else(|| speaker_id.clone())
    }

    /// Get a property value with coordinator resolution for PerCoordinator services.
    ///
    /// If the property's service is PerCoordinator AND the property scope is Speaker,
    /// reads from the coordinator's speaker_props. Otherwise reads from the
    /// speaker's own props.
    ///
    /// Group-scoped properties (e.g. GroupVolume) come from a PerCoordinator service
    /// but are stored in `group_props`, not `speaker_props`, so they are not resolved
    /// through the coordinator's speaker_props.
    pub(crate) fn get_resolved<P: SonosProperty>(&self, speaker_id: &SpeakerId) -> Option<P> {
        if P::SERVICE.scope() == ServiceScope::PerCoordinator && P::SCOPE == Scope::Speaker {
            let coordinator_id = self.resolve_coordinator(speaker_id);
            self.speaker_props.get(&coordinator_id)?.get::<P>()
        } else {
            self.speaker_props.get(speaker_id)?.get::<P>()
        }
    }

    /// Resolve which speaker's bag a `set` of `P` for `speaker_id` should target.
    ///
    /// The exact mirror of [`Self::get_resolved`]'s branch, so a write always
    /// lands where the matching read looks. Factored out rather than inlined at
    /// the two sites because the two must not be able to drift apart.
    pub(crate) fn resolve_write_target<P: SonosProperty>(
        &self,
        speaker_id: &SpeakerId,
    ) -> SpeakerId {
        if P::SERVICE.scope() == ServiceScope::PerCoordinator && P::SCOPE == Scope::Speaker {
            self.resolve_coordinator(speaker_id)
        } else {
            speaker_id.clone()
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn get<P: Property>(&self, speaker_id: &SpeakerId) -> Option<P> {
        self.speaker_props.get(speaker_id)?.get::<P>()
    }

    pub(crate) fn set<P: Property>(
        &mut self,
        speaker_id: &SpeakerId,
        value: P,
        stamp: WriteStamp,
    ) -> WriteOutcome {
        let bag = self
            .speaker_props
            .entry(speaker_id.clone())
            .or_insert_with(PropertyBag::new);
        bag.set(value, stamp)
    }

    pub(crate) fn get_group<P: Property>(&self, group_id: &GroupId) -> Option<P> {
        self.group_props.get(group_id)?.get::<P>()
    }

    pub(crate) fn set_group<P: Property>(
        &mut self,
        group_id: &GroupId,
        value: P,
        stamp: WriteStamp,
    ) -> WriteOutcome {
        let bag = self
            .group_props
            .entry(group_id.clone())
            .or_insert_with(PropertyBag::new);
        bag.set(value, stamp)
    }

    fn set_system<P: Property>(&mut self, value: P, stamp: WriteStamp) -> WriteOutcome {
        self.system_props.set(value, stamp)
    }

    /// Update a speaker's IP address in the store. Returns the old IP if changed.
    pub(crate) fn update_speaker_ip_address(
        &mut self,
        speaker_id: &SpeakerId,
        new_ip: IpAddr,
    ) -> Option<IpAddr> {
        if let Some(info) = self.speakers.get_mut(speaker_id) {
            let old_ip = info.ip_address;
            if old_ip != new_ip {
                info.ip_address = new_ip;
                return Some(old_ip);
            }
        }
        None
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
    /// Provenance of the value currently stored under each `TypeId`.
    ///
    /// Kept in a sibling map rather than boxed alongside the value so the
    /// existing type-erased `values` layout, and its `downcast_ref` reads, stay
    /// exactly as they were.
    stamps: HashMap<TypeId, WriteStamp>,
}

impl PropertyBag {
    pub(crate) fn new() -> Self {
        Self {
            values: HashMap::new(),
            stamps: HashMap::new(),
        }
    }

    fn get<P: Property>(&self) -> Option<P> {
        let type_id = TypeId::of::<P>();
        self.values
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<P>())
            .cloned()
    }

    /// Write `value` if `stamp` is not older than what is already stored.
    ///
    /// The staleness check comes *first*: a stale write is rejected outright,
    /// before the equality comparison, so it can neither change the value nor
    /// advance the stamp. Without that ordering a late `fetch()` response would
    /// overwrite a newer event-derived value.
    fn set<P: Property>(&mut self, value: P, stamp: WriteStamp) -> WriteOutcome {
        let type_id = TypeId::of::<P>();

        if let Some(prev) = self.stamps.get(&type_id) {
            if !stamp.supersedes(prev) {
                tracing::debug!(
                    "Rejecting stale {:?} write for {}: observed {:?} before the stored {:?} write",
                    stamp.source,
                    P::KEY,
                    stamp.observed_at,
                    prev.source,
                );
                return WriteOutcome::Stale;
            }
        }

        let current = self
            .values
            .get(&type_id)
            .and_then(|boxed| boxed.downcast_ref::<P>());
        let changed = current != Some(&value);

        // Record the stamp even when the value is unchanged: the observation
        // *did* happen and is now the most recent one, so a later write must be
        // ordered against it rather than against some older observation.
        self.stamps.insert(type_id, stamp);

        if changed {
            self.values.insert(type_id, Box::new(value));
            WriteOutcome::Changed
        } else {
            WriteOutcome::Unchanged
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

    /// Watched properties for iter() filtering, reference-counted.
    ///
    /// Counted rather than a plain set because several independent watchers can
    /// hold the same `(speaker_id, property_key)` at once — two widgets watching
    /// one property, an SDK `WatchHandle` alongside a direct `register_watch`, or
    /// a handle reacquired before the previous one has dropped.
    /// With a `HashSet` the *first* release removed the entry and silenced every
    /// remaining watcher; the count means an entry disappears only when the last
    /// watcher lets go.
    watched: Arc<RwLock<WatchCounts>>,

    /// IP to speaker ID mapping (for event worker)
    ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>,

    /// Event manager (set-once via OnceLock — enables live events)
    event_manager: OnceLock<Arc<SonosEventManager>>,

    /// Fan-out for change events: every `iter()` is an independent subscriber
    /// receiving every event.
    ///
    /// Replaces the previous single `mpsc::Sender`/shared-`Receiver` pair, under
    /// which two `iter()` loops silently split the stream between them. See
    /// [`EventFanout`].
    fanout: Arc<EventFanout>,

    /// Background event processor handle (lazily spawned)
    _worker: Mutex<Option<JoinHandle<()>>>,

    /// Cleanup timeout for subscriptions
    cleanup_timeout: Duration,

    /// Maps property key → Service for WatchRegistry's unregister_watches_for_service.
    /// Shared with StateWatchRegistry via Arc.
    key_to_service: Arc<RwLock<HashMap<&'static str, Service>>>,

    /// Lazy event manager initialization closure (set-once).
    /// Called by watch() to trigger event manager creation on first use.
    event_init: OnceLock<EventInitFn>,
}

// ============================================================================
// StateWatchRegistry - WatchRegistry impl for SonosEventManager
// ============================================================================

/// Lightweight WatchRegistry implementation wired into the event manager.
///
/// Separated from StateManager because `mpsc::Sender` is `!Sync`,
/// preventing StateManager itself from satisfying `WatchRegistry: Sync`.
/// This struct holds only the Arc-wrapped fields needed for watch management.
struct StateWatchRegistry {
    watched: Arc<RwLock<WatchCounts>>,
    ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>,
    key_to_service: Arc<RwLock<HashMap<&'static str, Service>>>,
}

impl WatchRegistry for StateWatchRegistry {
    fn register_watch(&self, speaker_id: &SpeakerId, key: &'static str, service: Service) {
        retain_subscription_watch(&self.watched, speaker_id, key);
        self.key_to_service.write().insert(key, service);
    }

    fn unregister_watches_for_service(&self, ip: IpAddr, service: Service) {
        // 1. Resolve IP → SpeakerId. Bound to a local first so the read guard
        //    is released before this function takes any other lock.
        let resolved = self.ip_to_speaker.read().get(&ip).cloned();
        let speaker_id = match resolved {
            Some(id) => id,
            None => {
                tracing::warn!(
                    "unregister_watches_for_service: no speaker found for IP {}",
                    ip
                );
                return;
            }
        };

        // 2. Find property keys belonging to this service
        let service_keys: Vec<&'static str> = self
            .key_to_service
            .read()
            .iter()
            .filter(|(_, &svc)| svc == service)
            .map(|(&key, _)| key)
            .collect();

        // 3. Drop the subscription hold on each of this service's keys.
        //
        // Only the subscription hold: a `direct` hold is owned by an individual
        // watcher (polling fallback, cache-only, member forwarding) which
        // releases it through its own guard. Removing entries wholesale here is
        // what previously made dropping one `WatchHandle` silence its siblings.
        for key in service_keys {
            release_subscription_watch(&self.watched, &speaker_id, key);
        }
    }
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
        let mut store = self.store.write();
        let mut ip_map = self.ip_to_speaker.write();

        for device in devices {
            let speaker_id = SpeakerId::new(&device.id);
            let ip: IpAddr = device
                .ip_address
                .parse()
                .map_err(|_| StateError::InvalidIpAddress(device.ip_address.clone()))?;

            let friendly_name = if device.room_name.is_empty() || device.room_name == "Unknown" {
                device.name.clone()
            } else {
                device.room_name.clone()
            };

            let info = SpeakerInfo {
                id: speaker_id.clone(),
                name: friendly_name,
                room_name: device.room_name.clone(),
                ip_address: ip,
                port: device.port,
                model_name: device.model_name.clone(),
                software_version: "unknown".to_string(),
                boot_seq: 0,
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

        if let Some(em) = self.event_manager.get() {
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
        self.store.read().speakers()
    }

    /// Get a specific speaker info by ID
    pub fn speaker_info(&self, speaker_id: &SpeakerId) -> Option<SpeakerInfo> {
        self.store.read().speaker(speaker_id).cloned()
    }

    /// Get speaker IP by ID
    pub fn get_speaker_ip(&self, speaker_id: &SpeakerId) -> Option<IpAddr> {
        self.store.read().speaker(speaker_id).map(|s| s.ip_address)
    }

    /// Get boot_seq for a speaker (used by GroupManagement AddMember)
    pub fn get_boot_seq(&self, speaker_id: &SpeakerId) -> Option<u32> {
        self.store.read().speaker(speaker_id).map(|s| s.boot_seq)
    }

    /// Update a speaker's IP address in both the store and the reverse map.
    pub fn update_speaker_ip(&self, speaker_id: &SpeakerId, new_ip: IpAddr) {
        let old_ip = {
            let mut store = self.store.write();
            store.update_speaker_ip_address(speaker_id, new_ip)
        };
        if let Some(old_ip) = old_ip {
            let mut map = self.ip_to_speaker.write();
            map.remove(&old_ip);
            map.insert(new_ip, speaker_id.clone());
        }
    }

    /// Get all satellite speaker IDs from topology data.
    pub fn get_satellite_ids(&self) -> Vec<SpeakerId> {
        self.store.read().satellite_ids.iter().cloned().collect()
    }

    /// Store satellite speaker IDs from topology data.
    pub fn set_satellite_ids(&self, ids: Vec<SpeakerId>) {
        self.store.write().satellite_ids = ids.into_iter().collect();
    }

    /// Create a blocking iterator over change events
    ///
    /// Only emits events for properties that have been watched.
    ///
    /// Each call returns an **independent** iterator: every iterator receives
    /// every event, so two event loops both see the whole stream instead of
    /// splitting it between them. An iterator only receives events emitted after
    /// it was created, so take it before the writes you want to observe.
    ///
    /// Each iterator owns an unbounded queue, so a slow consumer never loses an
    /// event and never blocks a fast one — and never drains means never bounded.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // First, watch some properties
    /// speaker.volume.watch()?;
    ///
    /// // Then iterate over changes — the new value rides along on the event
    /// for event in manager.iter() {
    ///     match &event.change {
    ///         PropertyChange::Volume(v) => println!("volume -> {}%", v.value()),
    ///         other => println!("{} changed", other.key()),
    ///     }
    /// }
    /// ```
    pub fn iter(&self) -> ChangeIterator {
        ChangeIterator::new(&self.fanout)
    }

    /// Get current property value (sync, no subscription)
    ///
    /// For PerCoordinator speaker-scoped properties, this transparently reads
    /// from the coordinator's store, so group members see the coordinator's value.
    pub fn get_property<P: SonosProperty>(&self, speaker_id: &SpeakerId) -> Option<P> {
        self.store.read().get_resolved::<P>(speaker_id)
    }

    /// Get current group property value (sync, no subscription)
    pub fn get_group_property<P: Property>(&self, group_id: &GroupId) -> Option<P> {
        self.store.read().get_group::<P>(group_id)
    }

    /// Set a property value
    ///
    /// Updates the property value in the store and emits a change event
    /// if the property is being watched.
    ///
    /// The write is routed the same way [`Self::get_property`] reads: for a
    /// `PerCoordinator` speaker-scoped property, the value lands in the
    /// *coordinator's* bag, because `get_resolved` reads it from there. Writing
    /// the raw `speaker_id` instead put the value in a bag nothing ever reads —
    /// so `speaker.play()` on a grouped member updated a cache entry that
    /// `playback_state.get()` could not see, and the UI kept showing the old
    /// state until an event arrived.
    ///
    /// The notification is still keyed on the *requesting* speaker, so a member
    /// watching the property is woken by its own write. The coordinator's own
    /// watchers are reached by the worker's group fan-out on the next event.
    ///
    /// Stamped [`ChangeSource::LocalAction`] as of now. Use
    /// [`Self::set_property_stamped`] for a `fetch()` result, whose observation
    /// predates the write by a full network round trip.
    pub fn set_property<P: SonosProperty>(&self, speaker_id: &SpeakerId, value: P) {
        self.set_property_stamped(
            speaker_id,
            value,
            WriteStamp::now(ChangeSource::LocalAction),
        );
    }

    /// Set a property value with explicit write provenance.
    ///
    /// Rejected without effect if `stamp` is older than the observation already
    /// stored — see [`WriteStamp`]. Returns the outcome so a caller can tell a
    /// rejected write from an accepted one.
    pub fn set_property_stamped<P: SonosProperty>(
        &self,
        speaker_id: &SpeakerId,
        value: P,
        stamp: WriteStamp,
    ) -> WriteOutcome {
        // Resolve and write under one lock: taking the coordinator from a
        // separate read would leave a window in which a topology event regroups
        // the speaker and the write lands in the wrong bag.
        let (target_id, outcome) = {
            let mut store = self.store.write();
            let target_id = store.resolve_write_target::<P>(speaker_id);
            let outcome = store.set::<P>(&target_id, value.clone(), stamp);
            (target_id, outcome)
        };

        if outcome.changed() {
            // Key the notification on the speaker the caller asked about, so a
            // member watching the property is woken by its own write...
            self.maybe_emit_change(speaker_id, &value, stamp);
            // ...and on the coordinator too when they differ, since it is the
            // coordinator's bag that actually changed.
            if target_id != *speaker_id {
                self.maybe_emit_change(&target_id, &value, stamp);
            }
        }

        outcome
    }

    /// Set a group property value
    ///
    /// Updates the group property value in the store and emits a change event
    /// if the property is being watched (keyed on the coordinator's speaker ID).
    /// Used by the SDK layer to store group-scoped values fetched via API calls.
    ///
    /// Stamped [`ChangeSource::LocalAction`]; see
    /// [`Self::set_group_property_stamped`] for `fetch()` results.
    pub fn set_group_property<P: SonosProperty>(&self, group_id: &GroupId, value: P) {
        self.set_group_property_stamped(
            group_id,
            value,
            WriteStamp::now(ChangeSource::LocalAction),
        );
    }

    /// Set a group property value with explicit write provenance.
    ///
    /// Rejected without effect if `stamp` is older than the observation already
    /// stored — see [`WriteStamp`].
    pub fn set_group_property_stamped<P: SonosProperty>(
        &self,
        group_id: &GroupId,
        value: P,
        stamp: WriteStamp,
    ) -> WriteOutcome {
        let (outcome, coordinator_id) = {
            let mut store = self.store.write();
            let outcome = store.set_group::<P>(group_id, value.clone(), stamp);
            if !outcome.changed() {
                return outcome;
            }
            let coordinator_id = store.groups.get(group_id).map(|g| g.coordinator_id.clone());
            (outcome, coordinator_id)
        };

        if let Some(coordinator_id) = coordinator_id {
            self.maybe_emit_change(&coordinator_id, &value, stamp);
        }

        outcome
    }

    /// Register a property as watched (called by PropertyHandle::watch)
    ///
    /// Adds one reference. Balanced by [`Self::unregister_watch`]; the property
    /// keeps emitting until every registration has been unregistered.
    pub fn register_watch(&self, speaker_id: &SpeakerId, property_key: &'static str) {
        retain_direct_watch(&self.watched, speaker_id, property_key);
    }

    /// Unregister a property watch
    ///
    /// Releases one reference taken by [`Self::register_watch`]. The property
    /// stops being watched only when the last reference is released, so one
    /// watcher going away cannot silence its siblings. Unregistering something
    /// that was never registered is a no-op.
    pub fn unregister_watch(&self, speaker_id: &SpeakerId, property_key: &'static str) {
        release_direct_watch(&self.watched, speaker_id, property_key);
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
        if let Some(em) = self.event_manager.get() {
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
        if let Some(em) = self.event_manager.get() {
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
        is_pair_watched(&self.watched.read(), speaker_id, property_key)
    }

    /// Emit a change event if the property is being watched
    fn maybe_emit_change<P: SonosProperty>(
        &self,
        speaker_id: &SpeakerId,
        value: &P,
        stamp: WriteStamp,
    ) {
        let is_watched = is_pair_watched(&self.watched.read(), speaker_id, P::KEY);
        if !is_watched {
            return;
        }

        // A property with no `PropertyChange` variant cannot be carried in an
        // event. That is only `Topology` today, which is not watchable through
        // the SDK, so this is unreachable in practice — but it is a silent
        // dropped notification if a new watchable property forgets to implement
        // `to_change`, so it warns rather than returning quietly.
        let Some(change) = value.to_change() else {
            tracing::warn!(
                "Not emitting a change event for {}: no PropertyChange variant \
                 (SonosProperty::to_change returned None)",
                P::KEY
            );
            return;
        };

        self.fanout
            .send(ChangeEvent::new(speaker_id.clone(), change, stamp));
    }

    /// Initialize from topology data
    pub fn initialize(&self, topology: Topology) {
        let mut store = self.store.write();
        for speaker in &topology.speakers {
            store.add_speaker(speaker.clone());
        }
        for group in &topology.groups {
            store.add_group(group.clone());
        }
        // `LocalAction`: topology supplied by the caller (a poll result or a
        // test fixture), not decoded from a NOTIFY. Nothing orders against the
        // system bag today, but it is stamped for consistency.
        store.set_system(topology, WriteStamp::now(ChangeSource::LocalAction));
    }

    /// Check if initialized with any speakers
    pub fn is_initialized(&self) -> bool {
        !self.store.read().is_empty()
    }

    /// Get number of speakers
    pub fn speaker_count(&self) -> usize {
        self.store.read().speaker_count()
    }

    /// Get number of groups
    pub fn group_count(&self) -> usize {
        self.store.read().group_count()
    }

    /// Get all current groups
    ///
    /// Returns all groups in the system. Every speaker is always in a group,
    /// so a single speaker forms a group of one.
    pub fn groups(&self) -> Vec<GroupInfo> {
        self.store.read().groups.values().cloned().collect()
    }

    /// Get a specific group by ID
    pub fn get_group(&self, group_id: &GroupId) -> Option<GroupInfo> {
        self.store.read().groups.get(group_id).cloned()
    }

    /// Get the group a speaker belongs to
    ///
    /// Uses the speaker_to_group mapping for quick lookup.
    pub fn get_group_for_speaker(&self, speaker_id: &SpeakerId) -> Option<GroupInfo> {
        let store = self.store.read();
        let group_id = store.speaker_to_group.get(speaker_id)?;
        store.groups.get(group_id).cloned()
    }

    /// Resolve the subscription target for a PerCoordinator service.
    ///
    /// For PerCoordinator services, returns the coordinator's `(SpeakerId, IpAddr)`
    /// so the SDK can route UPnP subscriptions to the coordinator speaker.
    /// Falls back to the speaker itself if no group data exists.
    ///
    /// For non-PerCoordinator services, returns the speaker's own identity.
    pub fn resolve_subscription_target(
        &self,
        speaker_id: &SpeakerId,
        speaker_ip: IpAddr,
        service: Service,
    ) -> (SpeakerId, IpAddr) {
        if service.scope() == ServiceScope::PerCoordinator {
            let store = self.store.read();
            let coordinator_id = store.resolve_coordinator(speaker_id);
            if coordinator_id == *speaker_id {
                (speaker_id.clone(), speaker_ip)
            } else {
                let coord_ip = store
                    .speaker(&coordinator_id)
                    .map(|s| s.ip_address)
                    .unwrap_or(speaker_ip);
                (coordinator_id, coord_ip)
            }
        } else {
            (speaker_id.clone(), speaker_ip)
        }
    }

    /// Get access to the event manager (if configured)
    ///
    /// This allows PropertyHandle::watch() to trigger UPnP subscriptions
    /// via the event manager's ensure_service_subscribed() method.
    pub fn event_manager(&self) -> Option<&Arc<SonosEventManager>> {
        self.event_manager.get()
    }

    /// Wire an event manager into this StateManager after construction.
    ///
    /// Spawns the event worker thread and registers all known devices.
    /// Can only be called once — subsequent calls are no-ops.
    pub fn set_event_manager(&self, em: Arc<SonosEventManager>) -> Result<()> {
        tracing::debug!("StateManager::set_event_manager called");
        if self.event_manager.set(Arc::clone(&em)).is_err() {
            tracing::debug!("Event manager already set — no-op");
            return Ok(()); // Already set — no-op
        }

        // Wire this StateManager as the WatchRegistry
        em.set_watch_registry(Arc::new(StateWatchRegistry {
            watched: Arc::clone(&self.watched),
            ip_to_speaker: Arc::clone(&self.ip_to_speaker),
            key_to_service: Arc::clone(&self.key_to_service),
        }));

        // Register all known devices with the event manager
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
            tracing::warn!(
                "Failed to add devices to event manager during lazy init: {}",
                e
            );
        }

        // Spawn event worker thread
        let worker = spawn_state_event_worker(
            em,
            Arc::clone(&self.store),
            Arc::clone(&self.watched),
            Arc::clone(&self.fanout),
            Arc::clone(&self.ip_to_speaker),
        );
        info!("StateManager event worker started (lazy init)");

        if let Ok(mut w) = self._worker.lock() {
            *w = Some(worker);
        }

        Ok(())
    }

    /// Set the lazy event manager initialization closure.
    ///
    /// Called once by `SonosSystem::from_devices_inner()` after construction.
    /// Subsequent calls are no-ops (OnceLock semantics).
    pub fn set_event_init(&self, f: EventInitFn) {
        let _ = self.event_init.set(f);
    }

    /// Get the event init closure (if set).
    ///
    /// Used by `PropertyHandle::watch()` and `GroupPropertyHandle::watch()`
    /// to trigger lazy event manager creation on first use.
    pub fn event_init(&self) -> Option<&EventInitFn> {
        self.event_init.get()
    }
}

impl Clone for StateManager {
    fn clone(&self) -> Self {
        let event_manager = OnceLock::new();
        if let Some(em) = self.event_manager.get() {
            let _ = event_manager.set(Arc::clone(em));
        }
        let event_init = OnceLock::new();
        if let Some(f) = self.event_init.get() {
            let _ = event_init.set(Arc::clone(f));
        }
        Self {
            store: Arc::clone(&self.store),
            watched: Arc::clone(&self.watched),
            ip_to_speaker: Arc::clone(&self.ip_to_speaker),
            event_manager,
            fanout: Arc::clone(&self.fanout),
            _worker: Mutex::new(None),
            cleanup_timeout: self.cleanup_timeout,
            key_to_service: Arc::clone(&self.key_to_service),
            event_init,
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
        let fanout = Arc::new(EventFanout::new());

        let store = Arc::new(RwLock::new(StateStore::new()));
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let ip_to_speaker = Arc::new(RwLock::new(HashMap::new()));
        let key_to_service = Arc::new(RwLock::new(HashMap::new()));

        let event_manager_lock = OnceLock::new();
        let mut worker = None;

        // If event_manager provided at build time, wire it up eagerly
        if let Some(em) = self.event_manager {
            let _ = event_manager_lock.set(Arc::clone(&em));

            // Wire WatchRegistry
            em.set_watch_registry(Arc::new(StateWatchRegistry {
                watched: Arc::clone(&watched),
                ip_to_speaker: Arc::clone(&ip_to_speaker),
                key_to_service: Arc::clone(&key_to_service),
            }));

            let worker_handle = spawn_state_event_worker(
                em,
                Arc::clone(&store),
                Arc::clone(&watched),
                Arc::clone(&fanout),
                Arc::clone(&ip_to_speaker),
            );
            info!("StateManager event worker started");
            worker = Some(worker_handle);
        }

        let manager = StateManager {
            store,
            watched,
            ip_to_speaker,
            event_manager: event_manager_lock,
            fanout,
            _worker: Mutex::new(worker),
            cleanup_timeout: self.cleanup_timeout,
            key_to_service,
            event_init: OnceLock::new(),
        };

        info!("StateManager created (sync-first mode)");
        Ok(manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iter::ChangeIterator;
    use crate::property::{GroupVolume, PlaybackState, Volume};
    use sonos_api::Service;

    /// An event-sourced stamp for "now", for tests that only need a valid one.
    fn test_stamp() -> WriteStamp {
        WriteStamp::now(ChangeSource::Event)
    }

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
        assert_eq!(
            manager.get_property::<Volume>(&speaker_id),
            Some(Volume::new(50))
        );
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

        // Subscribe before writing: an iterator receives events emitted after
        // it exists, not a replay of everything since the manager was built.
        let iter = manager.iter();

        // Set property (should emit event)
        manager.set_property(&speaker_id, Volume::new(75));
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.speaker_id.as_str(), "RINCON_123");
        assert_eq!(event.property_key(), "volume");
    }

    #[test]
    fn test_set_group_property_emits_change_event() {
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
        let group_id = GroupId::new("RINCON_123:1");

        // Add group so coordinator lookup works
        {
            let mut store = manager.store.write();
            store.add_group(GroupInfo::new(
                group_id.clone(),
                speaker_id.clone(),
                vec![speaker_id.clone()],
            ));
        }

        // Register watch on coordinator for group_volume
        manager.register_watch(&speaker_id, "group_volume");

        let iter = manager.iter();

        // Set group property (should emit event via coordinator)
        manager.set_group_property(&group_id, GroupVolume::new(80));

        // Verify event was emitted
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));
        assert!(event.is_some());

        let event = event.unwrap();
        assert_eq!(event.speaker_id.as_str(), "RINCON_123");
        assert_eq!(event.property_key(), "group_volume");
        assert_eq!(event.service(), Service::GroupRenderingControl);
    }

    #[test]
    fn test_set_group_property_no_event_when_unwatched() {
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
        let group_id = GroupId::new("RINCON_123:1");

        {
            let mut store = manager.store.write();
            store.add_group(GroupInfo::new(
                group_id.clone(),
                speaker_id.clone(),
                vec![speaker_id.clone()],
            ));
        }

        let iter = manager.iter();

        // Don't register any watch
        manager.set_group_property(&group_id, GroupVolume::new(50));
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));
        assert!(event.is_none());
    }

    // ========================================================================
    // StateStore Group Operations Tests
    // ========================================================================

    #[test]
    fn test_add_group_updates_speaker_to_group() {
        let mut store = StateStore::new();

        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");
        let group_id = GroupId::new("RINCON_111:1");

        let group = GroupInfo::new(
            group_id.clone(),
            speaker1.clone(),
            vec![speaker1.clone(), speaker2.clone()],
        );

        store.add_group(group);

        // Verify speaker_to_group mapping is updated for all members
        assert_eq!(store.speaker_to_group.get(&speaker1), Some(&group_id));
        assert_eq!(store.speaker_to_group.get(&speaker2), Some(&group_id));
    }

    #[test]
    fn test_add_group_single_speaker() {
        let mut store = StateStore::new();

        let speaker = SpeakerId::new("RINCON_333");
        let group_id = GroupId::new("RINCON_333:1");

        let group = GroupInfo::new(group_id.clone(), speaker.clone(), vec![speaker.clone()]);

        store.add_group(group.clone());

        // Verify speaker_to_group mapping
        assert_eq!(store.speaker_to_group.get(&speaker), Some(&group_id));

        // Verify group is stored
        assert_eq!(store.groups.get(&group_id), Some(&group));
    }

    #[test]
    fn test_get_group_for_speaker_returns_correct_group() {
        let mut store = StateStore::new();

        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");
        let speaker3 = SpeakerId::new("RINCON_333");
        let group1_id = GroupId::new("RINCON_111:1");
        let group2_id = GroupId::new("RINCON_333:1");

        // Group 1: speaker1 (coordinator) + speaker2
        let group1 = GroupInfo::new(
            group1_id.clone(),
            speaker1.clone(),
            vec![speaker1.clone(), speaker2.clone()],
        );

        // Group 2: speaker3 alone
        let group2 = GroupInfo::new(group2_id.clone(), speaker3.clone(), vec![speaker3.clone()]);

        store.add_group(group1.clone());
        store.add_group(group2.clone());

        // Verify get_group_for_speaker returns correct groups
        assert_eq!(store.get_group_for_speaker(&speaker1), Some(&group1));
        assert_eq!(store.get_group_for_speaker(&speaker2), Some(&group1));
        assert_eq!(store.get_group_for_speaker(&speaker3), Some(&group2));
    }

    #[test]
    fn test_get_group_for_speaker_returns_none_for_unknown() {
        let store = StateStore::new();

        let unknown_speaker = SpeakerId::new("RINCON_UNKNOWN");

        assert!(store.get_group_for_speaker(&unknown_speaker).is_none());
    }

    #[test]
    fn test_clear_groups_removes_all_group_data() {
        let mut store = StateStore::new();

        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");
        let group_id = GroupId::new("RINCON_111:1");

        let group = GroupInfo::new(
            group_id.clone(),
            speaker1.clone(),
            vec![speaker1.clone(), speaker2.clone()],
        );

        store.add_group(group);

        // Verify data exists
        assert!(!store.groups.is_empty());
        assert!(!store.speaker_to_group.is_empty());

        // Clear groups
        store.clear_groups();

        // Verify all group data is cleared
        assert!(store.groups.is_empty());
        assert!(store.group_props.is_empty());
        assert!(store.speaker_to_group.is_empty());
    }

    #[test]
    fn test_clear_groups_then_add_new_groups() {
        let mut store = StateStore::new();

        // Add initial group
        let speaker1 = SpeakerId::new("RINCON_111");
        let group1_id = GroupId::new("RINCON_111:1");
        let group1 = GroupInfo::new(group1_id.clone(), speaker1.clone(), vec![speaker1.clone()]);
        store.add_group(group1);

        // Clear and add new group
        store.clear_groups();

        let speaker2 = SpeakerId::new("RINCON_222");
        let group2_id = GroupId::new("RINCON_222:1");
        let group2 = GroupInfo::new(group2_id.clone(), speaker2.clone(), vec![speaker2.clone()]);
        store.add_group(group2.clone());

        // Verify old group is gone, new group exists
        assert!(!store.groups.contains_key(&group1_id));
        assert_eq!(store.groups.get(&group2_id), Some(&group2));

        // Verify speaker_to_group is updated correctly
        assert!(!store.speaker_to_group.contains_key(&speaker1));
        assert_eq!(store.speaker_to_group.get(&speaker2), Some(&group2_id));
    }

    // ========================================================================
    // StateManager Group Methods Tests
    // ========================================================================

    #[test]
    fn test_state_manager_groups_returns_all_groups() {
        let manager = StateManager::new().unwrap();

        // Add devices
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
        manager.add_devices(devices).unwrap();

        // Create groups via initialize
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
            manager.speaker_infos(),
            vec![group1.clone(), group2.clone()],
        );
        manager.initialize(topology);

        // Verify groups() returns all groups
        let groups = manager.groups();
        assert_eq!(groups.len(), 2);

        // Verify both groups are present (order may vary)
        let group_ids: Vec<_> = groups.iter().map(|g| g.id.clone()).collect();
        assert!(group_ids.contains(&GroupId::new("RINCON_111:1")));
        assert!(group_ids.contains(&GroupId::new("RINCON_222:1")));
    }

    #[test]
    fn test_state_manager_groups_returns_empty_when_no_groups() {
        let manager = StateManager::new().unwrap();

        // No groups added
        let groups = manager.groups();
        assert!(groups.is_empty());
    }

    #[test]
    fn test_state_manager_get_group_returns_correct_group() {
        let manager = StateManager::new().unwrap();

        // Add device
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        manager.add_devices(devices).unwrap();

        // Create group via initialize
        let speaker = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");
        let group = GroupInfo::new(group_id.clone(), speaker.clone(), vec![speaker.clone()]);

        let topology = Topology::new(manager.speaker_infos(), vec![group.clone()]);
        manager.initialize(topology);

        // Verify get_group returns the correct group
        let found = manager.get_group(&group_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), group);
    }

    #[test]
    fn test_state_manager_get_group_returns_none_for_unknown() {
        let manager = StateManager::new().unwrap();

        // No groups added
        let unknown_id = GroupId::new("RINCON_UNKNOWN:1");
        let found = manager.get_group(&unknown_id);
        assert!(found.is_none());
    }

    #[test]
    fn test_state_manager_get_group_for_speaker_returns_correct_group() {
        let manager = StateManager::new().unwrap();

        // Add devices
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
        manager.add_devices(devices).unwrap();

        // Create a group with both speakers
        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");
        let group_id = GroupId::new("RINCON_111:1");
        let group = GroupInfo::new(
            group_id.clone(),
            speaker1.clone(),
            vec![speaker1.clone(), speaker2.clone()],
        );

        let topology = Topology::new(manager.speaker_infos(), vec![group.clone()]);
        manager.initialize(topology);

        // Verify get_group_for_speaker returns the correct group for both speakers
        let found1 = manager.get_group_for_speaker(&speaker1);
        assert!(found1.is_some());
        assert_eq!(found1.unwrap(), group);

        let found2 = manager.get_group_for_speaker(&speaker2);
        assert!(found2.is_some());
        assert_eq!(found2.unwrap(), group);
    }

    #[test]
    fn test_state_manager_get_group_for_speaker_returns_none_for_unknown() {
        let manager = StateManager::new().unwrap();

        // No groups added
        let unknown_speaker = SpeakerId::new("RINCON_UNKNOWN");
        let found = manager.get_group_for_speaker(&unknown_speaker);
        assert!(found.is_none());
    }

    #[test]
    fn test_state_manager_group_methods_consistency() {
        let manager = StateManager::new().unwrap();

        // Add device
        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        manager.add_devices(devices).unwrap();

        // Create group via initialize
        let speaker = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");
        let group = GroupInfo::new(group_id.clone(), speaker.clone(), vec![speaker.clone()]);

        let topology = Topology::new(manager.speaker_infos(), vec![group.clone()]);
        manager.initialize(topology);

        // Verify all three methods return consistent data
        let groups = manager.groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], group);

        let by_id = manager.get_group(&group_id);
        assert_eq!(by_id, Some(group.clone()));

        let by_speaker = manager.get_group_for_speaker(&speaker);
        assert_eq!(by_speaker, Some(group.clone()));

        // All should return the same group
        assert_eq!(groups[0], by_id.unwrap());
        assert_eq!(groups[0], by_speaker.unwrap());
    }

    // ========================================================================
    // boot_seq Tests
    // ========================================================================

    #[test]
    fn test_get_boot_seq_returns_none_for_unknown_speaker() {
        let manager = StateManager::new().unwrap();
        let unknown = SpeakerId::new("RINCON_UNKNOWN");
        assert!(manager.get_boot_seq(&unknown).is_none());
    }

    #[test]
    fn test_boot_seq_defaults_to_zero_for_new_speaker() {
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

        // Before any topology event, boot_seq should be 0
        assert_eq!(manager.get_boot_seq(&speaker_id), Some(0));
    }

    // ========================================================================
    // StateWatchRegistry Tests
    // ========================================================================

    #[test]
    fn test_state_watch_registry_register_and_unregister() {
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let ip_to_speaker = Arc::new(RwLock::new(HashMap::new()));
        let key_to_service = Arc::new(RwLock::new(HashMap::new()));

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");
        ip_to_speaker.write().insert(ip, speaker_id.clone());

        let registry = StateWatchRegistry {
            watched: Arc::clone(&watched),
            ip_to_speaker: Arc::clone(&ip_to_speaker),
            key_to_service: Arc::clone(&key_to_service),
        };

        // Register watches on two services
        registry.register_watch(&speaker_id, "volume", Service::RenderingControl);
        registry.register_watch(&speaker_id, "mute", Service::RenderingControl);
        registry.register_watch(&speaker_id, "playback_state", Service::AVTransport);

        assert_eq!(watched.read().len(), 3);

        // Unregister RenderingControl — should remove volume + mute, keep playback_state
        registry.unregister_watches_for_service(ip, Service::RenderingControl);

        let w = watched.read();
        assert_eq!(w.len(), 1);
        assert!(is_pair_watched(&w, &speaker_id, "playback_state"));
        assert!(!is_pair_watched(&w, &speaker_id, "volume"));
        assert!(!is_pair_watched(&w, &speaker_id, "mute"));
    }

    #[test]
    fn test_state_watch_registry_unknown_ip_is_noop() {
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let ip_to_speaker = Arc::new(RwLock::new(HashMap::new()));
        let key_to_service = Arc::new(RwLock::new(HashMap::new()));

        let speaker_id = SpeakerId::new("RINCON_123");

        let registry = StateWatchRegistry {
            watched: Arc::clone(&watched),
            ip_to_speaker,
            key_to_service: Arc::clone(&key_to_service),
        };

        // Register a watch (simulating direct add to shared set)
        retain_direct_watch(&watched, &speaker_id, "volume");
        key_to_service
            .write()
            .insert("volume", Service::RenderingControl);

        // Unregister for an unknown IP — should be a no-op
        let unknown_ip: IpAddr = "10.0.0.1".parse().unwrap();
        registry.unregister_watches_for_service(unknown_ip, Service::RenderingControl);

        // Watch should still be there
        assert_eq!(watched.read().len(), 1);
    }

    #[test]
    fn test_state_watch_registry_only_removes_matching_speaker() {
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let ip_to_speaker = Arc::new(RwLock::new(HashMap::new()));
        let key_to_service = Arc::new(RwLock::new(HashMap::new()));

        let ip1: IpAddr = "192.168.1.100".parse().unwrap();
        let ip2: IpAddr = "192.168.1.101".parse().unwrap();
        let speaker1 = SpeakerId::new("RINCON_111");
        let speaker2 = SpeakerId::new("RINCON_222");

        ip_to_speaker.write().insert(ip1, speaker1.clone());
        ip_to_speaker.write().insert(ip2, speaker2.clone());

        let registry = StateWatchRegistry {
            watched: Arc::clone(&watched),
            ip_to_speaker,
            key_to_service: Arc::clone(&key_to_service),
        };

        // Both speakers watch volume
        registry.register_watch(&speaker1, "volume", Service::RenderingControl);
        registry.register_watch(&speaker2, "volume", Service::RenderingControl);
        assert_eq!(watched.read().len(), 2);

        // Unregister only speaker1's IP
        registry.unregister_watches_for_service(ip1, Service::RenderingControl);

        let w = watched.read();
        assert_eq!(w.len(), 1);
        assert!(is_pair_watched(&w, &speaker2, "volume"));
        assert!(!is_pair_watched(&w, &speaker1, "volume"));
    }

    // ========================================================================
    // Watch reference counting
    // ========================================================================

    /// Two watchers on the *same* property: the first release must not silence
    /// the second, and the second must actually clear it.
    ///
    /// This is the arithmetic behind the sibling-survival guarantee. With a
    /// plain `HashSet` the first `unregister_watch` removed the only entry, so
    /// watcher two went quiet while still holding its handle.
    #[test]
    fn test_watch_refcount_survives_partial_release() {
        let manager = StateManager::new().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");

        manager.register_watch(&speaker_id, "volume");
        manager.register_watch(&speaker_id, "volume");
        assert!(manager.is_watched(&speaker_id, "volume"));

        // One watcher goes away; the other still holds a reference.
        manager.unregister_watch(&speaker_id, "volume");
        assert!(
            manager.is_watched(&speaker_id, "volume"),
            "one of two watchers released — the property must stay watched"
        );

        // Last watcher goes away.
        manager.unregister_watch(&speaker_id, "volume");
        assert!(!manager.is_watched(&speaker_id, "volume"));

        // Over-release must not wrap around and resurrect the watch.
        manager.unregister_watch(&speaker_id, "volume");
        assert!(!manager.is_watched(&speaker_id, "volume"));
    }

    /// A subscription teardown for one service must not take individually-held
    /// watches with it.
    ///
    /// `unregister_watches_for_service` clears every key of a service at once.
    /// Previously it removed the map entries outright, so a `direct` hold taken
    /// by the polling-fallback / cache-only path — or by a second watcher of the
    /// same property — was destroyed by an unrelated subscription expiring.
    #[test]
    fn test_service_unregister_keeps_directly_held_watches() {
        let watched = Arc::new(RwLock::new(WatchCounts::new()));
        let ip_to_speaker = Arc::new(RwLock::new(HashMap::new()));
        let key_to_service = Arc::new(RwLock::new(HashMap::new()));

        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        let speaker_id = SpeakerId::new("RINCON_123");
        ip_to_speaker.write().insert(ip, speaker_id.clone());

        let registry = StateWatchRegistry {
            watched: Arc::clone(&watched),
            ip_to_speaker,
            key_to_service,
        };

        // A guard-based watch and a direct watch on the same property...
        registry.register_watch(&speaker_id, "volume", Service::RenderingControl);
        retain_direct_watch(&watched, &speaker_id, "volume");
        // ...plus a direct hold on a sibling property of the same service.
        registry.register_watch(&speaker_id, "mute", Service::RenderingControl);
        retain_direct_watch(&watched, &speaker_id, "mute");

        registry.unregister_watches_for_service(ip, Service::RenderingControl);

        let w = watched.read();
        assert!(
            is_pair_watched(&w, &speaker_id, "volume"),
            "the direct hold on volume must survive the subscription teardown"
        );
        assert!(
            is_pair_watched(&w, &speaker_id, "mute"),
            "the direct hold on mute must survive the subscription teardown"
        );
    }

    // ========================================================================
    // set_property / get_property symmetry
    // ========================================================================

    /// `set_property` must write where `get_property` reads.
    ///
    /// For a `PerCoordinator` speaker-scoped property, `get_resolved` reads the
    /// *coordinator's* bag. Writing the raw `speaker_id` therefore stored the
    /// value where nothing would ever look: `speaker.play()` on a grouped member
    /// updated a bag no reader consults, so the optimistic cache update was
    /// invisible from both the member and the coordinator.
    #[test]
    fn test_set_property_on_group_member_is_readable_from_both() {
        let manager = StateManager::new().unwrap();

        let devices = vec![
            Device {
                id: "RINCON_COORD".to_string(),
                name: "Living Room".to_string(),
                room_name: "Living Room".to_string(),
                ip_address: "192.168.1.100".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
            Device {
                id: "RINCON_MEMBER".to_string(),
                name: "Kitchen".to_string(),
                room_name: "Kitchen".to_string(),
                ip_address: "192.168.1.101".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
        ];
        manager.add_devices(devices).unwrap();

        let coordinator = SpeakerId::new("RINCON_COORD");
        let member = SpeakerId::new("RINCON_MEMBER");
        let group_id = GroupId::new("RINCON_COORD:1");
        let topology = Topology::new(
            manager.speaker_infos(),
            vec![GroupInfo::new(
                group_id,
                coordinator.clone(),
                vec![coordinator.clone(), member.clone()],
            )],
        );
        manager.initialize(topology);

        // Write through the *member* — what speaker.play() does on a grouped
        // speaker. PlaybackState is AVTransport (PerCoordinator) + Speaker scope.
        manager.set_property(&member, PlaybackState::Playing);

        assert_eq!(
            manager.get_property::<PlaybackState>(&member),
            Some(PlaybackState::Playing),
            "the member must be able to read back what it just wrote"
        );
        assert_eq!(
            manager.get_property::<PlaybackState>(&coordinator),
            Some(PlaybackState::Playing),
            "the write belongs in the coordinator's bag, which is where reads resolve"
        );

        // A PerSpeaker property written on the member stays on the member.
        manager.set_property(&member, Volume::new(33));
        assert_eq!(
            manager.get_property::<Volume>(&member),
            Some(Volume::new(33))
        );
        assert_eq!(
            manager.get_property::<Volume>(&coordinator),
            None,
            "PerSpeaker writes must not be redirected to the coordinator"
        );
    }

    // ========================================================================
    // resolve_coordinator Tests
    // ========================================================================

    #[test]
    fn test_resolve_coordinator_for_standalone_speaker() {
        let mut store = StateStore::new();

        let speaker = SpeakerId::new("RINCON_111");
        let group_id = GroupId::new("RINCON_111:1");

        store.add_speaker(SpeakerInfo {
            id: speaker.clone(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            boot_seq: 0,
            satellites: vec![],
        });
        store.add_group(GroupInfo::new(
            group_id,
            speaker.clone(),
            vec![speaker.clone()],
        ));

        // Standalone speaker is its own coordinator
        assert_eq!(store.resolve_coordinator(&speaker), speaker);
    }

    #[test]
    fn test_resolve_coordinator_for_group_member() {
        let mut store = StateStore::new();

        let coordinator = SpeakerId::new("RINCON_COORD");
        let member = SpeakerId::new("RINCON_MEMBER");
        let group_id = GroupId::new("RINCON_COORD:1");

        store.add_group(GroupInfo::new(
            group_id,
            coordinator.clone(),
            vec![coordinator.clone(), member.clone()],
        ));

        // Member resolves to the coordinator
        assert_eq!(store.resolve_coordinator(&member), coordinator);
        // Coordinator resolves to itself
        assert_eq!(store.resolve_coordinator(&coordinator), coordinator);
    }

    #[test]
    fn test_resolve_coordinator_no_group_data() {
        let store = StateStore::new();

        let speaker = SpeakerId::new("RINCON_UNKNOWN");

        // No group data — falls back to speaker's own ID
        assert_eq!(store.resolve_coordinator(&speaker), speaker);
    }

    // ========================================================================
    // get_resolved Tests
    // ========================================================================

    #[test]
    fn test_get_resolved_per_coordinator_reads_from_coordinator() {
        let mut store = StateStore::new();

        let coordinator = SpeakerId::new("RINCON_COORD");
        let member = SpeakerId::new("RINCON_MEMBER");
        let group_id = GroupId::new("RINCON_COORD:1");

        store.add_speaker(SpeakerInfo {
            id: coordinator.clone(),
            name: "Coord".to_string(),
            room_name: "Coord".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            boot_seq: 0,
            satellites: vec![],
        });
        store.add_speaker(SpeakerInfo {
            id: member.clone(),
            name: "Member".to_string(),
            room_name: "Member".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            boot_seq: 0,
            satellites: vec![],
        });
        store.add_group(GroupInfo::new(
            group_id,
            coordinator.clone(),
            vec![coordinator.clone(), member.clone()],
        ));

        // Set PlaybackState only on coordinator
        store.set(&coordinator, PlaybackState::Playing, test_stamp());

        // get_resolved on member should return coordinator's value (PerCoordinator + Speaker scope)
        let resolved: Option<PlaybackState> = store.get_resolved(&member);
        assert_eq!(resolved, Some(PlaybackState::Playing));

        // Direct get on member should return None (no data copied)
        let direct: Option<PlaybackState> = store.get(&member);
        assert_eq!(direct, None);
    }

    #[test]
    fn test_get_resolved_per_speaker_reads_own_props() {
        let mut store = StateStore::new();

        let coordinator = SpeakerId::new("RINCON_COORD");
        let member = SpeakerId::new("RINCON_MEMBER");
        let group_id = GroupId::new("RINCON_COORD:1");

        store.add_speaker(SpeakerInfo {
            id: coordinator.clone(),
            name: "Coord".to_string(),
            room_name: "Coord".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            boot_seq: 0,
            satellites: vec![],
        });
        store.add_speaker(SpeakerInfo {
            id: member.clone(),
            name: "Member".to_string(),
            room_name: "Member".to_string(),
            ip_address: "192.168.1.101".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            boot_seq: 0,
            satellites: vec![],
        });
        store.add_group(GroupInfo::new(
            group_id,
            coordinator.clone(),
            vec![coordinator.clone(), member.clone()],
        ));

        // Set Volume on coordinator only (PerSpeaker service)
        store.set(&coordinator, Volume::new(80), test_stamp());

        // get_resolved on member should NOT resolve to coordinator for PerSpeaker
        let resolved: Option<Volume> = store.get_resolved(&member);
        assert_eq!(resolved, None);

        // get_resolved on coordinator returns its own value
        let coord_resolved: Option<Volume> = store.get_resolved(&coordinator);
        assert_eq!(coord_resolved, Some(Volume::new(80)));
    }

    #[test]
    fn test_update_speaker_ip() {
        let manager = StateManager::new().unwrap();

        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Office".to_string(),
            room_name: "Office".to_string(),
            ip_address: "192.168.4.198".to_string(),
            port: 1400,
            model_name: "Roam 2".to_string(),
        }];
        manager.add_devices(devices).unwrap();

        let speaker_id = SpeakerId::new("RINCON_111");
        let old_ip: IpAddr = "192.168.4.198".parse().unwrap();
        let new_ip: IpAddr = "192.168.4.200".parse().unwrap();

        // Verify initial state
        assert_eq!(manager.get_speaker_ip(&speaker_id), Some(old_ip));

        // Update IP
        manager.update_speaker_ip(&speaker_id, new_ip);

        // Verify forward map updated
        assert_eq!(manager.get_speaker_ip(&speaker_id), Some(new_ip));

        // Verify reverse map updated (old IP removed, new IP present)
        let ip_map = manager.ip_to_speaker.read();
        assert!(!ip_map.contains_key(&old_ip));
        assert_eq!(ip_map.get(&new_ip), Some(&speaker_id));
    }

    #[test]
    fn test_update_speaker_ip_no_change() {
        let manager = StateManager::new().unwrap();

        let devices = vec![Device {
            id: "RINCON_111".to_string(),
            name: "Office".to_string(),
            room_name: "Office".to_string(),
            ip_address: "192.168.4.198".to_string(),
            port: 1400,
            model_name: "Roam 2".to_string(),
        }];
        manager.add_devices(devices).unwrap();

        let speaker_id = SpeakerId::new("RINCON_111");
        let same_ip: IpAddr = "192.168.4.198".parse().unwrap();

        // Update with same IP — should be a no-op
        manager.update_speaker_ip(&speaker_id, same_ip);
        assert_eq!(manager.get_speaker_ip(&speaker_id), Some(same_ip));
    }

    #[test]
    fn test_satellite_ids() {
        let manager = StateManager::new().unwrap();

        assert!(manager.get_satellite_ids().is_empty());

        let ids = vec![SpeakerId::new("RINCON_SAT1"), SpeakerId::new("RINCON_SAT2")];
        manager.set_satellite_ids(ids.clone());

        let stored = manager.get_satellite_ids();
        assert_eq!(stored.len(), 2);
        assert!(stored.contains(&SpeakerId::new("RINCON_SAT1")));
        assert!(stored.contains(&SpeakerId::new("RINCON_SAT2")));
    }

    // ========================================================================
    // Change events carry values / monotonic write ordering
    // ========================================================================

    /// The headline win: a queued burst is fully observable.
    ///
    /// `Playing -> Transitioning -> Playing` is three events but only one final
    /// store value. A consumer that drained the queue and re-read the store saw
    /// `Playing` three times — the `Transitioning` state, and the fact that
    /// anything moved at all, were unrecoverable. Carrying the value on the
    /// event makes the whole sequence visible.
    ///
    /// Deliberately does *not* assert on the store: the store holding the final
    /// `Playing` is correct and unchanged. The point is that the channel now
    /// preserves what the store cannot.
    #[test]
    fn test_queued_events_preserve_every_intermediate_value() {
        let manager = StateManager::new().unwrap();
        manager
            .add_devices(vec![Device {
                id: "RINCON_QUEUE".to_string(),
                name: "Queue Test".to_string(),
                room_name: "Test".to_string(),
                ip_address: "192.0.2.10".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            }])
            .unwrap();

        let speaker_id = SpeakerId::new("RINCON_QUEUE");
        manager.register_watch(&speaker_id, PlaybackState::KEY);

        let iter = manager.iter();

        // Queue all three transitions *before* reading any of them, which is
        // exactly the render-loop-behind-by-a-frame case.
        manager.set_property(&speaker_id, PlaybackState::Playing);
        manager.set_property(&speaker_id, PlaybackState::Transitioning);
        manager.set_property(&speaker_id, PlaybackState::Playing);
        let observed: Vec<PlaybackState> = iter
            .try_iter()
            .filter_map(|e| match e.change {
                PropertyChange::PlaybackState(s) => Some(s),
                _ => None,
            })
            .collect();

        assert_eq!(
            observed,
            vec![
                PlaybackState::Playing,
                PlaybackState::Transitioning,
                PlaybackState::Playing,
            ],
            "every queued value must be observable from the event stream"
        );

        // And the store still holds only the final value — which is why the
        // event payload is the only way to see the middle one.
        assert_eq!(
            manager.get_property::<PlaybackState>(&speaker_id),
            Some(PlaybackState::Playing)
        );
    }

    /// A manager with one speaker watched for `Volume`, ready to emit.
    fn manager_watching_volume() -> (StateManager, SpeakerId) {
        let manager = StateManager::new().unwrap();
        manager
            .add_devices(vec![Device {
                id: "RINCON_FANOUT".to_string(),
                name: "Living Room".to_string(),
                room_name: "Living Room".to_string(),
                // RFC 5737 TEST-NET-1: documentation-only, never routed.
                ip_address: "192.0.2.10".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            }])
            .unwrap();
        let speaker_id = SpeakerId::new("RINCON_FANOUT");
        manager.register_watch(&speaker_id, Volume::KEY);
        (manager, speaker_id)
    }

    fn volumes_from(iter: &ChangeIterator) -> Vec<u8> {
        iter.try_iter()
            .filter_map(|e| match e.change {
                PropertyChange::Volume(v) => Some(v.value()),
                _ => None,
            })
            .collect()
    }

    /// **The defect this fan-out exists to fix.** Two independent `iter()` loops
    /// must each see the *whole* stream.
    ///
    /// Previously both iterators locked one shared receiver, so every event went
    /// to whichever consumer won the lock and each saw only a random subset —
    /// silently, with no error and no log. A dashboard that added a second event
    /// loop simply started missing half its updates.
    #[test]
    fn test_two_iterators_each_receive_every_event() {
        let (manager, speaker_id) = manager_watching_volume();

        let dashboard = manager.iter();
        let logger = manager.iter();

        for v in [10u8, 20, 30, 40] {
            manager.set_property(&speaker_id, Volume::new(v));
        }

        // Neither consumer is missing anything, and neither stole from the other.
        assert_eq!(
            volumes_from(&dashboard),
            vec![10, 20, 30, 40],
            "the first iterator must see every event"
        );
        assert_eq!(
            volumes_from(&logger),
            vec![10, 20, 30, 40],
            "the second iterator must see every event too, not a subset"
        );
    }

    /// No-regression baseline: one consumer still receives every event, in the
    /// order it was emitted. Fanning out must not reorder or drop anything, which
    /// is what keeps the observation-time ordering of 4.1a meaningful downstream.
    #[test]
    fn test_single_iterator_receives_every_event_in_order() {
        let (manager, speaker_id) = manager_watching_volume();

        let iter = manager.iter();
        let sent: Vec<u8> = (1..=25).collect();
        for &v in &sent {
            manager.set_property(&speaker_id, Volume::new(v));
        }

        assert_eq!(volumes_from(&iter), sent);
    }

    /// Dropping one consumer must neither stall the survivor nor leak its slot.
    ///
    /// The departed iterator's queue is released, and the remaining one keeps
    /// receiving — the sender side does not wedge on a dead subscriber or keep
    /// feeding it forever.
    #[test]
    fn test_dropped_consumer_does_not_stall_survivor() {
        let (manager, speaker_id) = manager_watching_volume();

        let survivor = manager.iter();
        let departing = manager.iter();

        manager.set_property(&speaker_id, Volume::new(5));
        drop(departing);

        // The survivor still gets everything, before and after the departure.
        manager.set_property(&speaker_id, Volume::new(6));
        manager.set_property(&speaker_id, Volume::new(7));

        assert_eq!(
            volumes_from(&survivor),
            vec![5, 6, 7],
            "the remaining consumer must keep receiving after a sibling drops"
        );

        // And a fresh subscriber still works, so the registry is not corrupted.
        let latecomer = manager.iter();
        manager.set_property(&speaker_id, Volume::new(8));
        assert_eq!(volumes_from(&latecomer), vec![8]);
    }

    /// A slow `fetch()` must not overwrite a newer event-derived value.
    ///
    /// Simulates the real race by timestamp rather than by threads: the fetch
    /// observation is stamped *before* the event's, as it would be if the SOAP
    /// request were issued first and its response arrived second.
    #[test]
    fn test_stale_fetch_does_not_clobber_newer_event_value() {
        let manager = StateManager::new().unwrap();
        manager
            .add_devices(vec![Device {
                id: "RINCON_RACE".to_string(),
                name: "Race Test".to_string(),
                room_name: "Test".to_string(),
                ip_address: "192.0.2.11".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            }])
            .unwrap();

        let speaker_id = SpeakerId::new("RINCON_RACE");

        // t0: a fetch is issued (observation made), but its response is still
        // in flight.
        let fetch_observed_at = Instant::now();

        // t1: an event arrives and lands first with the newer, correct value.
        let event_outcome = manager.set_property_stamped(
            &speaker_id,
            Volume::new(40),
            WriteStamp::now(ChangeSource::Event),
        );
        assert_eq!(event_outcome, WriteOutcome::Changed);

        // t2: the fetch response finally lands, carrying the *older* reading.
        let fetch_outcome = manager.set_property_stamped(
            &speaker_id,
            Volume::new(10),
            WriteStamp::observed_at(ChangeSource::Fetch, fetch_observed_at),
        );

        assert_eq!(
            fetch_outcome,
            WriteOutcome::Stale,
            "a fetch observed before the stored event must be rejected"
        );
        assert_eq!(
            manager.get_property::<Volume>(&speaker_id),
            Some(Volume::new(40)),
            "the newer event value must survive the late fetch response"
        );
    }

    /// The guard must not reject legitimately newer writes — otherwise the
    /// store would freeze after its first write and the test above would pass
    /// for the wrong reason.
    #[test]
    fn test_newer_write_is_accepted_after_an_earlier_one() {
        let manager = StateManager::new().unwrap();
        manager
            .add_devices(vec![Device {
                id: "RINCON_FWD".to_string(),
                name: "Forward Test".to_string(),
                room_name: "Test".to_string(),
                ip_address: "192.0.2.12".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            }])
            .unwrap();

        let speaker_id = SpeakerId::new("RINCON_FWD");
        let early = Instant::now();

        assert_eq!(
            manager.set_property_stamped(
                &speaker_id,
                Volume::new(10),
                WriteStamp::observed_at(ChangeSource::Fetch, early),
            ),
            WriteOutcome::Changed
        );

        // A later fetch, correctly ordered, wins.
        assert_eq!(
            manager.set_property_stamped(
                &speaker_id,
                Volume::new(20),
                WriteStamp::now(ChangeSource::Fetch),
            ),
            WriteOutcome::Changed
        );
        assert_eq!(
            manager.get_property::<Volume>(&speaker_id),
            Some(Volume::new(20))
        );
    }
}
