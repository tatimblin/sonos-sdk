//! Generic PropertyHandle for DOM-like property access
//!
//! Provides a consistent pattern for accessing any property on a speaker:
//! - `get()` - Get cached value (instant, no network)
//! - `fetch()` - Fetch fresh value from device (blocking API call)
//! - `watch()` - Returns a `WatchHandle` that keeps the subscription alive

use std::fmt;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use sonos_api::operation::{ComposableOperation, UPnPOperation};
use sonos_api::{ServiceScope, SonosClient};
use sonos_event_manager::WatchGuard;
use sonos_state::{property::SonosProperty, ChangeSource, SpeakerId, StateManager, WriteStamp};

use crate::SdkError;

/// Shared context for all property handles on a speaker
///
/// This struct holds the common data needed by all PropertyHandles,
/// allowing them to share a single Arc instead of duplicating data.
#[derive(Clone)]
pub struct SpeakerContext {
    pub(crate) speaker_id: SpeakerId,
    pub(crate) speaker_ip: IpAddr,
    pub(crate) state_manager: Arc<StateManager>,
    pub(crate) api_client: SonosClient,
}

impl SpeakerContext {
    /// Create a new SpeakerContext
    pub fn new(
        speaker_id: SpeakerId,
        speaker_ip: IpAddr,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Arc<Self> {
        Arc::new(Self {
            speaker_id,
            speaker_ip,
            state_manager,
            api_client,
        })
    }
}

// ============================================================================
// Watch status types
// ============================================================================

/// How property updates will be delivered after calling `watch()`
///
/// This enum indicates the mechanism that will be used to receive property
/// updates. The SDK automatically selects the best available method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatchMode {
    /// UPnP event subscription is active - real-time updates will be received
    ///
    /// This is the preferred mode, providing immediate notifications when
    /// properties change on the device.
    Events,

    /// UPnP subscription failed, updates may come via polling fallback
    ///
    /// The event manager was configured but subscription failed (possibly due
    /// to firewall). The SDK's polling fallback may still provide updates,
    /// but they won't be real-time.
    Polling,

    /// No event manager configured - cache-only mode
    ///
    /// Properties will only update when explicitly fetched via `fetch()`.
    /// Call `system.configure_events()` to enable automatic updates.
    CacheOnly,
}

impl fmt::Display for WatchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WatchMode::Events => write!(f, "Events (real-time)"),
            WatchMode::Polling => write!(f, "Polling (fallback)"),
            WatchMode::CacheOnly => write!(f, "CacheOnly (no events)"),
        }
    }
}

/// RAII handle returned by `watch()`. Holds a subscription lease and reads the
/// property live from the state store. Dropping the handle starts the grace
/// period — the UPnP subscription persists for 50ms so it can be reacquired
/// cheaply on the next frame.
///
/// Not `Clone` — each handle is one subscription hold.
///
/// # The value is live, not a snapshot
///
/// [`Self::value`] reads the store on every call, so a handle acquired once and
/// held across many events keeps returning the *current* value. There is no need
/// to re-`watch()` to refresh it; a handle is a lease on the subscription, and
/// reading through it is the same read `get()` performs.
///
/// The value is therefore returned by clone rather than by reference: the store
/// sits behind an `RwLock` shared with the event worker, and handing out a
/// borrow into it would either hold that lock for the handle's whole lifetime or
/// alias a value the worker is free to replace. `P` is a small `Clone` property
/// (a `u8`, a `bool`, a few `String`s at worst), so the copy is cheaper than the
/// lock it would otherwise pin.
///
/// # Example
///
/// ```rust,ignore
/// // Watch returns a handle — hold it to keep the subscription alive
/// let volume = speaker.volume.watch()?;
///
/// if let Some(v) = volume.value() {
///     println!("Volume: {}%", v.value());
/// }
///
/// // Hold the same handle across events — value() re-reads each time
/// for _event in system.iter() {
///     println!("Volume now: {:?}", volume.value());
/// }
///
/// // Dropping the handle starts the 50ms grace period
/// drop(volume);
/// ```
#[must_use = "dropping the handle starts the grace period — hold it to keep the subscription alive"]
pub struct WatchHandle<P> {
    /// Reads the property from the store on demand.
    ///
    /// A closure rather than a `SpeakerContext`/`GroupContext` pair because the
    /// two `watch()` implementations read from different stores (`get_property`
    /// vs `get_group_property`) and resolve different keys. Capturing the read
    /// itself keeps `WatchHandle` unaware of which one it came from, so the two
    /// construction sites cannot drift into two different notions of "current".
    read: Box<dyn Fn() -> Option<P> + Send + Sync>,
    mode: WatchMode,
    _cleanup: WatchCleanup,
}

impl<P> WatchHandle<P> {
    /// Returns the watch mode (Events, Polling, or CacheOnly).
    pub fn mode(&self) -> WatchMode {
        self.mode
    }

    /// Returns the property's current value, read live from the state store.
    ///
    /// Costs one read-lock acquisition plus a clone of `P` — not free, unlike
    /// the frozen field this replaced, but it is the same cost as `get()` and it
    /// is what makes the handle a live view instead of a stale snapshot.
    ///
    /// Returns `None` if no value has been observed yet, or if the value has
    /// since become unreachable (a speaker that left the topology, or a
    /// `PerCoordinator` property whose group was dissolved).
    pub fn value(&self) -> Option<P> {
        (self.read)()
    }

    /// Returns true if a value is currently available from the store.
    ///
    /// Also a live read: this can go from `false` to `true` as the first event
    /// arrives, without the handle being re-acquired.
    pub fn has_value(&self) -> bool {
        self.value().is_some()
    }

    /// Returns true if real-time UPnP events are active.
    pub fn has_realtime_events(&self) -> bool {
        self.mode == WatchMode::Events
    }
}

impl<P: fmt::Debug> fmt::Debug for WatchHandle<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatchHandle")
            .field("value", &self.value())
            .field("mode", &self.mode)
            .finish()
    }
}

/// Internal cleanup strategy for WatchHandle.
///
/// - `Guard`: Event manager is active — WatchGuard handles the subscription
///   lifecycle (ref counting, grace period, unsubscribe).
/// - `CacheOnly`: No event manager — just unregisters from the watched set.
/// - `CoordinatorGuard`: PerCoordinator service routed to coordinator —
///   WatchGuard manages the coordinator's subscription, CacheOnlyGuard cleans
///   up the member's watched-set entry on drop.
///
/// Fields are never read — they exist solely for their Drop behavior.
#[allow(dead_code)]
enum WatchCleanup {
    Guard(WatchGuard),
    CacheOnly(CacheOnlyGuard),
    CoordinatorGuard {
        _guard: WatchGuard,
        _member_cleanup: CacheOnlyGuard,
    },
}

