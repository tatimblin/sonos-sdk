//! Playback state enumeration

use serde::{Deserialize, Serialize};

/// Current playback state of a speaker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    /// Currently playing audio
    Playing,
    /// Playback is paused
    Paused,
    /// Playback is stopped
    Stopped,
    /// Transitioning between states
    Transitioning,
}

impl PlaybackState {
    /// Parse from Sonos transport state string
    ///
    /// Handles common Sonos transport state values like:
    /// - "PLAYING"
    /// - "PAUSED_PLAYBACK"
    /// - "STOPPED"
    /// - "TRANSITIONING"
    pub fn from_transport_state(state: &str) -> Self {
        match state.to_uppercase().as_str() {
            "PLAYING" => PlaybackState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => PlaybackState::Paused,
            "TRANSITIONING" => PlaybackState::Transitioning,
            _ => PlaybackState::Stopped,
        }
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        PlaybackState::Stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_transport_state_playing() {
        assert_eq!(
            PlaybackState::from_transport_state("PLAYING"),
            PlaybackState::Playing
        );
        assert_eq!(
            PlaybackState::from_transport_state("playing"),
            PlaybackState::Playing
        );
    }

    #[test]
    fn test_from_transport_state_paused() {
        assert_eq!(
            PlaybackState::from_transport_state("PAUSED_PLAYBACK"),
            PlaybackState::Paused
        );
        assert_eq!(
            PlaybackState::from_transport_state("PAUSED"),
            PlaybackState::Paused
        );
    }

    #[test]
    fn test_from_transport_state_stopped() {
        assert_eq!(
            PlaybackState::from_transport_state("STOPPED"),
            PlaybackState::Stopped
        );
    }

    #[test]
    fn test_from_transport_state_transitioning() {
        assert_eq!(
            PlaybackState::from_transport_state("TRANSITIONING"),
            PlaybackState::Transitioning
        );
    }

    #[test]
    fn test_from_transport_state_unknown() {
        assert_eq!(
            PlaybackState::from_transport_state("UNKNOWN"),
            PlaybackState::Stopped
        );
    }

    #[test]
    fn test_default() {
        assert_eq!(PlaybackState::default(), PlaybackState::Stopped);
    }
}
