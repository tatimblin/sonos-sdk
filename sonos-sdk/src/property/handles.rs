//! Generic PropertyHandle for DOM-like property access
//!
//! Provides a consistent pattern for accessing any property on a speaker:
//! - `get()` - Get cached value (instant, no network)
//! - `fetch()` - Fetch fresh value from device (blocking API call)
//! - `watch()` - Register for change notifications
//! - `unwatch()` - Unregister from change notifications

use std::fmt;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::operation::{ComposableOperation, UPnPOperation};
use sonos_api::SonosClient;
use sonos_state::{property::SonosProperty, SpeakerId, StateManager};

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

/// Result of a `watch()` operation
///
/// Contains both the current cached value (if any) and information about
/// how future updates will be delivered.
#[derive(Debug, Clone)]
pub struct WatchStatus<P> {
    /// Current cached value of the property (if any)
    pub current: Option<P>,

    /// How updates will be delivered
    ///
    /// Check this to understand whether real-time events are working:
    /// - `Events`: Full real-time support
    /// - `Polling`: Degraded but functional
    /// - `CacheOnly`: Manual refresh only
    pub mode: WatchMode,
}

impl<P> WatchStatus<P> {
    /// Create a new WatchStatus
    pub fn new(current: Option<P>, mode: WatchMode) -> Self {
        Self { current, mode }
    }