/// Cleanup guard for CacheOnly mode (no event manager).
///
/// Holds one reference on `(speaker_id, property_key)` in the state manager's
/// watched set and releases it on drop. Because the set is reference-counted,
/// dropping this guard only stops emission if no other watcher still holds the
/// same pair — several `WatchHandle`s for one property can coexist, and one
/// going away must not silence the others.
struct CacheOnlyGuard {
    state_manager: Arc<StateManager>,
    speaker_id: SpeakerId,
    property_key: &'static str,
}

impl Drop for CacheOnlyGuard {
    fn drop(&mut self) {
        // Releases exactly the one reference taken by the matching
        // `register_watch()` above.
        self.state_manager
            .unregister_watch(&self.speaker_id, self.property_key);
    }
}

/// Trait for properties that can be fetched from the device
///
/// This trait defines how to fetch a property value from a Sonos device.
/// Each property type that supports fetching must implement this trait.
///
/// # Type Parameters
///
/// - `Op`: The UPnP operation type used to fetch this property
///
/// # Example
///
/// ```rust,ignore
/// impl Fetchable for Volume {
///     type Operation = GetVolumeOperation;
///
///     fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
///         rendering_control::get_volume_operation("Master".to_string())
///             .build()
///             .map_err(|e| SdkError::FetchFailed(e.to_string()))
///     }
///
///     fn from_response(response: GetVolumeResponse) -> Self {
///         Volume::new(response.current_volume)
///     }
/// }
/// ```
pub trait Fetchable: SonosProperty {
    /// The UPnP operation type used to fetch this property
    type Operation: UPnPOperation;

    /// Build the operation to fetch this property
    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError>;

    /// Convert the operation response to the property value
    fn from_response(response: <Self::Operation as UPnPOperation>::Response) -> Self;
}

/// Trait for properties that require context (e.g., speaker_id) to interpret the response
///
/// Unlike `Fetchable`, the response contains data for multiple entities and
/// the correct one must be extracted using context.
pub trait FetchableWithContext: SonosProperty {
    /// The UPnP operation type used to fetch this property
    type Operation: UPnPOperation;

    /// Build the operation to fetch this property
    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError>;

    /// Convert the operation response to the property value using speaker context
    fn from_response_with_context(
        response: <Self::Operation as UPnPOperation>::Response,
        speaker_id: &SpeakerId,
    ) -> Option<Self>;
}

/// Generic property handle providing get/fetch/watch/unwatch pattern
///
/// This is the core abstraction for the DOM-like API. Each property on a Speaker
/// is accessed through a PropertyHandle that provides consistent methods for
/// reading cached values, fetching fresh values, and watching for changes.
///
/// # Type Parameter
///
/// - `P`: The property type, must implement `SonosProperty`
///
/// # Example
///
/// ```rust,ignore
/// // Get cached value (instant, no network call)
/// let volume = speaker.volume.get();
///
/// // Fetch fresh value from device (blocking API call)
/// let fresh_volume = speaker.volume.fetch()?;
///
/// // Watch for changes — hold the handle to keep the subscription alive
/// let handle = speaker.volume.watch()?;
/// println!("Volume: {:?}", handle.value());
/// // Dropping handle starts 50ms grace period
/// ```
#[derive(Clone)]
pub struct PropertyHandle<P: SonosProperty> {
    context: Arc<SpeakerContext>,
    _phantom: PhantomData<P>,
}

impl<P: SonosProperty> PropertyHandle<P> {
    /// Create a new PropertyHandle from a shared SpeakerContext
    pub fn new(context: Arc<SpeakerContext>) -> Self {
        Self {
            context,
            _phantom: PhantomData,
        }
    }

    /// Get cached property value (sync, instant, no network call)
    ///
    /// Returns the currently cached value for this property, or `None` if
    /// no value has been cached yet. This method never makes network calls.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(volume) = speaker.volume.get() {
    ///     println!("Current volume: {}%", volume.value());
    /// }
    /// ```
    #[must_use = "returns the cached property value"]
    pub fn get(&self) -> Option<P> {
        self.context
            .state_manager
            .get_property::<P>(&self.context.speaker_id)
    }

    /// Start watching this property for changes (sync)
    ///
    /// Returns a [`WatchHandle`] that keeps the subscription alive. Hold
    /// the handle for as long as you need updates — dropping it starts a
    /// 50ms grace period before the UPnP subscription is torn down.
    ///
    /// Acquire the handle **once** and keep it: [`WatchHandle::value`] reads the
    /// store on every call, so one handle held across a whole render loop reports
    /// every change. Re-watching per frame is not needed to refresh the value.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Acquire once, outside the loop — the handle is a live view
    /// let volume = speaker.volume.watch()?;
    ///
    /// if let Some(v) = volume.value() {
    ///     println!("Volume: {}%", v.value());
    /// }
    ///
    /// // Changes appear in system.iter() while the handle is alive, and the
    /// // same handle reports the new value.
    /// for _event in system.iter() {
    ///     println!("Volume: {:?}", volume.value());
    /// }
    /// ```
    pub fn watch(&self) -> Result<WatchHandle<P>, SdkError> {
        tracing::trace!(
            "watch() called for {:?} on {}",
            P::SERVICE,
            self.context.speaker_id.as_str()
        );

        // Trigger lazy event manager init if needed
        if self.context.state_manager.event_manager().is_none() {
            if let Some(init) = self.context.state_manager.event_init() {
                tracing::debug!(
                    "Event manager not initialized, triggering lazy init for {:?} on {}",
                    P::SERVICE,
                    self.context.speaker_id.as_str()
                );
                init().map_err(|e| SdkError::EventManager(e.to_string()))?;
            } else {
                tracing::debug!(
                    "No event_init closure available (test mode?) for {}",
                    self.context.speaker_id.as_str()
                );
            }
        }

        // Resolve subscription target: for PerCoordinator services, route to coordinator
        let (sub_id, sub_ip) = self.context.state_manager.resolve_subscription_target(
            &self.context.speaker_id,
            self.context.speaker_ip,
            P::SERVICE,
        );
        let routed_to_coordinator = sub_id != self.context.speaker_id;

        let (mode, cleanup) = if let Some(em) = self.context.state_manager.event_manager() {
            match em.acquire_watch(&sub_id, P::KEY, sub_ip, P::SERVICE) {
                Ok(guard) => {
                    if routed_to_coordinator {
                        // Register the member's watch for notification forwarding
                        self.context
                            .state_manager
                            .register_watch(&self.context.speaker_id, P::KEY);
                        (
                            WatchMode::Events,
                            WatchCleanup::CoordinatorGuard {
                                _guard: guard,
                                _member_cleanup: CacheOnlyGuard {
                                    state_manager: Arc::clone(&self.context.state_manager),
                                    speaker_id: self.context.speaker_id.clone(),
                                    property_key: P::KEY,
                                },
                            },
                        )
                    } else {
                        (WatchMode::Events, WatchCleanup::Guard(guard))
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to subscribe to {:?} for {}: {} - falling back to polling",
                        P::SERVICE,
                        self.context.speaker_id.as_str(),
                        e
                    );
                    // Register directly for polling fallback
                    self.context
                        .state_manager
                        .register_watch(&self.context.speaker_id, P::KEY);
                    (
                        WatchMode::Polling,
                        WatchCleanup::CacheOnly(CacheOnlyGuard {
                            state_manager: Arc::clone(&self.context.state_manager),
                            speaker_id: self.context.speaker_id.clone(),
                            property_key: P::KEY,
                        }),
                    )
                }
            }
        } else {
            // No event manager — cache-only mode
            tracing::warn!(
                "No event manager available for {} — falling back to cache-only mode",
                self.context.speaker_id.as_str()
            );
            self.context
                .state_manager
                .register_watch(&self.context.speaker_id, P::KEY);
            (
                WatchMode::CacheOnly,
                WatchCleanup::CacheOnly(CacheOnlyGuard {
                    state_manager: Arc::clone(&self.context.state_manager),
                    speaker_id: self.context.speaker_id.clone(),
                    property_key: P::KEY,
                }),
            )
        };

        tracing::debug!(
            "watch() resolved to {:?} for {} on {}",
            mode,
            P::KEY,
            self.context.speaker_id.as_str()
        );

        // The handle reads through a clone of the context rather than capturing a
        // value, so `value()` returns what the store holds *now*. It reads by the
        // same `get_property` path `get()` uses, which means it inherits
        // coordinator resolution and the write-ordering guard (§4.1a of the
        // sonos-state spec) for free: the store only ever holds the
        // newest-observed value, so a live read cannot resurrect a stale one.
        let context = Arc::clone(&self.context);
        Ok(WatchHandle {
            read: Box::new(move || context.state_manager.get_property::<P>(&context.speaker_id)),
            mode,
            _cleanup: cleanup,
        })
    }

