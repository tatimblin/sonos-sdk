//! Speaker handle with sync property accessors
//!
//! Provides a clean sync API for accessing speaker properties:
//!
//! ```rust,ignore
//! let speakers = state_manager.speakers();
//! for speaker in &speakers {
//!     // Sync read (instant, from cache)
//!     if let Some(vol) = speaker.volume.get() {
//!         println!("{}: {}%", speaker.name, vol.0);
//!     }
//!
//!     // Register for change events
//!     speaker.volume.watch()?;
//!
//!     // Fetch fresh value from device (blocking network call)
//!     let fresh_vol = speaker.volume.fetch()?;
//! }
//! ```

use std::marker::PhantomData;
use std::net::IpAddr;
use std::sync::Arc;

use crate::model::{SpeakerId, SpeakerInfo};
use crate::property::{
    Bass, CurrentTrack, GroupMembership, Loudness, Mute, PlaybackState, Position, Property,
    SonosProperty, Treble, Volume,
};
use crate::state::StateManager;
use crate::Result;

// ============================================================================
// PropertyHandle - sync property accessor
// ============================================================================

/// Handle for accessing a specific property on a speaker (sync)
///
/// Provides three access patterns:
/// - `get()`: Instant read of current cached value
/// - `watch()`: Register for change events via `iter()`
/// - `fetch()`: Blocking network call to get fresh value
///
/// All methods are synchronous.
pub struct PropertyHandle<P: SonosProperty> {
    speaker_id: SpeakerId,
    speaker_ip: IpAddr,
    state_manager: Arc<StateManager>,
    _phantom: PhantomData<P>,
}

impl<P: SonosProperty> PropertyHandle<P> {
    /// Create a new property handle
    pub(crate) fn new(
        speaker_id: SpeakerId,
        speaker_ip: IpAddr,
        state_manager: Arc<StateManager>,
    ) -> Self {
        Self {
            speaker_id,
            speaker_ip,
            state_manager,
            _phantom: PhantomData,
        }
    }

    /// Get current cached value (instant, no network)
    ///
    /// Returns the value from the local cache. If the value hasn't been
    /// fetched yet or hasn't arrived via events, returns `None`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(vol) = speaker.volume.get() {
    ///     println!("Volume: {}%", vol.0);
    /// }
    /// ```
    pub fn get(&self) -> Option<P> {
        self.state_manager.get_property::<P>(&self.speaker_id)
    }

    /// Register for change events and return current value
    ///
    /// After calling `watch()`, changes to this property will appear
    /// in `manager.iter()`. The subscription stays active until
    /// `unwatch()` is called or the StateManager is dropped.
    ///
    /// When an event manager is configured, this will also subscribe
    /// to the UPnP service for this property.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Register for volume changes
    /// let current = speaker.volume.watch()?;
    ///
    /// // Now iterate over changes
    /// for event in manager.iter() {
    ///     if event.property_key == "volume" {
    ///         let new_vol = speaker.volume.get();
    ///     }
    /// }
    /// ```
    pub fn watch(&self) -> Result<Option<P>> {
        // Register in local watched set
        self.state_manager.register_watch(&self.speaker_id, P::KEY);

        // Subscribe via event manager (sync call)
        if let Some(em) = self.state_manager.event_manager() {
            if let Err(e) = em.ensure_service_subscribed(self.speaker_ip, P::SERVICE) {
                tracing::warn!(
                    "Failed to subscribe to {:?} for {}: {}",
                    P::SERVICE,
                    self.speaker_id.as_str(),
                    e
                );
            }
        }

        Ok(self.get())
    }

    /// Stop watching this property
    ///
    /// After calling `unwatch()`, changes to this property will no longer
    /// appear in `manager.iter()`.
    ///
    /// When an event manager is configured, this will release the
    /// UPnP service subscription (if no other watchers remain).
    pub fn unwatch(&self) {
        // Unregister from local watched set
        self.state_manager.unregister_watch(&self.speaker_id, P::KEY);

        // Release subscription via event manager (sync call)
        if let Some(em) = self.state_manager.event_manager() {
            if let Err(e) = em.release_service_subscription(self.speaker_ip, P::SERVICE) {
                tracing::warn!(
                    "Failed to unsubscribe from {:?} for {}: {}",
                    P::SERVICE,
                    self.speaker_id.as_str(),
                    e
                );
            }
        }
    }

    /// Check if this property is currently being watched
    pub fn is_watched(&self) -> bool {
        self.state_manager.is_watched(&self.speaker_id, P::KEY)
    }

