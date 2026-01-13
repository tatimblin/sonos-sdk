//! Speaker handle with property accessors.
//!
//! Provides a clean API for accessing speaker properties:
//!
//! ```rust,ignore
//! let speakers = state_manager.speakers();
//! for speaker in &speakers {
//!     let volume = speaker.volume.get();
//!     let mute = speaker.mute.watch().await?;
//! }
//! ```

use std::net::IpAddr;
use std::sync::Arc;

use crate::model::{SpeakerId, SpeakerInfo};
use crate::property::{
    Bass, CurrentTrack, GroupMembership, Loudness, Mute, PlaybackState, Position, Treble, Volume,
};
use crate::property_handle::PropertyHandle;
use crate::reactive::StateManager;
use crate::watch_cache::WatchCache;

/// Handle for a Sonos speaker with property accessors.
///
/// Provides:
/// - Device metadata (id, name, ip, model)
/// - Property handles for all supported properties
///
/// Each property handle provides `get()` and `watch()` methods:
/// - `get()`: Synchronous read of current value
/// - `watch()`: Ensures subscription exists and returns current value
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
/// // Watch volume (creates subscription if needed)
/// let volume = speaker.volume.watch().await?;
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
    pub(crate) fn new(
        info: SpeakerInfo,
        state_manager: Arc<StateManager>,
        watch_cache: Arc<WatchCache>,
    ) -> Self {
        let speaker_id = info.id.clone();

        Self {
            // Metadata
            id: info.id,
            name: info.name,
            room_name: info.room_name,
            ip_address: info.ip_address,
            port: info.port,
            model_name: info.model_name,

            // Property handles - RenderingControl
            volume: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),
            mute: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),
            bass: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),
            treble: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),
            loudness: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),

            // Property handles - AVTransport
            playback_state: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),
            position: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),
            current_track: PropertyHandle::new(
                speaker_id.clone(),
                Arc::clone(&state_manager),
                Arc::clone(&watch_cache),
            ),
            group_membership: PropertyHandle::new(
                speaker_id,
                state_manager,
                watch_cache,
            ),
        }
    }

    /// Create a Speaker handle from an existing SpeakerInfo with shared resources
    pub(crate) fn from_info(
        info: &SpeakerInfo,
        state_manager: Arc<StateManager>,
        watch_cache: Arc<WatchCache>,
    ) -> Self {
        Self::new(info.clone(), state_manager, watch_cache)
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

    #[test]
    fn test_speaker_debug() {
        // Just verify Debug impl compiles and produces reasonable output
        let info = SpeakerInfo {
            id: SpeakerId::new("test-speaker"),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "1.0".to_string(),
            satellites: vec![],
        };

        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("Living Room"));
        assert!(debug_str.contains("test-speaker"));
    }
}
