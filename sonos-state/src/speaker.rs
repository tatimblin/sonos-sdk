//! Speaker-related types and utilities
//!
//! This module provides speaker-related functionality for the sonos-state crate.
//! The main Speaker handle with property accessors is provided by sonos-sdk.

#[cfg(test)]
mod tests {
    use crate::model::{SpeakerId, SpeakerInfo};
    use crate::property::{Property, Volume, Mute, PlaybackState};

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
    fn test_speaker_info_debug() {
        let info = create_test_speaker_info();
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("Living Room"));
        assert!(debug_str.contains("RINCON_123"));
    }

    #[test]
    fn test_property_keys() {
        // Verify property keys are correct
        assert_eq!(Volume::KEY, "volume");
        assert_eq!(Mute::KEY, "mute");
        assert_eq!(PlaybackState::KEY, "playback_state");
    }
}
