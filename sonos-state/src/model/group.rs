//! Group and SpeakerRef types

use super::{GroupId, SpeakerId};
use serde::{Deserialize, Serialize};

/// Reference to a speaker within a group, including satellite information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerRef {
    /// Speaker ID
    id: SpeakerId,
    /// IDs of satellite speakers (for home theater setups)
    satellite_ids: Vec<SpeakerId>,
}

impl SpeakerRef {
    /// Create a new SpeakerRef
    pub fn new(id: SpeakerId, satellite_ids: Vec<SpeakerId>) -> Self {
        Self { id, satellite_ids }
    }

    /// Get the speaker ID
    pub fn get_id(&self) -> &SpeakerId {
        &self.id
    }

    /// Get satellite speaker IDs
    pub fn get_satellites(&self) -> &[SpeakerId] {
        &self.satellite_ids
    }

    /// Check if this speaker has satellites
    pub fn has_satellites(&self) -> bool {
        !self.satellite_ids.is_empty()
    }
}

/// A zone group containing one or more speakers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Unique group identifier
    id: GroupId,
    /// ID of the coordinator (master) speaker
    coordinator_id: SpeakerId,
    /// All speakers in this group
    members: Vec<SpeakerRef>,
}

impl Group {
    /// Create a new Group
    pub fn new(id: GroupId, coordinator_id: SpeakerId, members: Vec<SpeakerRef>) -> Self {
        Self {
            id,
            coordinator_id,
            members,
        }
    }

    /// Get the group ID
    pub fn get_id(&self) -> &GroupId {
        &self.id
    }

    /// Get the coordinator speaker ID
    pub fn get_coordinator_id(&self) -> &SpeakerId {
        &self.coordinator_id
    }

    /// Get all group members
    pub fn get_members(&self) -> &[SpeakerRef] {
        &self.members
    }

    /// Get number of members in this group
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Check if this is a standalone speaker (group of 1)
    pub fn is_standalone(&self) -> bool {
        self.members.len() == 1
    }

    /// Check if a speaker is in this group
    pub fn contains_speaker(&self, speaker_id: &SpeakerId) -> bool {
        self.members.iter().any(|m| m.get_id() == speaker_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_group() -> Group {
        let members = vec![
            SpeakerRef::new(SpeakerId::new("RINCON_1"), vec![]),
            SpeakerRef::new(SpeakerId::new("RINCON_2"), vec![]),
        ];
        Group::new(
            GroupId::new("RINCON_1:0"),
            SpeakerId::new("RINCON_1"),
            members,
        )
    }

    #[test]
    fn test_group_creation() {
        let group = create_test_group();
        assert_eq!(group.get_id().as_str(), "RINCON_1:0");
        assert_eq!(group.get_coordinator_id().as_str(), "RINCON_1");
        assert_eq!(group.member_count(), 2);
    }

    #[test]
    fn test_is_standalone() {
        let standalone = Group::new(
            GroupId::new("RINCON_1:0"),
            SpeakerId::new("RINCON_1"),
            vec![SpeakerRef::new(SpeakerId::new("RINCON_1"), vec![])],
        );
        assert!(standalone.is_standalone());

        let grouped = create_test_group();
        assert!(!grouped.is_standalone());
    }

    #[test]
    fn test_contains_speaker() {
        let group = create_test_group();
        assert!(group.contains_speaker(&SpeakerId::new("RINCON_1")));
        assert!(group.contains_speaker(&SpeakerId::new("RINCON_2")));
        assert!(!group.contains_speaker(&SpeakerId::new("RINCON_3")));
    }

    #[test]
    fn test_speaker_ref_satellites() {
        let speaker_ref = SpeakerRef::new(
            SpeakerId::new("RINCON_1"),
            vec![SpeakerId::new("SAT_1"), SpeakerId::new("SAT_2")],
        );
        assert!(speaker_ref.has_satellites());
        assert_eq!(speaker_ref.get_satellites().len(), 2);
    }
}
