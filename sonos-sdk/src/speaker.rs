//! Speaker handle with property accessors
//!
//! Provides a DOM-like interface for accessing speaker properties.

use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::SonosClient;
use sonos_discovery::Device;
use sonos_state::{SpeakerId, StateManager};

use crate::SdkError;

use crate::property::{
    BassHandle, CurrentTrackHandle, GroupMembershipHandle, LoudnessHandle, MuteHandle,
    PlaybackStateHandle, PositionHandle, PropertyHandle, SpeakerContext, TrebleHandle, VolumeHandle,
};

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
    /// Model name of the speaker (e.g., "Sonos One", "Sonos Beam")
    pub model_name: String,

    // ========================================================================
    // RenderingControl properties
    // ========================================================================
    /// Volume property (0-100)
    pub volume: VolumeHandle,
    /// Mute state (true = muted)
    pub mute: MuteHandle,
    /// Bass EQ setting (-10 to +10)
    pub bass: BassHandle,
    /// Treble EQ setting (-10 to +10)
    pub treble: TrebleHandle,
    /// Loudness compensation setting
    pub loudness: LoudnessHandle,

    // ========================================================================
    // AVTransport properties
    // ========================================================================
    /// Playback state (Playing/Paused/Stopped/Transitioning)
    pub playback_state: PlaybackStateHandle,
    /// Current playback position and duration
    pub position: PositionHandle,
    /// Current track information (title, artist, album, etc.)
    pub current_track: CurrentTrackHandle,

    // ========================================================================
    // ZoneGroupTopology properties
    // ========================================================================
    /// Group membership information (group_id, is_coordinator)
    pub group_membership: GroupMembershipHandle,
}

impl Speaker {
    /// Create a Speaker from a discovered Device
    ///
    /// This is the preferred way to create a Speaker when you have a Device
    /// from discovery. It handles IP address parsing and extracts all relevant
    /// fields from the Device struct.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let devices = sonos_discovery::get();
    /// for device in devices {
    ///     let speaker = Speaker::from_device(&device, state_manager.clone(), api_client.clone())?;
    ///     println!("Created speaker: {}", speaker.name);
    /// }
    /// ```
    pub fn from_device(
        device: &Device,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Result<Self, SdkError> {
        let ip: IpAddr = device
            .ip_address
            .parse()
            .map_err(|_| SdkError::InvalidIpAddress)?;

        Ok(Self::new(
            SpeakerId::new(&device.id),
            device.name.clone(),
            ip,
            device.model_name.clone(),
            state_manager,
            api_client,
        ))
    }

    /// Create a new Speaker handle
    ///
    /// For most use cases, prefer [`Speaker::from_device()`] which handles
    /// IP parsing and extracts fields from a Device struct.
    pub fn new(
        id: SpeakerId,
        name: String,
        ip: IpAddr,
        model_name: String,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        let context = SpeakerContext::new(id.clone(), ip, state_manager, api_client);

        Self {
            id,
            name,
            ip,
            model_name,
            // RenderingControl properties
            volume: PropertyHandle::new(Arc::clone(&context)),
            mute: PropertyHandle::new(Arc::clone(&context)),
            bass: PropertyHandle::new(Arc::clone(&context)),
            treble: PropertyHandle::new(Arc::clone(&context)),
            loudness: PropertyHandle::new(Arc::clone(&context)),
            // AVTransport properties
            playback_state: PropertyHandle::new(Arc::clone(&context)),
            position: PropertyHandle::new(Arc::clone(&context)),
            current_track: PropertyHandle::new(Arc::clone(&context)),
            // ZoneGroupTopology properties
            group_membership: PropertyHandle::new(context),
        }
    }
}
