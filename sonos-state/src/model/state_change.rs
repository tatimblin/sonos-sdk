//! State change event types

use super::{Group, PlaybackState, SpeakerId, TrackInfo};
use serde::{Deserialize, Serialize};

/// Represents a state change event that consumers can react to
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateChange {
    /// Volume changed on a speaker
    VolumeChanged {
        /// Speaker that changed
        speaker_id: SpeakerId,
        /// Previous volume level
        old_volume: u8,
        /// New volume level
        new_volume: u8,
    },

    /// Mute state changed on a speaker
    MuteChanged {
        /// Speaker that changed
        speaker_id: SpeakerId,
        /// New mute state
        muted: bool,
    },

    /// Playback state changed on a speaker
    PlaybackStateChanged {
        /// Speaker that changed
        speaker_id: SpeakerId,
        /// Previous playback state
        old_state: PlaybackState,
        /// New playback state
        new_state: PlaybackState,
    },

    /// Track position changed (for seeking/progress)
    PositionChanged {
        /// Speaker that changed
        speaker_id: SpeakerId,
        /// New position in milliseconds
        position_ms: u64,
        /// Track duration in milliseconds
        duration_ms: u64,
    },

    /// Current track changed
    TrackChanged {
        /// Speaker that changed
        speaker_id: SpeakerId,
        /// Previous track (if any)
        old_track: Option<TrackInfo>,
        /// New track (if any)
        new_track: Option<TrackInfo>,
    },

    /// Zone group topology changed
    GroupsChanged {
        /// Previous group configuration
        old_groups: Vec<Group>,
        /// New group configuration
        new_groups: Vec<Group>,
    },

    /// A new speaker was discovered and added to state
    SpeakerAdded {
        /// ID of the added speaker
        speaker_id: SpeakerId,
    },

    /// A speaker went offline or was removed
    SpeakerRemoved {
        /// ID of the removed speaker
        speaker_id: SpeakerId,
    },

    /// Full state initialized (emitted after initialization completes)
    StateInitialized {
        /// Number of speakers discovered
        speaker_count: usize,
        /// Number of groups discovered
        group_count: usize,
    },

    /// Error occurred during event processing
    ProcessingError {
        /// Speaker involved (if applicable)
        speaker_id: Option<SpeakerId>,
        /// Service name involved (if applicable)
        service: Option<String>,
        /// Error description
        error: String,
    },
}

impl StateChange {
    /// Get the speaker ID associated with this change, if any
    pub fn speaker_id(&self) -> Option<&SpeakerId> {
        match self {
            StateChange::VolumeChanged { speaker_id, .. } => Some(speaker_id),
            StateChange::MuteChanged { speaker_id, .. } => Some(speaker_id),
            StateChange::PlaybackStateChanged { speaker_id, .. } => Some(speaker_id),
            StateChange::PositionChanged { speaker_id, .. } => Some(speaker_id),
            StateChange::TrackChanged { speaker_id, .. } => Some(speaker_id),
            StateChange::SpeakerAdded { speaker_id } => Some(speaker_id),
            StateChange::SpeakerRemoved { speaker_id } => Some(speaker_id),
            StateChange::ProcessingError { speaker_id, .. } => speaker_id.as_ref(),
            StateChange::GroupsChanged { .. } => None,
            StateChange::StateInitialized { .. } => None,
        }
    }

    /// Check if this is an error event
    pub fn is_error(&self) -> bool {
        matches!(self, StateChange::ProcessingError { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speaker_id_extraction() {
        let change = StateChange::VolumeChanged {
            speaker_id: SpeakerId::new("RINCON_123"),
            old_volume: 50,
            new_volume: 60,
        };
        assert_eq!(change.speaker_id().unwrap().as_str(), "RINCON_123");

        let groups_change = StateChange::GroupsChanged {
            old_groups: vec![],
            new_groups: vec![],
        };
        assert!(groups_change.speaker_id().is_none());
    }

    #[test]
    fn test_is_error() {
        let volume_change = StateChange::VolumeChanged {
            speaker_id: SpeakerId::new("RINCON_123"),
            old_volume: 50,
            new_volume: 60,
        };
        assert!(!volume_change.is_error());

        let error = StateChange::ProcessingError {
            speaker_id: None,
            service: None,
            error: "Test error".to_string(),
        };
        assert!(error.is_error());
    }
}