    /// Check if real-time events are active
    #[must_use]
    pub fn has_realtime_events(&self) -> bool {
        self.mode == WatchMode::Events
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
/// // Watch for changes (registers for notifications)
/// let current = speaker.volume.watch()?;
///
/// // Stop watching
/// speaker.volume.unwatch();
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
    /// Registers this property for change notifications. After calling
    /// `watch()`, changes to this property will appear in `system.iter()`.
    ///
    /// When an event manager is configured, this will automatically
    /// subscribe to the UPnP service for this property.
    ///
    /// Returns a [`WatchStatus`] containing:
    /// - `current`: The current cached value (if any)
    /// - `mode`: How updates will be delivered (Events, Polling, or CacheOnly)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Start watching volume changes
    /// let status = speaker.volume.watch()?;
    ///
    /// // Check if real-time events are working
    /// if !status.has_realtime_events() {
    ///     println!("Warning: Running in {} mode", status.mode);
    /// }
    ///
    /// // Now changes will appear in system.iter()
    /// for event in system.iter() {
    ///     if event.property_key == "volume" {
    ///         let new_vol = speaker.volume.get();
    ///         println!("Volume changed to: {:?}", new_vol);
    ///     }
    /// }
    /// ```
    #[must_use = "returns watch status including the delivery mode"]
    pub fn watch(&self) -> Result<WatchStatus<P>, SdkError> {
        // Register for changes via state manager
        self.context
            .state_manager
            .register_watch(&self.context.speaker_id, P::KEY);

        // Determine watch mode based on event manager status
        let mode = if let Some(em) = self.context.state_manager.event_manager() {
            match em.ensure_service_subscribed(self.context.speaker_ip, P::SERVICE) {
                Ok(()) => WatchMode::Events,
                Err(e) => {
                    tracing::warn!(
                        "Failed to subscribe to {:?} for {}: {} - falling back to polling",
                        P::SERVICE,
                        self.context.speaker_id.as_str(),
                        e
                    );
                    WatchMode::Polling
                }
            }
        } else {
            WatchMode::CacheOnly
        };

        // Return status with current cached value
        Ok(WatchStatus::new(self.get(), mode))
    }

    /// Stop watching this property (sync)
    ///
    /// Unregisters this property from change notifications.
    /// When an event manager is configured, this will release
    /// the UPnP service subscription if no other watchers remain.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Stop watching volume changes
    /// speaker.volume.unwatch();
    /// ```
    pub fn unwatch(&self) {
        self.context
            .state_manager
            .unregister_watch(&self.context.speaker_id, P::KEY);

        // Release UPnP service subscription via event manager if configured
        if let Some(em) = self.context.state_manager.event_manager() {
            if let Err(e) = em.release_service_subscription(self.context.speaker_ip, P::SERVICE) {
                tracing::warn!(
                    "Failed to release subscription for {:?} on {}: {}",
                    P::SERVICE,
                    self.context.speaker_id.as_str(),
                    e
                );
            }
        }
    }

    /// Check if this property is currently being watched
    ///
    /// Returns `true` if `watch()` has been called and `unwatch()` has not
    /// been called since.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// speaker.volume.watch()?;
    /// assert!(speaker.volume.is_watched());
    ///
    /// speaker.volume.unwatch();
    /// assert!(!speaker.volume.is_watched());
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
// Type aliases for common property handles
// ============================================================================

use sonos_api::services::{
    av_transport::{
        self, GetPositionInfoOperation, GetPositionInfoResponse, GetTransportInfoOperation,
        GetTransportInfoResponse,
    },
    group_rendering_control::{self, GetGroupVolumeOperation, GetGroupVolumeResponse},
    rendering_control::{self, GetVolumeOperation, GetVolumeResponse},
};
use sonos_state::{
    Bass, CurrentTrack, GroupId, GroupMembership, GroupVolume, Loudness, Mute, PlaybackState,
    Position, Treble, Volume,
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

// ============================================================================
// Placeholder implementations for properties without dedicated API operations
// ============================================================================
//
// Note: The following properties don't have dedicated GetXxx operations in the
// Sonos UPnP API. Their values are typically obtained through:
// 1. UPnP event subscriptions (RenderingControl LastChange events)
// 2. Parsing the LastChange XML from events
//
// For now, these properties can only be read from cache (via get()) after
// being populated by the event system. The fetch() method is not available
// for these property types.
//
// Properties without fetch():
// - Mute: Obtained from RenderingControl events
// - Bass: Obtained from RenderingControl events
// - Treble: Obtained from RenderingControl events
// - Loudness: Obtained from RenderingControl events
// - CurrentTrack: Obtained from AVTransport events (track metadata)
// - GroupMembership: Obtained from ZoneGroupTopology events

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
    /// Registers using the coordinator's speaker ID so the event worker
    /// correctly routes events through to `system.iter()`.
    #[must_use = "returns watch status including the delivery mode"]
    pub fn watch(&self) -> Result<WatchStatus<P>, SdkError> {
        // Register for changes using the coordinator's speaker ID and property key
        self.context
            .state_manager
            .register_watch(&self.context.coordinator_id, P::KEY);

        // Subscribe to the coordinator's UPnP service
        let mode = if let Some(em) = self.context.state_manager.event_manager() {
            match em.ensure_service_subscribed(self.context.coordinator_ip, P::SERVICE) {
                Ok(()) => WatchMode::Events,
                Err(e) => {
                    tracing::warn!(
                        "Failed to subscribe to {:?} for group {}: {} - falling back to polling",
                        P::SERVICE,
                        self.context.group_id.as_str(),
                        e
                    );
                    WatchMode::Polling
                }
            }
        } else {
            WatchMode::CacheOnly
        };

        Ok(WatchStatus::new(self.get(), mode))
    }

    /// Stop watching this group property (sync)
    pub fn unwatch(&self) {
        self.context
            .state_manager
            .unregister_watch(&self.context.coordinator_id, P::KEY);

        if let Some(em) = self.context.state_manager.event_manager() {
            if let Err(e) =
                em.release_service_subscription(self.context.coordinator_ip, P::SERVICE)
            {
                tracing::warn!(
                    "Failed to release subscription for {:?} on group {}: {}",
                    P::SERVICE,
                    self.context.group_id.as_str(),
                    e
                );
            }
        }
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

// ============================================================================
// Group type aliases
// ============================================================================

/// Handle for group volume (0-100)
pub type GroupVolumeHandle = GroupPropertyHandle<GroupVolume>;

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
        handle.watch().unwrap();
        assert!(handle.is_watched());
    }

    #[test]
    fn test_unwatch_unregisters_property() {
        let state_manager = create_test_state_manager();
        let context = create_test_context(Arc::clone(&state_manager));

        let handle: VolumeHandle = PropertyHandle::new(context);

        handle.watch().unwrap();
        assert!(handle.is_watched());

        handle.unwatch();
        assert!(!handle.is_watched());
    }

    #[test]
    fn test_watch_returns_current_value() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");

        state_manager.set_property(&speaker_id, Volume::new(50));

        let context = create_test_context(Arc::clone(&state_manager));
        let handle: VolumeHandle = PropertyHandle::new(context);

        let status = handle.watch().unwrap();
        assert_eq!(status.current, Some(Volume::new(50)));
        // No event manager configured, so should be CacheOnly mode
        assert_eq!(status.mode, WatchMode::CacheOnly);
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
    fn test_group_property_handle_watch_unwatch() {
        let state_manager = create_test_state_manager();
        let context = create_test_group_context(Arc::clone(&state_manager));

        let handle: GroupVolumeHandle = GroupPropertyHandle::new(context);

        assert!(!handle.is_watched());
        handle.watch().unwrap();
        assert!(handle.is_watched());

        handle.unwatch();
        assert!(!handle.is_watched());
    }

    #[test]
    fn test_group_property_handle_group_id() {
        let state_manager = create_test_state_manager();
        let context = create_test_group_context(state_manager);

        let handle: GroupVolumeHandle = GroupPropertyHandle::new(context);

        assert_eq!(handle.group_id().as_str(), "RINCON_TEST123:1");
    }
}