    /// Check if this property is currently being watched
    ///
    /// Returns `true` while *any* `WatchHandle` for this property is alive, or
    /// during the grace period after the last handle was dropped. Watches are
    /// reference-counted, so dropping one of several handles leaves this `true`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let handle = speaker.volume.watch()?;
    /// assert!(speaker.volume.is_watched());
    ///
    /// drop(handle); // starts 50ms grace period
    /// // is_watched() remains true during grace period
    /// ```
    #[must_use = "returns whether the property is being watched"]
    pub fn is_watched(&self) -> bool {
        self.context
            .state_manager
            .is_watched(&self.context.speaker_id, P::KEY)
    }

    /// Get the speaker ID this handle is associated with
    pub fn speaker_id(&self) -> &SpeakerId {
        &self.context.speaker_id
    }

    /// Get the speaker IP address
    pub fn speaker_ip(&self) -> IpAddr {
        self.context.speaker_ip
    }
}

// ============================================================================
// Fetch implementation for Fetchable properties
// ============================================================================

impl<P: Fetchable> PropertyHandle<P> {
    /// Watch with lazy fetch: subscribes to events, and if the cache is empty,
    /// performs a one-time fetch to seed the value.
    ///
    /// Use this instead of `watch()` when you need a value on the first frame
    /// without waiting for a UPnP event to arrive.
    pub fn watch_or_fetch(&self) -> Result<WatchHandle<P>, SdkError> {
        let wh = self.watch()?;
        if !wh.has_value() {
            // `fetch()` writes the value into the store, and the handle reads the
            // store live, so there is nothing to patch onto the handle — unlike
            // when the handle carried a frozen snapshot. A stale-rejected write
            // is fine here too: it means an event already delivered something
            // newer, which is exactly what `value()` will then return.
            if let Err(e) = self.fetch() {
                tracing::warn!("watch_or_fetch: fetch failed for {}: {e}", P::KEY);
            }
        }
        Ok(wh)
    }

    /// Fetch fresh value from device + update cache (sync)
    ///
    /// This makes a synchronous UPnP call to the device and updates
    /// the local state cache with the result.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Fetch fresh volume from device
    /// let volume = speaker.volume.fetch()?;
    /// println!("Current volume: {}%", volume.value());
    ///
    /// // The cache is now updated, so get() returns the same value
    /// assert_eq!(speaker.volume.get(), Some(volume));
    /// ```
    #[must_use = "returns the fetched value from the device"]
    pub fn fetch(&self) -> Result<P, SdkError> {
        let operation = P::build_operation()?;

        // Resolve target: coordinator for PerCoordinator services, fresh IP for PerSpeaker
        let (target_id, target_ip) = if P::SERVICE.scope() == ServiceScope::PerCoordinator {
            self.context.state_manager.resolve_subscription_target(
                &self.context.speaker_id,
                self.context.speaker_ip,
                P::SERVICE,
            )
        } else {
            let current_ip = self
                .context
                .state_manager
                .get_speaker_ip(&self.context.speaker_id)
                .unwrap_or(self.context.speaker_ip);
            (self.context.speaker_id.clone(), current_ip)
        };

        // Stamped *before* the request, not after. The device's answer describes
        // it as of this instant; by the time the response lands, an event may
        // have already delivered a newer value, and the store must keep that one.
        // Stamping after the round trip would make this stale read look like the
        // freshest write and clobber it.
        let observed_at = Instant::now();

        let response = self
            .context
            .api_client
            .execute_enhanced(&target_ip.to_string(), operation)
            .map_err(SdkError::ApiError)?;

        let property_value = P::from_response(response);

        // Store under target_id (coordinator for PerCoordinator, self for PerSpeaker).
        // May be rejected as stale, which is correct — the caller still gets the
        // value it fetched, it just does not overwrite a newer one in the cache.
        self.context.state_manager.set_property_stamped(
            &target_id,
            property_value.clone(),
            WriteStamp::observed_at(ChangeSource::Fetch, observed_at),
        );

        Ok(property_value)
    }
}

// ============================================================================
// Concrete fetch for FetchableWithContext properties
// ============================================================================
//
// Rust does not allow two generic impl blocks (Fetchable + FetchableWithContext)
// defining the same `fetch()` method, so context-dependent properties get a
// concrete impl instead.

