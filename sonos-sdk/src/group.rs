//! Group handle for accessing speaker groups
//!
//! Provides access to group information and member speakers.
//! All speakers are always in a group - a single speaker forms a group of one.
//!
//! ## Write Operations and State Cache
//!
//! Write methods (e.g., `set_volume()`, `set_mute()`) update the state cache
//! optimistically after the SOAP call succeeds. The cached value may be stale
//! if the coordinator rejects the command silently, until the next UPnP event
//! corrects it.

use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::operation::{ComposableOperation, UPnPOperation, ValidationError};
use sonos_api::services::av_transport;
use sonos_api::services::group_rendering_control::{self, SetRelativeGroupVolumeResponse};
use sonos_api::SonosClient;
use sonos_state::{GroupId, GroupInfo, GroupMute, GroupVolume, SpeakerId, StateManager};

use crate::property::{GroupContext, GroupMuteHandle, GroupPropertyHandle, GroupVolumeChangeableHandle, GroupVolumeHandle};
use crate::SdkError;
use crate::Speaker;

/// Result of a multi-speaker group operation (e.g., `dissolve()`, `create_group()`)
///
/// Instead of short-circuiting on the first failure, multi-speaker operations
/// attempt every speaker and report per-speaker results. This gives callers
/// full visibility into partial failures.
#[derive(Debug)]
pub struct GroupChangeResult {
    /// Speakers that were successfully changed
    pub succeeded: Vec<SpeakerId>,
    /// Speakers that failed, with error descriptions
    pub failed: Vec<(SpeakerId, SdkError)>,
}

impl GroupChangeResult {
    /// Returns `true` if all speakers were changed successfully
    pub fn is_success(&self) -> bool {
        self.failed.is_empty()
    }

    /// Returns `true` if some speakers succeeded and some failed
    pub fn is_partial(&self) -> bool {
        !self.succeeded.is_empty() && !self.failed.is_empty()
    }
}

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

    // ========================================================================
    // GroupRenderingControl properties
    // ========================================================================
    /// Group volume (0-100)
    pub volume: GroupVolumeHandle,
    /// Group mute state
    pub mute: GroupMuteHandle,
    /// Whether group volume can be changed (event-only, no fetch)
    pub volume_changeable: GroupVolumeChangeableHandle,

    // Internal references
    coordinator_ip: IpAddr,
    state_manager: Arc<StateManager>,
    api_client: SonosClient,
}