    /// Fetch fresh value from device (blocking network call)
    ///
    /// Makes a UPnP request to the device to get the current value.
    /// This is slower than `get()` but always returns the latest value.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Force refresh from device
    /// let fresh_vol = speaker.volume.fetch()?;
    /// println!("Fresh volume: {}%", fresh_vol.0);
    /// ```
    ///
    /// # Note
    ///
    /// Currently returns cached value as a placeholder.
    /// Full implementation requires sonos-api integration.
    pub fn fetch(&self) -> Result<Option<P>> {
        // TODO: Implement actual fetch via sonos-api
        // For now, return cached value
        Ok(self.get())
    }

    /// Get the speaker ID this handle belongs to
    pub fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    /// Get the property key
    pub fn property_key(&self) -> &'static str {
        P::KEY
    }
}

impl<P: SonosProperty> Clone for PropertyHandle<P> {
    fn clone(&self) -> Self {
        Self {
            speaker_id: self.speaker_id.clone(),
            speaker_ip: self.speaker_ip,
            state_manager: Arc::clone(&self.state_manager),
            _phantom: PhantomData,
        }
    }
}

impl<P: SonosProperty> std::fmt::Debug for PropertyHandle<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyHandle")
            .field("speaker_id", &self.speaker_id)
            .field("property_key", &P::KEY)
            .finish()
    }
}

// ============================================================================
// Speaker - handle with property accessors
// ============================================================================

/// Handle for a Sonos speaker with property accessors (sync)
///
/// Provides:
/// - Device metadata (id, name, ip, model)
/// - Property handles for all supported properties
///
/// Each property handle provides `get()`, `watch()`, and `fetch()` methods.
/// All methods are synchronous.
///
/// # Example
///
/// ```rust,ignore
/// let speaker = state_manager.speaker(&speaker_id)?;
///
/// // Read volume without subscription
/// if let Some(vol) = speaker.volume.get() {
///     println!("Volume: {}%", vol.0);
/// }
///
/// // Watch volume for changes
/// speaker.volume.watch()?;
///
/// // Fetch fresh value from device
/// let fresh = speaker.volume.fetch()?;
/// ```
#[derive(Clone)]
pub struct Speaker {
    // === Metadata ===
    /// Unique speaker identifier
    pub id: SpeakerId,
    /// Friendly name of the speaker
    pub name: String,
    /// Room name
    pub room_name: String,
    /// IP address
    pub ip_address: IpAddr,
    /// Port (typically 1400)
    pub port: u16,
    /// Model name (e.g., "Sonos One")
    pub model_name: String,

    // === Property Handles (RenderingControl) ===
    /// Volume property (0-100)
    pub volume: PropertyHandle<Volume>,
    /// Mute property
    pub mute: PropertyHandle<Mute>,
    /// Bass EQ (-10 to +10)
    pub bass: PropertyHandle<Bass>,
    /// Treble EQ (-10 to +10)
    pub treble: PropertyHandle<Treble>,
    /// Loudness compensation
    pub loudness: PropertyHandle<Loudness>,

    // === Property Handles (AVTransport) ===
    /// Current playback state
    pub playback_state: PropertyHandle<PlaybackState>,
    /// Playback position
    pub position: PropertyHandle<Position>,
    /// Current track info
    pub current_track: PropertyHandle<CurrentTrack>,
    /// Group membership
    pub group_membership: PropertyHandle<GroupMembership>,
}

impl Speaker {
    /// Create a new Speaker handle from SpeakerInfo
    pub(crate) fn new(info: SpeakerInfo, state_manager: Arc<StateManager>) -> Self {
        let speaker_id = info.id.clone();
        let speaker_ip = info.ip_address;

        Self {
            // Metadata
            id: info.id,
            name: info.name,
            room_name: info.room_name,
            ip_address: info.ip_address,
            port: info.port,
            model_name: info.model_name,

            // Property handles - RenderingControl
            volume: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),
            mute: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),
            bass: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),
            treble: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),
            loudness: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),

            // Property handles - AVTransport
            playback_state: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),
            position: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),
            current_track: PropertyHandle::new(speaker_id.clone(), speaker_ip, Arc::clone(&state_manager)),
            group_membership: PropertyHandle::new(speaker_id, speaker_ip, state_manager),
        }
    }
}

impl std::fmt::Debug for Speaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speaker")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("room_name", &self.room_name)
            .field("ip_address", &self.ip_address)
            .field("model_name", &self.model_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_speaker_info() -> SpeakerInfo {
        SpeakerInfo {
            id: SpeakerId::new("RINCON_123"),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "1.0".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_speaker_debug() {
        let info = create_test_speaker_info();
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("Living Room"));
        assert!(debug_str.contains("RINCON_123"));
    }

    #[test]
    fn test_property_handle_key() {
        // Just verify the property key is correct
        assert_eq!(Volume::KEY, "volume");
        assert_eq!(Mute::KEY, "mute");
        assert_eq!(PlaybackState::KEY, "playback_state");
    }
}