impl PropertyHandle<GroupMembership> {
    /// Fetch fresh value from device using speaker context + update cache (sync)
    ///
    /// The response is interpreted using the speaker_id to extract the relevant
    /// property value from the full topology response.
    #[must_use = "returns the fetched value from the device"]
    pub fn fetch(&self) -> Result<GroupMembership, SdkError> {
        let operation = <GroupMembership as FetchableWithContext>::build_operation()?;

        // Stamped before the request — see `Fetchable::fetch`.
        let observed_at = Instant::now();

        let response = self
            .context
            .api_client
            .execute_enhanced(&self.context.speaker_ip.to_string(), operation)
            .map_err(SdkError::ApiError)?;

        let property_value =
            GroupMembership::from_response_with_context(response, &self.context.speaker_id)
                .ok_or_else(|| {
                    SdkError::FetchFailed(format!(
                        "Speaker {} not found in topology response",
                        self.context.speaker_id.as_str()
                    ))
                })?;

        self.context.state_manager.set_property_stamped(
            &self.context.speaker_id,
            property_value.clone(),
            WriteStamp::observed_at(ChangeSource::Fetch, observed_at),
        );

        Ok(property_value)
    }
}

// ============================================================================
// Type aliases for common property handles
// ============================================================================

use sonos_api::services::{
    av_transport::{
        self, GetPositionInfoOperation, GetPositionInfoResponse, GetTransportInfoOperation,
        GetTransportInfoResponse,
    },
    group_rendering_control::{
        self, GetGroupMuteOperation, GetGroupMuteResponse, GetGroupVolumeOperation,
        GetGroupVolumeResponse,
    },
    rendering_control::{
        self, GetBassOperation, GetBassResponse, GetLoudnessOperation, GetLoudnessResponse,
        GetMuteOperation, GetMuteResponse, GetTrebleOperation, GetTrebleResponse,
        GetVolumeOperation, GetVolumeResponse,
    },
    zone_group_topology::{self, GetZoneGroupStateOperation, GetZoneGroupStateResponse},
};
use sonos_state::{
    Bass, CurrentTrack, GroupId, GroupMembership, GroupMute, GroupVolume, GroupVolumeChangeable,
    Loudness, Mute, PlaybackState, Position, Treble, Volume,
};

// ============================================================================
// Helper functions
// ============================================================================

/// Helper to create consistent error messages for operation build failures
fn build_error<E: std::fmt::Display>(operation_name: &str, e: E) -> SdkError {
    SdkError::FetchFailed(format!("Failed to build {operation_name} operation: {e}"))
}

// ============================================================================
// Fetchable implementations
// ============================================================================

impl Fetchable for Volume {
    type Operation = GetVolumeOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        rendering_control::get_volume_operation("Master".to_string())
            .build()
            .map_err(|e| build_error("GetVolume", e))
    }

    fn from_response(response: GetVolumeResponse) -> Self {
        Volume::new(response.current_volume)
    }
}

impl Fetchable for PlaybackState {
    type Operation = GetTransportInfoOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        av_transport::get_transport_info_operation()
            .build()
            .map_err(|e| build_error("GetTransportInfo", e))
    }

    fn from_response(response: GetTransportInfoResponse) -> Self {
        match response.current_transport_state.as_str() {
            "PLAYING" => PlaybackState::Playing,
            "PAUSED" | "PAUSED_PLAYBACK" => PlaybackState::Paused,
            "STOPPED" => PlaybackState::Stopped,
            _ => PlaybackState::Transitioning,
        }
    }
}

impl Fetchable for Position {
    type Operation = GetPositionInfoOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        av_transport::get_position_info_operation()
            .build()
            .map_err(|e| build_error("GetPositionInfo", e))
    }

    fn from_response(response: GetPositionInfoResponse) -> Self {
        let position_ms = Position::parse_time_to_ms(&response.rel_time).unwrap_or(0);
        let duration_ms = Position::parse_time_to_ms(&response.track_duration).unwrap_or(0);
        Position::new(position_ms, duration_ms)
    }
}

impl Fetchable for Mute {
    type Operation = GetMuteOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        rendering_control::get_mute_operation("Master".to_string())
            .build()
            .map_err(|e| build_error("GetMute", e))
    }

    fn from_response(response: GetMuteResponse) -> Self {
        Mute::new(response.current_mute)
    }
}

impl Fetchable for Bass {
    type Operation = GetBassOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        rendering_control::get_bass_operation()
            .build()
            .map_err(|e| build_error("GetBass", e))
    }

    fn from_response(response: GetBassResponse) -> Self {
        Bass::new(response.current_bass)
    }
}

impl Fetchable for Treble {
    type Operation = GetTrebleOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        rendering_control::get_treble_operation()
            .build()
            .map_err(|e| build_error("GetTreble", e))
    }

    fn from_response(response: GetTrebleResponse) -> Self {
        Treble::new(response.current_treble)
    }
}

impl Fetchable for Loudness {
    type Operation = GetLoudnessOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        rendering_control::get_loudness_operation("Master".to_string())
            .build()
            .map_err(|e| build_error("GetLoudness", e))
    }

    fn from_response(response: GetLoudnessResponse) -> Self {
        Loudness::new(response.current_loudness)
    }
}

impl Fetchable for CurrentTrack {
    type Operation = GetPositionInfoOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        av_transport::get_position_info_operation()
            .build()
            .map_err(|e| build_error("GetPositionInfo", e))
    }

    fn from_response(response: GetPositionInfoResponse) -> Self {
        let metadata = if response.track_meta_data.is_empty()
            || response.track_meta_data == "NOT_IMPLEMENTED"
        {
            None
        } else {
            Some(response.track_meta_data.as_str())
        };
        let (title, artist, album, album_art_uri) = sonos_state::parse_track_metadata(metadata);
        CurrentTrack {
            title,
            artist,
            album,
            album_art_uri,
            uri: Some(response.track_uri).filter(|s| !s.is_empty()),
        }
    }
}

// ============================================================================
// FetchableWithContext implementations
// ============================================================================

impl FetchableWithContext for GroupMembership {
    type Operation = GetZoneGroupStateOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        zone_group_topology::get_zone_group_state_operation()
            .build()
            .map_err(|e| build_error("GetZoneGroupState", e))
    }

    fn from_response_with_context(
        response: GetZoneGroupStateResponse,
        speaker_id: &SpeakerId,
    ) -> Option<Self> {
        let zone_groups =
            zone_group_topology::parse_zone_group_state_xml(&response.zone_group_state).ok()?;

        for group in &zone_groups {
            let is_member = group.members.iter().any(|m| m.uuid == speaker_id.as_str());
            if is_member {
                let is_coordinator = group.coordinator == speaker_id.as_str();
                return Some(GroupMembership::new(
                    GroupId::new(&group.id),
                    is_coordinator,
                ));
            }
        }

        None
    }
}