impl Group {
    /// Create a new Group handle from GroupInfo
    ///
    /// Returns `None` if the coordinator's IP address cannot be resolved
    /// (e.g., the coordinator speaker is not registered in state).
    pub(crate) fn from_info(
        info: GroupInfo,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Option<Self> {
        let coordinator_ip = state_manager.get_speaker_ip(&info.coordinator_id)?;

        let group_context = GroupContext::new(
            info.id.clone(),
            info.coordinator_id.clone(),
            coordinator_ip,
            Arc::clone(&state_manager),
            api_client.clone(),
        );

        Some(Self {
            id: info.id,
            coordinator_id: info.coordinator_id,
            member_ids: info.member_ids,
            volume: GroupPropertyHandle::new(Arc::clone(&group_context)),
            mute: GroupPropertyHandle::new(Arc::clone(&group_context)),
            volume_changeable: GroupPropertyHandle::new(group_context),
            coordinator_ip,
            state_manager,
            api_client,
        })
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

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Execute a UPnP operation against this group's coordinator
    fn exec<Op: UPnPOperation>(
        &self,
        operation: Result<ComposableOperation<Op>, ValidationError>,
    ) -> Result<Op::Response, SdkError> {
        let op = operation?;
        self.api_client
            .execute_enhanced(&self.coordinator_ip.to_string(), op)
            .map_err(SdkError::ApiError)
    }

    // ========================================================================
    // GroupManagement — Group lifecycle
    // ========================================================================

    /// Add a speaker to this group
    ///
    /// Sends `SetAVTransportURI` to the member speaker with `x-rincon:{coordinator_id}`
    /// to join the coordinator's audio stream. This is the standard Sonos grouping mechanism.
    /// After calling this, re-fetch groups via `system.groups()` to see updated membership.
    pub fn add_speaker(&self, speaker: &Speaker) -> Result<(), SdkError> {
        if speaker.id == self.coordinator_id {
            return Err(SdkError::InvalidOperation(
                "Cannot add coordinator to its own group".to_string(),
            ));
        }
        let rincon_uri = format!("x-rincon:{}", self.coordinator_id.as_str());
        let op = av_transport::set_av_transport_uri(
            rincon_uri,
            String::new(),
        ).build()?;
        self.api_client
            .execute_enhanced::<av_transport::SetAVTransportURIOperation>(
                &speaker.ip.to_string(),
                op,
            )
            .map_err(SdkError::ApiError)?;
        Ok(())
    }

    /// Remove a speaker from this group
    ///
    /// Sends `BecomeCoordinatorOfStandaloneGroup` to the member speaker, causing it
    /// to leave the group and become standalone. Cannot remove the coordinator.
    pub fn remove_speaker(&self, speaker: &Speaker) -> Result<(), SdkError> {
        if speaker.id == self.coordinator_id {
            return Err(SdkError::InvalidOperation(
                "Cannot remove coordinator from its own group; use delegate_coordination_to() first".to_string(),
            ));
        }
        let op = av_transport::become_coordinator_of_standalone_group()
            .build()?;
        self.api_client
            .execute_enhanced::<av_transport::BecomeCoordinatorOfStandaloneGroupOperation>(
                &speaker.ip.to_string(),
                op,
            )
            .map_err(SdkError::ApiError)?;
        Ok(())
    }

    /// Dissolve this group by removing all non-coordinator members
    ///
    /// Attempts to remove every non-coordinator member, even if some fail.
    /// Returns a [`GroupChangeResult`] showing which speakers were successfully
    /// removed and which failed. For standalone groups, returns an empty result.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = group.dissolve();
    /// if !result.is_success() {
    ///     for (id, err) in &result.failed {
    ///         eprintln!("Failed to remove {}: {}", id, err);
    ///     }
    /// }
    /// ```
    pub fn dissolve(&self) -> GroupChangeResult {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for member in self.members() {
            if !self.is_coordinator(&member.id) {
                match self.remove_speaker(&member) {
                    Ok(()) => succeeded.push(member.id.clone()),
                    Err(e) => failed.push((member.id.clone(), e)),
                }
            }
        }

        GroupChangeResult { succeeded, failed }
    }

    // ========================================================================
    // GroupRenderingControl — Volume and mute
    // ========================================================================

    /// Set group volume (0-100)
    ///
    /// Updates the state cache to the new `GroupVolume` on success.
    pub fn set_volume(&self, volume: u16) -> Result<(), SdkError> {
        self.exec(group_rendering_control::set_group_volume(volume).build())?;
        self.state_manager.set_group_property(&self.id, GroupVolume(volume));
        Ok(())
    }

    /// Adjust group volume relative to current level
    ///
    /// Returns the new absolute volume.
    pub fn set_relative_volume(
        &self,
        adjustment: i16,
    ) -> Result<SetRelativeGroupVolumeResponse, SdkError> {
        let response = self.exec(group_rendering_control::set_relative_group_volume(adjustment).build())?;
        self.state_manager.set_group_property(&self.id, GroupVolume(response.new_volume));
        Ok(response)
    }

    /// Set group mute state
    ///
    /// Updates the state cache to the new `GroupMute` value on success.
    pub fn set_mute(&self, muted: bool) -> Result<(), SdkError> {
        self.exec(group_rendering_control::set_group_mute(muted).build())?;
        self.state_manager.set_group_property(&self.id, GroupMute(muted));
        Ok(())
    }

    /// Snapshot the current group volume (for later restore)
    pub fn snapshot_volume(&self) -> Result<(), SdkError> {
        self.exec(group_rendering_control::snapshot_group_volume().build())?;
        Ok(())
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

        let group = Group::from_info(group_info, state_manager, api_client).unwrap();

        assert_eq!(group.id.as_str(), "RINCON_111:1");
        assert_eq!(group.coordinator_id.as_str(), "RINCON_111");
        assert_eq!(group.member_ids.len(), 1);
    }

    #[test]
    fn test_group_from_info_returns_none_for_unknown_coordinator() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
        ]);
        let api_client = SonosClient::new();

