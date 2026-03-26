//! Generic PropertyHandle for DOM-like property access
//!
//! Provides a consistent pattern for accessing any property on a speaker:
//! - `get()` - Get cached value (instant, no network)
//! - `fetch()` - Fetch fresh value from device (blocking API call)
//! - `watch()` - Returns a `WatchHandle` that keeps the subscription alive

use std::fmt;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::ops::Deref;
use std::sync::Arc;

use sonos_api::operation::{ComposableOperation, UPnPOperation};
use sonos_api::SonosClient;
use sonos_event_manager::WatchGuard;
use sonos_state::{property::SonosProperty, SpeakerId, StateManager};

use crate::SdkError;

/// Closure type for lazy event manager initialization.
///
/// Called by `PropertyHandle::watch()` to trigger event manager creation
/// on first use. Captures shared `Arc`s to the system's `Mutex` and
/// `StateManager`, avoiding a direct reference to `SonosSystem`.
pub type EventInitFn = Arc<dyn Fn() -> Result<(), SdkError> + Send + Sync>;

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
    /// Optional closure to trigger lazy event manager initialization.
    /// `None` in test mode (no event infrastructure).
    pub(crate) event_init: Option<EventInitFn>,
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
            event_init: None,
        })
    }

    /// Create a new SpeakerContext with an event init closure
    pub fn with_event_init(
        speaker_id: SpeakerId,
        speaker_ip: IpAddr,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
        event_init: EventInitFn,
    ) -> Arc<Self> {
        Arc::new(Self {
            speaker_id,
            speaker_ip,
            state_manager,
            api_client,
            event_init: Some(event_init),
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

/// RAII handle returned by `watch()`. Holds a snapshot of the current value
/// along with a subscription guard. Dropping the handle starts the grace
/// period — the UPnP subscription persists for 50ms so it can be reacquired
/// cheaply on the next frame.
///
/// Not `Clone` — each handle is one subscription hold.
///
/// # Example
///
/// ```rust,ignore
/// // Watch returns a handle — hold it to keep the subscription alive
/// let volume = speaker.volume.watch()?;
///
/// // Deref to Option<P> for ergonomic access
/// if let Some(v) = &*volume {
///     println!("Volume: {}%", v.value());
/// }
///
/// // Or use the value() convenience method
/// if let Some(v) = volume.value() {
///     println!("Volume: {}%", v.value());
/// }
///
/// // Dropping the handle starts the 50ms grace period
/// drop(volume);
/// ```
#[must_use = "dropping the handle starts the grace period — hold it to keep the subscription alive"]
pub struct WatchHandle<P> {
    value: Option<P>,
    mode: WatchMode,
    _cleanup: WatchCleanup,
}

impl<P> Deref for WatchHandle<P> {
    type Target = Option<P>;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<P> WatchHandle<P> {
    /// Returns the watch mode (Events, Polling, or CacheOnly).
    pub fn mode(&self) -> WatchMode {
        self.mode
    }

    /// Convenience: returns a reference to the inner value, if available.
    /// Equivalent to `(*handle).as_ref()` but more ergonomic.
    pub fn value(&self) -> Option<&P> {
        self.value.as_ref()
    }

    /// Returns true if a value has been received from the device.
    pub fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// Returns true if real-time UPnP events are active.
    pub fn has_realtime_events(&self) -> bool {
        self.mode == WatchMode::Events
    }
}

impl<P: fmt::Debug> fmt::Debug for WatchHandle<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatchHandle")
            .field("value", &self.value)
            .field("mode", &self.mode)
            .finish()
    }
}

/// Internal cleanup strategy for WatchHandle.
///
/// - `Guard`: Event manager is active — WatchGuard handles the subscription
///   lifecycle (ref counting, grace period, unsubscribe).
/// - `CacheOnly`: No event manager — just unregisters from the watched set.
///
/// Fields are never read — they exist solely for their Drop behavior.
#[allow(dead_code)]
enum WatchCleanup {
    Guard(WatchGuard),
    CacheOnly(CacheOnlyGuard),
}

/// Cleanup guard for CacheOnly mode (no event manager).
/// Unregisters the property from the watched set on drop.
struct CacheOnlyGuard {
    state_manager: Arc<StateManager>,
    speaker_id: SpeakerId,
    property_key: &'static str,
}

impl Drop for CacheOnlyGuard {
    fn drop(&mut self) {
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
    /// # Example
    ///
    /// ```rust,ignore
    /// // Watch returns a handle — hold it to keep the subscription alive
    /// let volume = speaker.volume.watch()?;
    ///
    /// // Access the current value via Deref
    /// if let Some(v) = &*volume {
    ///     println!("Volume: {}%", v.value());
    /// }
    ///
    /// // Changes will appear in system.iter() while the handle is alive
    /// for event in system.iter() {
    ///     // Re-watch each frame to refresh the snapshot
    ///     let volume = speaker.volume.watch()?;
    ///     println!("Volume: {:?}", volume.value());
    /// }
    /// ```
    pub fn watch(&self) -> Result<WatchHandle<P>, SdkError> {
        // Trigger lazy event manager init if needed
        if self.context.state_manager.event_manager().is_none() {
            if let Some(ref init) = self.context.event_init {
                init()?;
            }
        }

        let (mode, cleanup) = if let Some(em) = self.context.state_manager.event_manager() {
            match em.acquire_watch(
                &self.context.speaker_id,
                P::KEY,
                self.context.speaker_ip,
                P::SERVICE,
            ) {
                Ok(guard) => (WatchMode::Events, WatchCleanup::Guard(guard)),
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

        Ok(WatchHandle {
            value: self.get(),
            mode,
            _cleanup: cleanup,
        })
    }

    /// Check if this property is currently being watched
    ///
    /// Returns `true` while a `WatchHandle` for this property is alive,
    /// or during the grace period after the last handle was dropped.
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
        // 1. Build operation using the Fetchable trait
        let operation = P::build_operation()?;

        // 2. Execute operation using enhanced API (sync call)
        let response = self
            .context
            .api_client
            .execute_enhanced(&self.context.speaker_ip.to_string(), operation)
            .map_err(SdkError::ApiError)?;

        // 3. Convert response to property type
        let property_value = P::from_response(response);

        // 4. Update state store
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, property_value.clone());

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

        self.context
            .state_manager
            .set_property(&self.context.speaker_id, property_value.clone());

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

        Ok(WatchHandle {
            value: self.get(),
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
    /// Fetch fresh value from coordinator + update group cache (sync)
    #[must_use = "returns the fetched value from the device"]
    pub fn fetch(&self) -> Result<P, SdkError> {
        let operation = P::build_operation()?;

        let response = self
            .context
            .api_client
            .execute_enhanced(&self.context.coordinator_ip.to_string(), operation)
            .map_err(SdkError::ApiError)?;

        let property_value = P::from_response(response);

        self.context
            .state_manager
            .set_group_property(&self.context.group_id, property_value.clone());

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

    #[test]
    fn test_watch_returns_current_value() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(50));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        let wh = handle.watch().unwrap();
        assert_eq!(*wh, Some(Volume::new(50)));
        assert_eq!(wh.value(), Some(&Volume::new(50)));
        // No event manager configured, so should be CacheOnly mode
        assert_eq!(wh.mode(), WatchMode::CacheOnly);
    }

    #[test]
    fn test_watch_handle_deref() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(75));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        let wh = handle.watch().unwrap();
        // Deref<Target = Option<P>>
        assert!(wh.has_value());
        assert!(!wh.has_realtime_events());
        if let Some(v) = &*wh {
            assert_eq!(v.value(), 75);
        } else {
            panic!("Expected Some value");
        }
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