// ============================================================================
// Event-only properties (no dedicated UPnP Get operation)
// ============================================================================
//
// GroupVolumeChangeable is the only remaining event-only property — there is
// no GetGroupVolumeChangeable operation in the Sonos UPnP API. Its value
// is obtained exclusively from GroupRenderingControl events.
//
// All other properties now have fetch() via Fetchable, FetchableWithContext,
// or GroupFetchable trait implementations.

// ============================================================================
// Type aliases
// ============================================================================

/// Handle for speaker volume (0-100)
pub type VolumeHandle = PropertyHandle<Volume>;

/// Handle for playback state (Playing/Paused/Stopped)
pub type PlaybackStateHandle = PropertyHandle<PlaybackState>;

/// Handle for mute state
pub type MuteHandle = PropertyHandle<Mute>;

/// Handle for bass EQ setting (-10 to +10)
pub type BassHandle = PropertyHandle<Bass>;

/// Handle for treble EQ setting (-10 to +10)
pub type TrebleHandle = PropertyHandle<Treble>;

/// Handle for loudness compensation setting
pub type LoudnessHandle = PropertyHandle<Loudness>;

/// Handle for current playback position
pub type PositionHandle = PropertyHandle<Position>;

/// Handle for current track information
pub type CurrentTrackHandle = PropertyHandle<CurrentTrack>;

/// Handle for group membership information
pub type GroupMembershipHandle = PropertyHandle<GroupMembership>;

// ============================================================================
// Group Property Handles
// ============================================================================

/// Shared context for all property handles on a group
///
/// Analogous to `SpeakerContext` but scoped to a group. Operations are
/// executed against the group's coordinator speaker.
#[derive(Clone)]
pub struct GroupContext {
    pub(crate) group_id: GroupId,
    pub(crate) coordinator_id: SpeakerId,
    pub(crate) coordinator_ip: IpAddr,
    pub(crate) state_manager: Arc<StateManager>,
    pub(crate) api_client: SonosClient,
}

impl GroupContext {
    /// Create a new GroupContext
    pub fn new(
        group_id: GroupId,
        coordinator_id: SpeakerId,
        coordinator_ip: IpAddr,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Arc<Self> {
        Arc::new(Self {
            group_id,
            coordinator_id,
            coordinator_ip,
            state_manager,
            api_client,
        })
    }
}

/// Generic property handle for group-scoped properties
///
/// Provides the same get/fetch/watch/unwatch pattern as `PropertyHandle`,
/// but reads from the group property store and executes API calls against
/// the group's coordinator.
#[derive(Clone)]
pub struct GroupPropertyHandle<P: SonosProperty> {
    context: Arc<GroupContext>,
    _phantom: PhantomData<P>,
}

impl<P: SonosProperty> GroupPropertyHandle<P> {
    /// Create a new GroupPropertyHandle from a shared GroupContext
    pub fn new(context: Arc<GroupContext>) -> Self {
        Self {
            context,
            _phantom: PhantomData,
        }
    }

    /// Get cached group property value (sync, instant, no network call)
    #[must_use = "returns the cached property value"]
    pub fn get(&self) -> Option<P> {
        self.context
            .state_manager
            .get_group_property::<P>(&self.context.group_id)
    }

    /// Start watching this group property for changes (sync)
    ///
    /// Returns a [`WatchHandle`] scoped to the group coordinator.
    /// Hold the handle to keep the subscription alive.
    pub fn watch(&self) -> Result<WatchHandle<P>, SdkError> {
        // Trigger lazy event manager init if needed
        if self.context.state_manager.event_manager().is_none() {
            if let Some(init) = self.context.state_manager.event_init() {
                tracing::debug!(
                    "Event manager not initialized, triggering lazy init for group {:?} on {}",
                    P::SERVICE,
                    self.context.group_id.as_str()
                );
                init().map_err(|e| SdkError::EventManager(e.to_string()))?;
            } else {
                tracing::debug!(
                    "No event_init closure available (test mode?) for group {}",
                    self.context.group_id.as_str()
                );
            }
        }

        let (mode, cleanup) = if let Some(em) = self.context.state_manager.event_manager() {
            match em.acquire_watch(
                &self.context.coordinator_id,
                P::KEY,
                self.context.coordinator_ip,
                P::SERVICE,
            ) {
                Ok(guard) => (WatchMode::Events, WatchCleanup::Guard(guard)),
                Err(e) => {
                    tracing::warn!(
                        "Failed to subscribe to {:?} for group {}: {} - falling back to polling",
                        P::SERVICE,
                        self.context.group_id.as_str(),
                        e
                    );
                    self.context
                        .state_manager
                        .register_watch(&self.context.coordinator_id, P::KEY);
                    (
                        WatchMode::Polling,
                        WatchCleanup::CacheOnly(CacheOnlyGuard {
                            state_manager: Arc::clone(&self.context.state_manager),
                            speaker_id: self.context.coordinator_id.clone(),
                            property_key: P::KEY,
                        }),
                    )
                }
            }
        } else {
            self.context
                .state_manager
                .register_watch(&self.context.coordinator_id, P::KEY);
            (
                WatchMode::CacheOnly,
                WatchCleanup::CacheOnly(CacheOnlyGuard {
                    state_manager: Arc::clone(&self.context.state_manager),
                    speaker_id: self.context.coordinator_id.clone(),
                    property_key: P::KEY,
                }),
            )
        };

        // Live read against the group store — see the speaker `watch()` above.
        let context = Arc::clone(&self.context);
        Ok(WatchHandle {
            read: Box::new(move || {
                context
                    .state_manager
                    .get_group_property::<P>(&context.group_id)
            }),
            mode,
            _cleanup: cleanup,
        })
    }

    /// Check if this group property is currently being watched
    #[must_use = "returns whether the property is being watched"]
    pub fn is_watched(&self) -> bool {
        self.context
            .state_manager
            .is_watched(&self.context.coordinator_id, P::KEY)
    }

    /// Get the group ID this handle is associated with
    pub fn group_id(&self) -> &GroupId {
        &self.context.group_id
    }
}

/// Trait for group properties that can be fetched from the coordinator
pub trait GroupFetchable: SonosProperty {
    /// The UPnP operation type used to fetch this property
    type Operation: UPnPOperation;

    /// Build the operation to fetch this property
    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError>;

    /// Convert the operation response to the property value
    fn from_response(response: <Self::Operation as UPnPOperation>::Response) -> Self;
}

impl<P: GroupFetchable> GroupPropertyHandle<P> {
    /// Watch with lazy fetch: subscribes to events, and if the cache is empty,
    /// performs a one-time fetch from the coordinator to seed the value.
    pub fn watch_or_fetch(&self) -> Result<WatchHandle<P>, SdkError> {
        let wh = self.watch()?;
        if !wh.has_value() {
            // See `PropertyHandle::watch_or_fetch` — the fetch lands in the store
            // the handle reads from, so no patching is needed.
            if let Err(e) = self.fetch() {
                tracing::warn!(
                    "watch_or_fetch: fetch failed for group {} {}: {e}",
                    self.context.group_id.as_str(),
                    P::KEY
                );
            }
        }
        Ok(wh)
    }