        // Coordinator is not a registered speaker
        let group_info = GroupInfo::new(
            GroupId::new("RINCON_UNKNOWN:1"),
            SpeakerId::new("RINCON_UNKNOWN"),
            vec![SpeakerId::new("RINCON_UNKNOWN")],
        );

        let group = Group::from_info(group_info, state_manager, api_client);
        assert!(group.is_none());
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

        let group = Group::from_info(group_info, state_manager, api_client).unwrap();

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

        let group = Group::from_info(group_info, state_manager, api_client).unwrap();

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

        let group = Group::from_info(group_info, state_manager, api_client).unwrap();

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
        )
        .unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
        assert!(!grouped.is_standalone());
    }

    #[test]
    fn test_group_volume_handle_accessible() {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
        ]);
        let api_client = SonosClient::new();

        let group_info = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            SpeakerId::new("RINCON_111"),
            vec![SpeakerId::new("RINCON_111")],
        );

        let group = Group::from_info(group_info, state_manager, api_client).unwrap();

        // Volume handle should exist and return None initially
        assert!(group.volume.get().is_none());
        assert_eq!(group.volume.group_id().as_str(), "RINCON_111:1");
    }

    fn create_test_group() -> Group {
        let state_manager = create_test_state_manager_with_speakers(vec![
            ("RINCON_111", "Living Room", "192.168.1.100"),
        ]);
        let api_client = SonosClient::new();

        let group_info = GroupInfo::new(
            GroupId::new("RINCON_111:1"),
            SpeakerId::new("RINCON_111"),
            vec![SpeakerId::new("RINCON_111")],
        );

        Group::from_info(group_info, state_manager, api_client).unwrap()
    }

    #[test]
    fn test_group_set_volume_rejects_over_100() {
        let group = create_test_group();
        let result = group.set_volume(150);
        assert!(matches!(result, Err(SdkError::ValidationFailed(_))));
    }

    #[test]
    fn test_group_action_methods_exist() {
        fn assert_void(_r: Result<(), SdkError>) {}
        fn assert_response<T>(_r: Result<T, SdkError>) {}

        let group = create_test_group();

        // These will fail at network level but prove signatures compile
        assert_void(group.set_volume(50));
        assert_response::<SetRelativeGroupVolumeResponse>(group.set_relative_volume(5));
        assert_void(group.set_mute(true));
        assert_void(group.snapshot_volume());
    }

    fn create_test_group_with_member() -> (Group, Speaker) {
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

        let group = Group::from_info(group_info, Arc::clone(&state_manager), api_client.clone()).unwrap();
        let member = Speaker::new(
            SpeakerId::new("RINCON_222"),
            "Kitchen".to_string(),
            "192.168.1.101".parse().unwrap(),
            "Sonos One".to_string(),
            state_manager,
            api_client,
        );

        (group, member)
    }

    #[test]
    fn test_add_speaker_rejects_coordinator_self_add() {
        let (group, _) = create_test_group_with_member();
        let coordinator = group.coordinator().unwrap();

        let result = group.add_speaker(&coordinator);
        assert!(matches!(result, Err(SdkError::InvalidOperation(_))));
    }

    #[test]
    fn test_remove_speaker_rejects_coordinator_removal() {
        let (group, _) = create_test_group_with_member();
        let coordinator = group.coordinator().unwrap();

        let result = group.remove_speaker(&coordinator);
        assert!(matches!(result, Err(SdkError::InvalidOperation(_))));
    }

    #[test]
    fn test_group_lifecycle_methods_exist() {
        fn assert_void(_r: Result<(), SdkError>) {}
        fn assert_change_result(_r: GroupChangeResult) {}

        let (group, member) = create_test_group_with_member();

        // These will fail at network level but prove signatures compile
        assert_void(group.add_speaker(&member));
        assert_void(group.remove_speaker(&member));
        assert_change_result(group.dissolve());
    }

    #[test]
    fn test_dissolve_standalone_returns_empty_result() {
        let group = create_test_group();
        let result = group.dissolve();
        assert!(result.is_success());
        assert!(result.succeeded.is_empty());
        assert!(result.failed.is_empty());
    }
}
