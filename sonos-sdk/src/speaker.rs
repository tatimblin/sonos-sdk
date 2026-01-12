use std::net::IpAddr;
use std::sync::Arc;
use sonos_state::{StateManager, SpeakerId};
use sonos_api::SonosClient;
use crate::property::{VolumeHandle, PlaybackStateHandle};

/// Speaker handle with property access
#[derive(Clone)]
pub struct Speaker {
    pub id: SpeakerId,
    pub name: String,
    pub ip: IpAddr,

    // Property handles providing get()/fetch()/watch()
    pub volume: VolumeHandle,
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
    pub fn new(
        id: SpeakerId,
        name: String,
        ip: IpAddr,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        Self {
            id: id.clone(),
            name,
            ip,
            volume: VolumeHandle::new(id.clone(), ip, Arc::clone(&state_manager), api_client.clone()),
            playback_state: PlaybackStateHandle::new(id.clone(), ip, Arc::clone(&state_manager), api_client),
        }
    }
}