    /// Fetch fresh value from coordinator + update group cache (sync)
    #[must_use = "returns the fetched value from the device"]
    pub fn fetch(&self) -> Result<P, SdkError> {
        let operation = P::build_operation()?;

        // Stamped before the request — see `Fetchable::fetch`.
        let observed_at = Instant::now();

        let response = self
            .context
            .api_client
            .execute_enhanced(&self.context.coordinator_ip.to_string(), operation)
            .map_err(SdkError::ApiError)?;

        let property_value = P::from_response(response);

        self.context.state_manager.set_group_property_stamped(
            &self.context.group_id,
            property_value.clone(),
            WriteStamp::observed_at(ChangeSource::Fetch, observed_at),
        );

        Ok(property_value)
    }
}

// ============================================================================
// GroupFetchable implementations
// ============================================================================

impl GroupFetchable for GroupVolume {
    type Operation = GetGroupVolumeOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        group_rendering_control::get_group_volume()
            .build()
            .map_err(|e| build_error("GetGroupVolume", e))
    }

    fn from_response(response: GetGroupVolumeResponse) -> Self {
        GroupVolume::new(response.current_volume)
    }
}

impl GroupFetchable for GroupMute {
    type Operation = GetGroupMuteOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        group_rendering_control::get_group_mute()
            .build()
            .map_err(|e| build_error("GetGroupMute", e))
    }

    fn from_response(response: GetGroupMuteResponse) -> Self {
        GroupMute::new(response.current_mute)
    }
}

// ============================================================================
// Group type aliases
// ============================================================================

/// Handle for group volume (0-100)
pub type GroupVolumeHandle = GroupPropertyHandle<GroupVolume>;

/// Handle for group mute state
pub type GroupMuteHandle = GroupPropertyHandle<GroupMute>;

