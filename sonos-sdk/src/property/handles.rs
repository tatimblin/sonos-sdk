//! Generic PropertyHandle for DOM-like property access
//!
//! Provides a consistent pattern for accessing any property on a speaker:
//! - `get()` - Get cached value (instant, no network)
//! - `fetch()` - Fetch fresh value from device (blocking API call)
//! - `watch()` - Register for change notifications
//! - `unwatch()` - Unregister from change notifications

use std::marker::PhantomData;
use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::operation::{ComposableOperation, UPnPOperation};
use sonos_api::SonosClient;
use sonos_state::{property::SonosProperty, SpeakerId, StateManager};

use crate::SdkError;

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
    speaker_id: SpeakerId,
    speaker_ip: IpAddr,
    state_manager: Arc<StateManager>,
    api_client: SonosClient,
    _phantom: PhantomData<P>,
}

impl<P: SonosProperty> PropertyHandle<P> {
    /// Create a new PropertyHandle for a specific speaker and property type
    pub fn new(
        speaker_id: SpeakerId,
        speaker_ip: IpAddr,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        Self {
            speaker_id,
            speaker_ip,
            state_manager,
            api_client,
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
    pub fn get(&self) -> Option<P> {
        self.state_manager.get_property::<P>(&self.speaker_id)
    }

    /// Start watching this property for changes (sync)
    ///
    /// Registers this property for change notifications. After calling
    /// `watch()`, changes to this property will appear in `system.iter()`.
    ///
    /// When an event manager is configured, this will automatically
    /// subscribe to the UPnP service for this property.
    ///
    /// Returns the current cached value if available.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Start watching volume changes
    /// let current_volume = speaker.volume.watch()?;
    ///
    /// // Now changes will appear in system.iter()
    /// for event in system.iter() {
    ///     if event.property_key == "volume" {
    ///         let new_vol = speaker.volume.get();
    ///         println!("Volume changed to: {:?}", new_vol);
    ///     }
    /// }
    /// ```
    pub fn watch(&self) -> Result<Option<P>, SdkError> {
        // Register for changes via state manager
        // This will also subscribe via the event manager if configured
        self.state_manager.register_watch(&self.speaker_id, P::KEY);

        // Return current cached value
        Ok(self.get())
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
        self.state_manager
            .unregister_watch(&self.speaker_id, P::KEY);
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
    pub fn is_watched(&self) -> bool {
        self.state_manager.is_watched(&self.speaker_id, P::KEY)
    }

    /// Get the speaker ID this handle is associated with
    pub fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    /// Get the speaker IP address
    pub fn speaker_ip(&self) -> IpAddr {
        self.speaker_ip
    }

    /// Get a reference to the API client (for fetch implementations)
    pub(crate) fn api_client(&self) -> &SonosClient {
        &self.api_client
    }

    /// Get a reference to the state manager (for fetch implementations)
    pub(crate) fn state_manager(&self) -> &Arc<StateManager> {
        &self.state_manager
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
    pub fn fetch(&self) -> Result<P, SdkError> {
        // 1. Build operation using the Fetchable trait
        let operation = P::build_operation()?;

        // 2. Execute operation using enhanced API (sync call)
        let response = self
            .api_client
            .execute_enhanced(&self.speaker_ip.to_string(), operation)
            .map_err(SdkError::ApiError)?;

        // 3. Convert response to property type
        let property_value = P::from_response(response);

        // 4. Update state store
        self.state_manager
            .set_property(&self.speaker_id, property_value.clone());

        Ok(property_value)
    }
}

// ============================================================================
// Type aliases for common property handles
// ============================================================================

use sonos_api::services::{
    av_transport::{self, GetTransportInfoOperation, GetTransportInfoResponse},
    rendering_control::{self, GetVolumeOperation, GetVolumeResponse},
};
use sonos_state::{
    Bass, CurrentTrack, GroupMembership, Loudness, Mute, PlaybackState, Position, Treble, Volume,
};

// ============================================================================
// Fetchable implementations
// ============================================================================

impl Fetchable for Volume {
    type Operation = GetVolumeOperation;

    fn build_operation() -> Result<ComposableOperation<Self::Operation>, SdkError> {
        rendering_control::get_volume_operation("Master".to_string())
            .build()
            .map_err(|e| SdkError::FetchFailed(format!("Failed to build GetVolume operation: {}", e)))
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
            .map_err(|e| {
                SdkError::FetchFailed(format!("Failed to build GetTransportInfo operation: {}", e))
            })
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

    #[test]
    fn test_property_handle_creation() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let api_client = SonosClient::new();

        let handle: VolumeHandle =
            PropertyHandle::new(speaker_id.clone(), speaker_ip, state_manager, api_client);

        assert_eq!(handle.speaker_id().as_str(), "RINCON_TEST123");
        assert_eq!(handle.speaker_ip(), speaker_ip);
    }

    #[test]
    fn test_get_returns_none_initially() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let api_client = SonosClient::new();

        let handle: VolumeHandle =
            PropertyHandle::new(speaker_id, speaker_ip, state_manager, api_client);

        // Initially no value cached
        assert!(handle.get().is_none());
    }

    #[test]
    fn test_get_returns_cached_value() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let api_client = SonosClient::new();

        // Set a value in the state manager
        state_manager.set_property(&speaker_id, Volume::new(75));

        let handle: VolumeHandle = PropertyHandle::new(
            speaker_id,
            speaker_ip,
            Arc::clone(&state_manager),
            api_client,
        );

        // Should return the cached value
        assert_eq!(handle.get(), Some(Volume::new(75)));
    }

    #[test]
    fn test_watch_registers_property() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let api_client = SonosClient::new();

        let handle: VolumeHandle = PropertyHandle::new(
            speaker_id.clone(),
            speaker_ip,
            Arc::clone(&state_manager),
            api_client,
        );

        // Not watched initially
        assert!(!handle.is_watched());

        // Watch the property
        handle.watch().unwrap();

        // Now it should be watched
        assert!(handle.is_watched());
    }

    #[test]
    fn test_unwatch_unregisters_property() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let api_client = SonosClient::new();

        let handle: VolumeHandle = PropertyHandle::new(
            speaker_id.clone(),
            speaker_ip,
            Arc::clone(&state_manager),
            api_client,
        );

        // Watch then unwatch
        handle.watch().unwrap();
        assert!(handle.is_watched());

        handle.unwatch();
        assert!(!handle.is_watched());
    }

    #[test]
    fn test_watch_returns_current_value() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let api_client = SonosClient::new();

        // Set a value first
        state_manager.set_property(&speaker_id, Volume::new(50));

        let handle: VolumeHandle = PropertyHandle::new(
            speaker_id,
            speaker_ip,
            Arc::clone(&state_manager),
            api_client,
        );

        // Watch should return the current value
        let result = handle.watch().unwrap();
        assert_eq!(result, Some(Volume::new(50)));
    }

    #[test]
    fn test_property_handle_clone() {
        let state_manager = create_test_state_manager();
        let speaker_id = SpeakerId::new("RINCON_TEST123");
        let speaker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let api_client = SonosClient::new();

        // Set a value
        state_manager.set_property(&speaker_id, Volume::new(60));

        let handle: VolumeHandle = PropertyHandle::new(
            speaker_id,
            speaker_ip,
            Arc::clone(&state_manager),
            api_client,
        );

        // Clone the handle
        let cloned = handle.clone();

        // Both should see the same value
        assert_eq!(handle.get(), cloned.get());
        assert_eq!(handle.get(), Some(Volume::new(60)));
    }
}
