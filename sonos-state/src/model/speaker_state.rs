//! Speaker state type

use super::{GroupId, PlaybackState, Speaker, TrackInfo};
use serde::{Deserialize, Serialize};

/// Complete state of a speaker including playback and group information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerState {
    /// The speaker device information
    pub speaker: Speaker,
    /// Current playback state
    pub playback_state: PlaybackState,
    /// Current volume (0-100)
    pub volume: u8,
    /// Whether the speaker is muted
    pub muted: bool,
    /// Current playback position in milliseconds
    pub position_ms: u64,
    /// Current track duration in milliseconds
    pub duration_ms: u64,
    /// Whether this speaker is the coordinator of its group
    pub is_coordinator: bool,
    /// The group this speaker belongs to
    pub group_id: Option<GroupId>,
    /// Current track information
    pub current_track: Option<TrackInfo>,
}

impl SpeakerState {
    /// Create a new SpeakerState with default values
    pub fn new(speaker: Speaker) -> Self {
        Self {
            speaker,
            playback_state: PlaybackState::default(),
            volume: 0,
            muted: false,
            position_ms: 0,
            duration_ms: 0,
            is_coordinator: false,
            group_id: None,
            current_track: None,
        }
    }

    /// Get the speaker ID
    pub fn get_id(&self) -> &super::SpeakerId {
        self.speaker.get_id()
    }

    /// Check if this speaker is currently playing
    pub fn is_playing(&self) -> bool {
        self.playback_state == PlaybackState::Playing
    }

    /// Get playback progress as a percentage (0.0 - 100.0)
    pub fn progress_percent(&self) -> f64 {
        if self.duration_ms == 0 {
            0.0
        } else {
            (self.position_ms as f64 / self.duration_ms as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SpeakerId;
    use std::net::IpAddr;

    fn create_test_speaker() -> Speaker {
        Speaker {
            id: SpeakerId::new("RINCON_123"),
            name: "Test Speaker".to_string(),
            room_name: "Test Room".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "56.0".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_new_defaults() {
        let state = SpeakerState::new(create_test_speaker());
        assert_eq!(state.playback_state, PlaybackState::Stopped);
        assert_eq!(state.volume, 0);
        assert!(!state.muted);
        assert_eq!(state.position_ms, 0);
        assert!(!state.is_coordinator);
        assert!(state.group_id.is_none());
    }

    #[test]
    fn test_is_playing() {
        let mut state = SpeakerState::new(create_test_speaker());
        assert!(!state.is_playing());

        state.playback_state = PlaybackState::Playing;
        assert!(state.is_playing());
    }

    #[test]
    fn test_progress_percent() {
        let mut state = SpeakerState::new(create_test_speaker());

        // Zero duration
        assert_eq!(state.progress_percent(), 0.0);

        // With duration
        state.duration_ms = 200_000;
        state.position_ms = 100_000;
        assert!((state.progress_percent() - 50.0).abs() < 0.001);
    }
}
