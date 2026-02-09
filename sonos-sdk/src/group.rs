//! Group handle for accessing speaker groups
//!
//! Provides access to group information and member speakers.
//! All speakers are always in a group - a single speaker forms a group of one.

use std::sync::Arc;

use sonos_api::SonosClient;
use sonos_state::{GroupId, GroupInfo, SpeakerId, StateManager};

use crate::Speaker;

/// Group handle with access to coordinator and members
///
/// Provides access to group information and member speakers.
/// All speakers are always in a group - a single speaker forms a group of one.
///
/// # Example
///
/// ```rust,ignore
/// // Get all groups
/// for group in system.groups() {
///     println!("Group: {} ({} members)", group.id, group.member_count());
///     
///     if let Some(coordinator) = group.coordinator() {
///         println!("  Coordinator: {}", coordinator.name);
///     }
///     
///     for member in group.members() {
///         let role = if group.is_coordinator(&member.id) { "coordinator" } else { "member" };
///         println!("  - {} ({})", member.name, role);
///     }
/// }
/// ```
#[derive(Clone)]
pub struct Group {
    /// Unique group identifier
    pub id: GroupId,
    /// Coordinator speaker ID
    pub coordinator_id: SpeakerId,
    /// All member speaker IDs (including coordinator)
    pub member_ids: Vec<SpeakerId>,

    // Internal references
    state_manager: Arc<StateManager>,
    api_client: SonosClient,
}

impl Group {
    /// Create a new Group handle from GroupInfo
    pub(crate) fn from_info(
        info: GroupInfo,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        Self {
            id: info.id,
            coordinator_id: info.coordinator_id,
            member_ids: info.member_ids,
            state_manager,
            api_client,
        }
    }

    /// Get the coordinator speaker
    ///
    /// Returns the Speaker handle for the group's coordinator.
    /// Returns `None` if the coordinator speaker is not found in state.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(coordinator) = group.coordinator() {
    ///     println!("Coordinator: {}", coordinator.name);
    ///     // Control playback via coordinator
    ///     let state = coordinator.playback_state.get();
    /// }
    /// ```
    pub fn coordinator(&self) -> Option<Speaker> {
        let info = self.state_manager.speaker_info(&self.coordinator_id)?;
        Some(Speaker::new(
            self.coordinator_id.clone(),
            info.name,
            info.ip_address,
            info.model_name,
            Arc::clone(&self.state_manager),
            self.api_client.clone(),
        ))
    }

    /// Get all member speakers
    ///
    /// Returns Speaker handles for all members in the group, including the coordinator.
    /// Only returns speakers that are found in state.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for member in group.members() {
    ///     println!("Member: {} ({})", member.name, member.model_name);
    /// }
    /// ```
    pub fn members(&self) -> Vec<Speaker> {
        self.member_ids
            .iter()
            .filter_map(|id| {
                let info = self.state_manager.speaker_info(id)?;
                Some(Speaker::new(
                    id.clone(),
                    info.name,
                    info.ip_address,
                    info.model_name,
                    Arc::clone(&self.state_manager),
                    self.api_client.clone(),
                ))
            })
            .collect()
    }

    /// Check if a speaker is the coordinator of this group
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for member in group.members() {
    ///     if group.is_coordinator(&member.id) {
    ///         println!("{} is the coordinator", member.name);
    ///     }
    /// }
    /// ```
    pub fn is_coordinator(&self, speaker_id: &SpeakerId) -> bool {
        self.coordinator_id == *speaker_id
    }

    /// Get the number of members in this group
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// println!("Group has {} members", group.member_count());
    /// ```
    pub fn member_count(&self) -> usize {
        self.member_ids.len()
    }

    /// Check if this is a standalone group (single speaker)
    ///
    /// A standalone group contains only one speaker, which is also the coordinator.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if group.is_standalone() {
    ///     println!("This speaker is not grouped with others");
    /// }
    /// ```
    pub fn is_standalone(&self) -> bool {
        self.member_ids.len() == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_discovery::Device;

    fn create_test_state_manager_with_speakers(speakers: Vec<(&str, &str, &str)>) -> Arc<StateManager> {
        let manager = StateManager::new().unwrap();
        let devices: Vec<Device> = speakers
            .into_iter()
            .map(|(id, name, ip)| Device {
                id: id.to_string(),
                name: name.to_string(),
                room_name: name.to_string(),
                ip_address: ip.to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();
        manager.add_devices(devices).unwrap();
        Arc::new(manager)
    }

    #[test]
    fn test_group_from_info() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
        ]);
        let api_client = SonosClient::new();

        let group_info = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            SpeakerId::new("RINCON_111"),
            vec![SpeakerId::new("RINCON_111")],
        );

        let group = Group::from_info(group_info, state_manager, api_client);

        assert_eq!(group.id.as_str(), "RINCON_111:1");
        assert_eq!(group.coordinator_id.as_str(), "RINCON_111");
        assert_eq!(group.member_ids.len(), 1);
    }