/// Handle for group volume changeable flag (event-only, no fetch)
pub type GroupVolumeChangeableHandle = GroupPropertyHandle<GroupVolumeChangeable>;

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_discovery::Device;
    use sonos_state::Property;

    fn create_test_state_manager() -> Arc<StateManager> {
        let manager = StateManager::new().unwrap();
        let devices = vec![Device {
            id: "RINCON_TEST123".to_string(),
            name: "Test Speaker".to_string(),
            room_name: "Test Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        manager.add_devices(devices).unwrap();
        Arc::new(manager)
    }

    fn create_test_context(state_manager: Arc<StateManager>) -> Arc<SpeakerContext> {
        SpeakerContext::new(
            SpeakerId::new("RINCON_TEST123"),
            "192.168.1.100".parse().unwrap(),
            state_manager,
            SonosClient::new(),
        )
    }

    #[test]
    fn test_property_handle_creation() {
        let state_manager = create_test_state_manager();
        let context = create_test_context(state_manager);
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();

        let handle: VolumeHandle = PropertyHandle::new(context);

        assert_eq!(handle.speaker_id().as_str(), "RINCON_TEST123");
        assert_eq!(handle.speaker_ip(), speaker_ip);
    }

    #[test]
    fn test_get_returns_none_initially() {
        let state_manager = create_test_state_manager();
        let context = create_test_context(state_manager);

        let handle: VolumeHandle = PropertyHandle::new(context);

        assert!(handle.get().is_none());
    }

    #[test]
    fn test_get_returns_cached_value() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(75));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        assert_eq!(handle.get(), Some(Volume::new(75)));
    }

    #[test]
    fn test_watch_registers_property() {
        let state_manager = create_test_state_manager();
        let context = create_test_context(Arc::clone(&state_manager));

        let handle: VolumeHandle = PropertyHandle::new(context);

        assert!(!handle.is_watched());
        let _wh = handle.watch().unwrap();
        assert!(handle.is_watched());
    }

    #[test]
    fn test_drop_watch_handle_unregisters_property() {
        let state_manager = create_test_state_manager();
        let context = create_test_context(Arc::clone(&state_manager));

        let handle: VolumeHandle = PropertyHandle::new(context);

        let wh = handle.watch().unwrap();
        assert!(handle.is_watched());

        drop(wh);
        assert!(!handle.is_watched());
    }

    /// Dropping one `WatchHandle` must not silence a sibling property.
    ///
    /// Volume and Mute both belong to RenderingControl, so they shared a
    /// subscription and shared one `(ip, service)` ref count. With a set-valued
    /// watched map the *first* drop removed the only entry, so the surviving
    /// handle went silent while the caller still held it — `is_watched()` said
    /// `false` and `system.iter()` stopped reporting the property.
    ///
    /// Overlapping holds no longer come from re-watching per frame (handles read
    /// live now, so nothing needs to), but they still arise wherever two
    /// independent watchers want the same property — which is the case this
    /// guards.
    ///
    /// Asserts delivery, not just the flag: a watch that is "registered" but no
    /// longer emits is the failure users would actually see.
    #[test]
    fn test_dropping_one_of_two_handles_keeps_property_emitting() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let context = create_test_context(Arc::clone(&state_manager));

        let volume: VolumeHandle = PropertyHandle::new(Arc::clone(&context));
        let mute: MuteHandle = PropertyHandle::new(context);

        let first = volume.watch().unwrap();
        let second = volume.watch().unwrap();
        // A sibling property of the same service, held once.
        let _mute_watch = mute.watch().unwrap();
        assert!(volume.is_watched());
        assert!(mute.is_watched());

        drop(first);

        assert!(
            volume.is_watched(),
            "one of two Volume handles dropped — the property must stay watched"
        );
        assert!(
            mute.is_watched(),
            "releasing a Volume handle must not disturb its RenderingControl sibling"
        );

        // The surviving handle must still receive events, not merely be flagged.
        state_manager.set_property(&speaker_id, Volume::new(11));
        state_manager.set_property(&speaker_id, Mute::new(true));

        let iter = state_manager.iter();
        let first_event = iter
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("Volume is still held by `second` and must still emit");
        assert_eq!(first_event.property_key(), Volume::KEY);
        let second_event = iter
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("Mute is still held and must still emit");
        assert_eq!(second_event.property_key(), Mute::KEY);

        // Last holder goes away: now it really stops.
        drop(second);
        assert!(!volume.is_watched());
        state_manager.set_property(&speaker_id, Volume::new(22));
        assert!(
            iter.recv_timeout(std::time::Duration::from_millis(50))
                .is_none(),
            "with every Volume handle dropped the property must stop emitting"
        );
    }

    /// A watcher reading through the SDK sees the value on the event, and sees
    /// it attributed to the write that caused it.
    ///
    /// Covers the SDK re-export boundary: `ChangeEvent.change` / `.source` must
    /// be reachable and correct from `sonos_sdk`, not just inside `sonos-state`.
    #[test]
    fn test_sdk_change_event_carries_value_and_source() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);
        let _wh = handle.watch().unwrap();

        let iter = state_manager.iter();
        state_manager.set_property(&speaker_id, Volume::new(37));

        let event = iter
            .recv_timeout(std::time::Duration::from_millis(100))
            .expect("a watched property write must emit");

        assert!(
            matches!(
                event.change,
                sonos_state::PropertyChange::Volume(Volume(37))
            ),
            "the event must carry the written value, got {:?}",
            event.change
        );
        assert_eq!(
            event.source,
            ChangeSource::LocalAction,
            "`set_property` is a local write, not a device report"
        );
    }

    /// A handle acquired *before* a change reports the new value afterwards,
    /// with no re-`watch()`.
    ///
    /// This is the defect PR-11b exists to fix: `WatchHandle` used to capture
    /// `self.get()` into a field at construction, so a handle held across a whole
    /// render loop kept reporting whatever the store happened to hold at the
    /// instant `watch()` ran. The only workaround was to re-acquire a handle
    /// every frame — which the docs recommended, and which is what made the
    /// refcount bug in PR #93 reachable in the first place.
    ///
    /// Asserts the *sequence*, not just the endpoint: two successive writes must
    /// both be visible through the one handle, so a fix that merely refreshed
    /// once cannot pass.
    #[test]
    fn test_handle_held_across_change_reports_new_value() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(10));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        // Acquired once, before either change, and never re-acquired.
        let wh = handle.watch().unwrap();
        assert_eq!(wh.value(), Some(Volume::new(10)));

        state_manager.set_property(&speaker_id, Volume::new(42));
        assert_eq!(
            wh.value(),
            Some(Volume::new(42)),
            "the handle froze its value at creation — a held handle must read live"
        );

        state_manager.set_property(&speaker_id, Volume::new(43));
        assert_eq!(
            wh.value(),
            Some(Volume::new(43)),
            "the handle must keep tracking, not refresh once"
        );
    }

    /// `has_value()` is a live read too: a handle acquired on an empty store must
    /// start reporting a value once one arrives, without being re-acquired.
    ///
    /// Separate from the test above because the `None → Some` transition is the
    /// case a snapshot gets *most* wrong — a handle acquired before the first
    /// event stayed permanently empty, so a dashboard that watched at startup
    /// rendered "—" forever.
    #[test]
    fn test_handle_acquired_before_first_value_becomes_populated() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        let wh = handle.watch().unwrap();
        assert!(!wh.has_value(), "nothing has been observed yet");
        assert_eq!(wh.value(), None);

        state_manager.set_property(&speaker_id, Volume::new(7));

        assert!(
            wh.has_value(),
            "has_value() froze at creation — it must reflect the store"
        );
        assert_eq!(wh.value(), Some(Volume::new(7)));
    }

    /// Every handle on the same property sees the update, and dropping one
    /// leaves the survivors both *live* and *receiving*.
    ///
    /// Combines the multi-handle and drop-a-sibling cases deliberately: the
    /// interesting failure is the interaction — a live read that works only while
    /// no handle has been dropped would pass them separately. Guards PR #93's
    /// refcount fix (delivery through `iter()`) alongside the new live read.
    #[test]
    fn test_all_handles_see_update_and_survive_a_sibling_drop() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let context = create_test_context(Arc::clone(&state_manager));

        let handle: VolumeHandle = PropertyHandle::new(Arc::clone(&context));

        let first = handle.watch().unwrap();
        let second = handle.watch().unwrap();
        let third = handle.watch().unwrap();

        let iter = state_manager.iter();
        state_manager.set_property(&speaker_id, Volume::new(31));

        // All three read the same new value.
        assert_eq!(first.value(), Some(Volume::new(31)));
        assert_eq!(second.value(), Some(Volume::new(31)));
        assert_eq!(third.value(), Some(Volume::new(31)));
        assert!(iter
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_some());

        drop(first);
        assert!(handle.is_watched(), "two handles still hold the property");

        // Survivors keep reading live *and* keep receiving.
        state_manager.set_property(&speaker_id, Volume::new(32));
        assert_eq!(
            second.value(),
            Some(Volume::new(32)),
            "a sibling handle dropping must not freeze the survivors"
        );
        assert_eq!(third.value(), Some(Volume::new(32)));
        assert!(
            iter.recv_timeout(std::time::Duration::from_millis(100))
                .is_some(),
            "the property is still held and must still emit"
        );

        drop(second);
        state_manager.set_property(&speaker_id, Volume::new(33));
        assert_eq!(
            third.value(),
            Some(Volume::new(33)),
            "the last handle must still read live"
        );
    }

    /// Reading through a handle whose value has become unreachable yields `None`,
    /// not a stale value resurrected from the handle's own memory.
    ///
    /// The edge case the live-read design introduces. `PlaybackState` comes from
    /// `AVTransport`, a `PerCoordinator` service, so `get_property` resolves it
    /// through the speaker's *coordinator*. Regroup the speaker under a
    /// coordinator that holds no `PlaybackState` and the correct answer becomes
    /// "unknown" — a snapshot handle would confidently report the old
    /// coordinator's value instead.
    ///
    /// The two speaker IDs are written explicitly rather than taken from
    /// `with_speakers`' fixed `RINCON_{i:03}` pattern, so the assertion depends on
    /// the regrouping rather than on either ID's spelling.
    #[test]
    fn test_handle_reads_none_when_value_becomes_unreachable() {
        let manager = StateManager::new().unwrap();
        manager
            .add_devices(vec![
                Device {
                    id: "RINCON_MEMBER".to_string(),
                    name: "Member".to_string(),
                    room_name: "Member".to_string(),
                    ip_address: "203.0.113.1".to_string(),
                    port: 1400,
                    model_name: "Sonos One".to_string(),
                },
                Device {
                    id: "RINCON_NEWCOORD".to_string(),
                    name: "New Coordinator".to_string(),
                    room_name: "New Coordinator".to_string(),
                    ip_address: "203.0.113.2".to_string(),
                    port: 1400,
                    model_name: "Sonos One".to_string(),
                },
            ])
            .unwrap();
        let state_manager = Arc::new(manager);

        let member = SpeakerId::new("RINCON_MEMBER");
        let new_coord = SpeakerId::new("RINCON_NEWCOORD");

        // The member is its own coordinator and knows it is playing.
        state_manager.set_property(&member, PlaybackState::Playing);

        let context = SpeakerContext::new(
            member.clone(),
            "203.0.113.1".parse().unwrap(),
            Arc::clone(&state_manager),
            SonosClient::new(),
        );
        let handle: PlaybackStateHandle = PropertyHandle::new(context);
        let wh = handle.watch().unwrap();
        assert_eq!(wh.value(), Some(PlaybackState::Playing));

        // Regroup: the member now follows a coordinator with no PlaybackState.
        state_manager.initialize(sonos_state::Topology {
            speakers: vec![],
            groups: vec![sonos_state::GroupInfo::new(
                GroupId::new("RINCON_NEWCOORD:1"),
                new_coord.clone(),
                vec![new_coord.clone(), member.clone()],
            )],
        });

        assert_eq!(
            wh.value(),
            None,
            "the coordinator holds no PlaybackState, so the answer is unknown — \
             a handle must not report the value it captured at creation"
        );

        // And it picks the new coordinator's value up when there is one.
        state_manager.set_property(&new_coord, PlaybackState::Paused);
        assert_eq!(wh.value(), Some(PlaybackState::Paused));
    }

    #[test]
    fn test_watch_returns_current_value() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(50));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        let wh = handle.watch().unwrap();
        assert_eq!(wh.value(), Some(Volume::new(50)));
        // No event manager configured, so should be CacheOnly mode
        assert_eq!(wh.mode(), WatchMode::CacheOnly);
    }

    #[test]
    fn test_watch_handle_accessors() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(75));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        let wh = handle.watch().unwrap();
        assert!(wh.has_value());
        assert!(!wh.has_realtime_events());
        assert_eq!(wh.value().map(|v| v.value()), Some(75));
    }

    #[test]
    fn test_property_handle_clone() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(60));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        let cloned = handle.clone();

        assert_eq!(handle.get(), cloned.get());
        assert_eq!(handle.get(), Some(Volume::new(60)));
    }

    // ========================================================================
    // Group property handle tests
    // ========================================================================

    fn create_test_group_context(state_manager: Arc<StateManager>) -> Arc<GroupContext> {
        GroupContext::new(
            GroupId::new("RINCON_TEST123:1"),
            SpeakerId::new("RINCON_TEST123"),
            "192.168.1.100".parse().unwrap(),
            state_manager,
            SonosClient::new(),
        )
    }

    #[test]
    fn test_group_property_handle_get_returns_none_initially() {
        let state_manager = create_test_state_manager();
        let context = create_test_group_context(state_manager);

        let handle: GroupVolumeHandle = GroupPropertyHandle::new(context);

        assert!(handle.get().is_none());
    }

    #[test]
    fn test_group_property_handle_get_returns_cached_value() {
        let state_manager = create_test_state_manager();
        let group_id = GroupId::new("RINCON_TEST123:1");

        // Store a group property value
        state_manager.set_group_property(&group_id, GroupVolume::new(65));

        let context = create_test_group_context(Arc::clone(&state_manager));
        let handle: GroupVolumeHandle = GroupPropertyHandle::new(context);

        assert_eq!(handle.get(), Some(GroupVolume::new(65)));
    }

    #[test]
    fn test_group_property_handle_watch_and_drop() {
        let state_manager = create_test_state_manager();
        let context = create_test_group_context(Arc::clone(&state_manager));

        let handle: GroupVolumeHandle = GroupPropertyHandle::new(context);

        assert!(!handle.is_watched());
        let wh = handle.watch().unwrap();
        assert!(handle.is_watched());

        drop(wh);
        assert!(!handle.is_watched());
    }

    /// The group `watch()` is a second, independent construction site — it reads
    /// the *group* store rather than the speaker store, so the speaker tests above
    /// say nothing about it. Without this, reverting only the group site left the
    /// whole suite green.
    #[test]
    fn test_group_handle_held_across_change_reports_new_value() {
        let state_manager = create_test_state_manager();
        let group_id = GroupId::new("RINCON_TEST123:1");
        let context = create_test_group_context(Arc::clone(&state_manager));

        let handle: GroupVolumeHandle = GroupPropertyHandle::new(context);

        let wh = handle.watch().unwrap();
        assert!(!wh.has_value(), "the group store is empty at this point");

        state_manager.set_group_property(&group_id, GroupVolume::new(20));
        assert_eq!(
            wh.value(),
            Some(GroupVolume::new(20)),
            "a group handle must read the group store live, not a snapshot"
        );

        state_manager.set_group_property(&group_id, GroupVolume::new(21));
        assert_eq!(wh.value(), Some(GroupVolume::new(21)));
    }

    #[test]
    fn test_group_property_handle_group_id() {
        let state_manager = create_test_state_manager();
        let context = create_test_group_context(state_manager);

        let handle: GroupVolumeHandle = GroupPropertyHandle::new(context);

        assert_eq!(handle.group_id().as_str(), "RINCON_TEST123:1");
    }

    #[test]
    fn test_group_mute_handle_accessible() {
        let state_manager = create_test_state_manager();
        let context = create_test_group_context(state_manager);

        let handle: GroupMuteHandle = GroupPropertyHandle::new(context);

        assert!(handle.get().is_none());
        assert_eq!(handle.group_id().as_str(), "RINCON_TEST123:1");
    }

    #[test]
    fn test_group_volume_changeable_handle_accessible() {
        let state_manager = create_test_state_manager();
        let context = create_test_group_context(state_manager);

        let handle: GroupVolumeChangeableHandle = GroupPropertyHandle::new(context);

        assert!(handle.get().is_none());
        assert_eq!(handle.group_id().as_str(), "RINCON_TEST123:1");
    }

    // ========================================================================
    // Trait implementation assertions
    // ========================================================================

    #[test]
    fn test_fetchable_impls_exist() {
        fn assert_fetchable<T: Fetchable>() {}
        assert_fetchable::<Volume>();
        assert_fetchable::<PlaybackState>();
        assert_fetchable::<Position>();
        assert_fetchable::<Mute>();
        assert_fetchable::<Bass>();
        assert_fetchable::<Treble>();
        assert_fetchable::<Loudness>();
        assert_fetchable::<CurrentTrack>();
    }

    #[test]
    fn test_fetchable_with_context_impls_exist() {
        fn assert_fetchable_with_context<T: FetchableWithContext>() {}
        assert_fetchable_with_context::<GroupMembership>();
    }

    #[test]
    fn test_group_fetchable_impls_exist() {
        fn assert_group_fetchable<T: GroupFetchable>() {}
        assert_group_fetchable::<GroupVolume>();
        assert_group_fetchable::<GroupMute>();
    }
}
