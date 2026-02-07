//! Speaker handle with property accessors
//!
//! Provides a DOM-like interface for accessing speaker properties.

use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::SonosClient;
use sonos_state::{SpeakerId, StateManager};

use crate::property::{PlaybackStateHandle, VolumeHandle};

/// Speaker handle with property access
///
/// Provides direct access to speaker properties through property handles.
/// Each property handle provides `get()`, `fetch()`, `watch()`, and `unwatch()` methods.
///
/// # Example
///
/// ```rust,ignore
/// // Get cached volume
/// let volume = speaker.volume.get();
///
/// // Fetch fresh volume from device
/// let fresh_volume = speaker.volume.fetch()?;
///
/// // Watch for volume changes
/// speaker.volume.watch()?;
///
/// // Stop watching
/// speaker.volume.unwatch();
/// ```
#[derive(Clone)]
pub struct Speaker {
    /// Unique speaker identifier
    pub id: SpeakerId,
    /// Friendly name of the speaker
    pub name: String,
    /// IP address of the speaker
    pub ip: IpAddr,

    // Property handles providing get()/fetch()/watch()
    /// Volume property (0-100)
    pub volume: VolumeHandle,
    /// Playback state (Playing/Paused/Stopped/Transitioning)
    pub playback_state: PlaybackStateHandle,
    // TODO: Add more properties as they become available:
    // pub mute: MuteHandle,
    // pub position: PositionHandle,
    // pub current_track: CurrentTrackHandle,
    // pub bass: BassHandle,
    // pub treble: TrebleHandle,
    // pub loudness: LoudnessHandle,
}

impl Speaker {
    /// Create a new Speaker handle
    pub fn new(
        id: SpeakerId,
        name: String,
        ip: IpAddr,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        use crate::property::PropertyHandle;

        Self {
            id: id.clone(),
            name,
            ip,
            volume: PropertyHandle::new(
                id.clone(),
                ip,
                Arc::clone(&state_manager),
                api_client.clone(),
            ),
            playback_state: PropertyHandle::new(
                id.clone(),
                ip,
                Arc::clone(&state_manager),
                api_client,
            ),
        }
    }
}