    #[test]
    fn test_coordinator_returns_correct_speaker() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
            ("RINCON_222", "Kitchen", "192.168.1.101"),
        ]);
        let api_client = SonosClient::new();

        let group_info = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            SpeakerId::new("RINCON_111"),
            vec![SpeakerId::new("RINCON_111"), SpeakerId::new("RINCON_222")],
        );

        let group = Group::from_info(group_info, state_manager, api_client);

        let coordinator = group.coordinator();
        assert!(coordinator.is_some());
        let coordinator = coordinator.unwrap();
        assert_eq!(coordinator.id.as_str(), "RINCON_111");
        assert_eq!(coordinator.name, "Living Room");
    }

    #[test]
    fn test_members_returns_all_members() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
            ("RINCON_222", "Kitchen", "192.168.1.101"),
        ]);
        let api_client = SonosClient::new();

        let group_info = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            SpeakerId::new("RINCON_111"),
            vec![SpeakerId::new("RINCON_111"), SpeakerId::new("RINCON_222")],
        );

        let group = Group::from_info(group_info, state_manager, api_client);

        let members = group.members();
        assert_eq!(members.len(), 2);

        let member_ids: Vec<_> = members.iter().map(|m| m.id.as_str()).collect();
        assert!(member_ids.contains(&"RINCON_111"));
        assert!(member_ids.contains(&"RINCON_222"));
    }

    #[test]
    fn test_is_coordinator_returns_correct_values() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
            ("RINCON_222", "Kitchen", "192.168.1.101"),
        ]);
        let api_client = SonosClient::new();

        let group_info = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            SpeakerId::new("RINCON_111"),
            vec![SpeakerId::new("RINCON_111"), SpeakerId::new("RINCON_222")],
        );

        let group = Group::from_info(group_info, state_manager, api_client);

        assert!(group.is_coordinator(&SpeakerId::new("RINCON_111")));
        assert!(!group.is_coordinator(&SpeakerId::new("RINCON_222")));
    }

    #[test]
    fn test_member_count() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
            ("RINCON_222", "Kitchen", "192.168.1.101"),
        ]);
        let api_client = SonosClient::new();

        // Single member group
        let single_group = Group::from_info(
            GroupInfo::new(
                GroupId::new("RINCON_111:1"),
                SpeakerId::new("RINCON_111"),
                vec![SpeakerId::new("RINCON_111")],
            ),
            Arc::clone(&state_manager),
            api_client.clone(),
        );
        assert_eq!(single_group.member_count(), 1);

        // Multi-member group
        let multi_group = Group::from_info(
            GroupInfo::new(
                GroupId::new("RINCON_111:1"),
                SpeakerId::new("RINCON_111"),
                vec![SpeakerId::new("RINCON_111"), SpeakerId::new("RINCON_222")],
            ),
            state_manager,
            api_client,
        );
        assert_eq!(multi_group.member_count(), 2);
    }

    #[test]
    fn test_is_standalone() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
            ("RINCON_222", "Kitchen", "192.168.1.101"),
        ]);
        let api_client = SonosClient::new();

        // Standalone group
        let standalone = Group::from_info(
            GroupInfo::new(
                GroupId::new("RINCON_111:1"),
                SpeakerId::new("RINCON_111"),
                vec![SpeakerId::new("RINCON_111")],
            ),
            Arc::clone(&state_manager),
            api_client.clone(),
        );
        assert!(standalone.is_standalone());

        // Non-standalone group
        let grouped = Group::from_info(
            GroupInfo::new(
                GroupId::new("RINCON_111:1"),
                SpeakerId::new("RINCON_111"),
                vec![SpeakerId::new("RINCON_111"), SpeakerId::new("RINCON_222")],
            ),
            state_manager,
            api_client,
        );
        assert!(!grouped.is_standalone());
    }
}
