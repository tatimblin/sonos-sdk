//! State snapshot for efficient read-only access

use crate::{Group, GroupId, SpeakerId, SpeakerState};
use std::collections::HashMap;

/// A snapshot of the current state that provides efficient read-only access
/// to speakers and groups without holding locks.
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    /// All speaker states
    pub speakers: HashMap<SpeakerId, SpeakerState>,
    /// All groups
    pub groups: HashMap<GroupId, Group>,
}

impl StateSnapshot {
    /// Create a new empty snapshot
    pub fn new() -> Self {
        Self {
            speakers: HashMap::new(),
            groups: HashMap::new(),
        }
    }

    /// Get all speakers as an iterator of references
    pub fn speakers(&self) -> impl Iterator<Item = &SpeakerState> {
        self.speakers.values()
    }

    /// Get all groups as an iterator of references
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    /// Get a specific speaker by ID
    pub fn get_speaker(&self, id: &SpeakerId) -> Option<&SpeakerState> {
        self.speakers.get(id)
    }

    /// Get a specific group by ID
    pub fn get_group(&self, id: &GroupId) -> Option<&Group> {
        self.groups.get(id)
    }

    /// Get speakers in a specific group
    pub fn speakers_in_group(&self, group_id: &GroupId) -> impl Iterator<Item = &SpeakerState> + '_ {
        let group_id = group_id.clone();
        self.speakers
            .values()
            .filter(move |s| s.group_id.as_ref() == Some(&group_id))
    }

    /// Get the coordinator of a group
    pub fn group_coordinator(&self, group_id: &GroupId) -> Option<&SpeakerState> {
        self.speakers.values().find(|s| {
            s.group_id.as_ref() == Some(group_id) && s.is_coordinator
        })
    }

    /// Get speakers by room name
    pub fn speakers_by_room(&self, room_name: &str) -> impl Iterator<Item = &SpeakerState> + '_ {
        let room_name = room_name.to_string();
        self.speakers
            .values()
            .filter(move |s| s.speaker.room_name == room_name)
    }

    /// Get speaker by name
    pub fn speaker_by_name(&self, name: &str) -> Option<&SpeakerState> {
        self.speakers.values().find(|s| s.speaker.name == name)
    }

    /// Get total number of speakers
    pub fn speaker_count(&self) -> usize {
        self.speakers.len()
    }

    /// Get total number of groups
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Check if the snapshot is empty
    pub fn is_empty(&self) -> bool {
        self.speakers.is_empty()
    }
}

impl Default for StateSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlaybackState, Speaker, SpeakerRef};
    use std::net::IpAddr;

    fn create_test_snapshot() -> StateSnapshot {
        let mut speakers = HashMap::new();
        let mut groups = HashMap::new();

        let speaker1 = Speaker {
            id: SpeakerId::new("RINCON_1"),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".parse::<IpAddr>().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "56.0".to_string(),
            satellites: vec![],
        };

        let speaker2 = Speaker {
            id: SpeakerId::new("RINCON_2"),
            name: "Kitchen".to_string(),
            room_name: "Kitchen".to_string(),
            ip_address: "192.168.1.101".parse::<IpAddr>().unwrap(),
            port: 1400,
            model_name: "Sonos Play:1".to_string(),
            software_version: "56.0".to_string(),
            satellites: vec![],
        };

        let mut state1 = SpeakerState::new(speaker1);
        state1.group_id = Some(GroupId::new("RINCON_1:0"));
        state1.is_coordinator = true;
        state1.playback_state = PlaybackState::Playing;

        let mut state2 = SpeakerState::new(speaker2);
        state2.group_id = Some(GroupId::new("RINCON_1:0"));
        state2.is_coordinator = false;

        speakers.insert(SpeakerId::new("RINCON_1"), state1);
        speakers.insert(SpeakerId::new("RINCON_2"), state2);

        let group = Group::new(
            GroupId::new("RINCON_1:0"),
            SpeakerId::new("RINCON_1"),
            vec![
                SpeakerRef::new(SpeakerId::new("RINCON_1"), vec![]),
                SpeakerRef::new(SpeakerId::new("RINCON_2"), vec![]),
            ],
        );
        groups.insert(GroupId::new("RINCON_1:0"), group);

        StateSnapshot { speakers, groups }
    }

    #[test]
    fn test_get_speaker() {
        let snapshot = create_test_snapshot();
        let speaker = snapshot.get_speaker(&SpeakerId::new("RINCON_1"));
        assert!(speaker.is_some());
        assert_eq!(speaker.unwrap().speaker.name, "Living Room");
    }

    #[test]
    fn test_speakers_in_group() {
        let snapshot = create_test_snapshot();
        let speakers: Vec<_> = snapshot
            .speakers_in_group(&GroupId::new("RINCON_1:0"))
            .collect();
        assert_eq!(speakers.len(), 2);
    }

    #[test]
    fn test_group_coordinator() {
        let snapshot = create_test_snapshot();
        let coordinator = snapshot.group_coordinator(&GroupId::new("RINCON_1:0"));
        assert!(coordinator.is_some());
        assert_eq!(coordinator.unwrap().speaker.name, "Living Room");
    }

    #[test]
    fn test_speaker_by_name() {
        let snapshot = create_test_snapshot();
        let speaker = snapshot.speaker_by_name("Kitchen");
        assert!(speaker.is_some());
        assert_eq!(speaker.unwrap().get_id().as_str(), "RINCON_2");
    }

    #[test]
    fn test_counts() {
        let snapshot = create_test_snapshot();
        assert_eq!(snapshot.speaker_count(), 2);
        assert_eq!(snapshot.group_count(), 1);
        assert!(!snapshot.is_empty());
    }
}
