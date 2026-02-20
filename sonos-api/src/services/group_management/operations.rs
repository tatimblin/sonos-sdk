//! GroupManagement service operations
//!
//! This module provides operations for managing speaker group membership
//! on Sonos speaker groups. All operations should be sent to the group coordinator only.
//!
//! # Operations
//! - `add_member` - Add a speaker to the group
//! - `remove_member` - Remove a speaker from the group
//! - `report_track_buffering_result` - Report track buffering status
//! - `set_source_area_ids` - Set source area identifiers

use crate::{define_upnp_operation, Validate};
use paste::paste;
use serde::{Deserialize, Serialize};

// =============================================================================
// ADD MEMBER OPERATION (Manual implementation due to boolean response field)
// =============================================================================

/// Request to add a member to the group
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddMemberOperationRequest {
    /// The member ID (RINCON format UUID) of the speaker to add
    pub member_id: String,
    /// The boot sequence number of the speaker
    pub boot_seq: u32,
}

impl Validate for AddMemberOperationRequest {}

/// Response from adding a member to the group
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AddMemberResponse {
    /// Current transport settings for the group
    pub current_transport_settings: String,
    /// Current URI being played
    pub current_uri: String,
    /// UUID of the group that was joined
    pub group_uuid_joined: String,
    /// Whether to reset volume after joining
    pub reset_volume_after: bool,
    /// Volume AV transport URI
    pub volume_av_transport_uri: String,
}

/// Operation to add a member to a speaker group
pub struct AddMemberOperation;

impl crate::operation::UPnPOperation for AddMemberOperation {
    type Request = AddMemberOperationRequest;
    type Response = AddMemberResponse;

    const SERVICE: crate::service::Service = crate::service::Service::GroupManagement;
    const ACTION: &'static str = "AddMember";

    fn build_payload(request: &Self::Request) -> Result<String, crate::operation::ValidationError> {
        <Self::Request as Validate>::validate(request, crate::operation::ValidationLevel::Basic)?;
        Ok(format!(
            "<MemberID>{}</MemberID><BootSeq>{}</BootSeq>",
            request.member_id, request.boot_seq
        ))
    }

    fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, crate::error::ApiError> {
        let current_transport_settings = xml
            .get_child("CurrentTransportSettings")
            .and_then(|e| e.get_text())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let current_uri = xml
            .get_child("CurrentURI")
            .and_then(|e| e.get_text())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let group_uuid_joined = xml
            .get_child("GroupUUIDJoined")
            .and_then(|e| e.get_text())
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Parse "1" as true, "0" as false for ResetVolumeAfter
        let reset_volume_after = xml
            .get_child("ResetVolumeAfter")
            .and_then(|e| e.get_text())
            .map(|s| s.as_ref() == "1")
            .unwrap_or(false);

        let volume_av_transport_uri = xml
            .get_child("VolumeAVTransportURI")
            .and_then(|e| e.get_text())
            .map(|s| s.to_string())
            .unwrap_or_default();

        Ok(AddMemberResponse {
            current_transport_settings,
            current_uri,
            group_uuid_joined,
            reset_volume_after,
            volume_av_transport_uri,
        })
    }
}

/// Create an AddMember operation builder
pub fn add_member_operation(
    member_id: String,
    boot_seq: u32,
) -> crate::operation::OperationBuilder<AddMemberOperation> {
    let request = AddMemberOperationRequest { member_id, boot_seq };
    crate::operation::OperationBuilder::new(request)
}

// =============================================================================
// REMOVE MEMBER OPERATION
// =============================================================================

define_upnp_operation! {
    operation: RemoveMemberOperation,
    action: "RemoveMember",
    service: GroupManagement,
    request: {
        member_id: String,
    },
    response: (),
    payload: |req| {
        format!("<InstanceID>{}</InstanceID><MemberID>{}</MemberID>", req.instance_id, req.member_id)
    },
    parse: |_xml| Ok(()),
}

impl Validate for RemoveMemberOperationRequest {}

// =============================================================================
// REPORT TRACK BUFFERING RESULT OPERATION
// =============================================================================

define_upnp_operation! {
    operation: ReportTrackBufferingResultOperation,
    action: "ReportTrackBufferingResult",
    service: GroupManagement,
    request: {
        member_id: String,
        result_code: i32,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><MemberID>{}</MemberID><ResultCode>{}</ResultCode>",
            req.instance_id, req.member_id, req.result_code
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for ReportTrackBufferingResultOperationRequest {}

// =============================================================================
// SET SOURCE AREA IDS OPERATION
// =============================================================================

define_upnp_operation! {
    operation: SetSourceAreaIdsOperation,
    action: "SetSourceAreaIds",
    service: GroupManagement,
    request: {
        desired_source_area_ids: String,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><DesiredSourceAreaIds>{}</DesiredSourceAreaIds>",
            req.instance_id, req.desired_source_area_ids
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetSourceAreaIdsOperationRequest {}

// =============================================================================
// LEGACY ALIASES
// =============================================================================

pub use add_member_operation as add_member;
pub use remove_member_operation as remove_member;
pub use report_track_buffering_result_operation as report_track_buffering_result;
pub use set_source_area_ids_operation as set_source_area_ids;

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::UPnPOperation;

    // --- AddMember Tests ---

    #[test]
    fn test_add_member_builder() {
        let op = add_member_operation("RINCON_123".to_string(), 42)
            .build()
            .unwrap();
        assert_eq!(op.request().member_id, "RINCON_123");
        assert_eq!(op.request().boot_seq, 42);
        assert_eq!(op.metadata().action, "AddMember");
        assert_eq!(op.metadata().service, "GroupManagement");
    }

    #[test]
    fn test_add_member_payload() {
        let request = AddMemberOperationRequest {
            member_id: "RINCON_ABC123".to_string(),
            boot_seq: 100,
        };
        let payload = AddMemberOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<MemberID>RINCON_ABC123</MemberID>"));
        assert!(payload.contains("<BootSeq>100</BootSeq>"));
    }

    #[test]
    fn test_add_member_response_parsing_reset_volume_true() {
        let xml_str = r#"<AddMemberResponse>
            <CurrentTransportSettings>settings</CurrentTransportSettings>
            <CurrentURI>x-rincon:RINCON_123</CurrentURI>
            <GroupUUIDJoined>group-uuid-123</GroupUUIDJoined>
            <ResetVolumeAfter>1</ResetVolumeAfter>
            <VolumeAVTransportURI>x-rincon:RINCON_456</VolumeAVTransportURI>
        </AddMemberResponse>"#;
        let xml = xmltree::Element::parse(xml_str.as_bytes()).unwrap();
        let response = AddMemberOperation::parse_response(&xml).unwrap();
        
        assert_eq!(response.current_transport_settings, "settings");
        assert_eq!(response.current_uri, "x-rincon:RINCON_123");
        assert_eq!(response.group_uuid_joined, "group-uuid-123");
        assert!(response.reset_volume_after);
        assert_eq!(response.volume_av_transport_uri, "x-rincon:RINCON_456");
    }

    #[test]
    fn test_add_member_response_parsing_reset_volume_false() {
        let xml_str = r#"<AddMemberResponse>
            <CurrentTransportSettings></CurrentTransportSettings>
            <CurrentURI></CurrentURI>
            <GroupUUIDJoined></GroupUUIDJoined>
            <ResetVolumeAfter>0</ResetVolumeAfter>
            <VolumeAVTransportURI></VolumeAVTransportURI>
        </AddMemberResponse>"#;
        let xml = xmltree::Element::parse(xml_str.as_bytes()).unwrap();
        let response = AddMemberOperation::parse_response(&xml).unwrap();
        
        assert!(!response.reset_volume_after);
    }

    // --- RemoveMember Tests ---

    #[test]
    fn test_remove_member_builder() {
        let op = remove_member_operation("RINCON_456".to_string())
            .build()
            .unwrap();
        assert_eq!(op.request().member_id, "RINCON_456");
        assert_eq!(op.metadata().action, "RemoveMember");
        assert_eq!(op.metadata().service, "GroupManagement");
    }

    #[test]
    fn test_remove_member_payload() {
        let request = RemoveMemberOperationRequest {
            member_id: "RINCON_XYZ".to_string(),
            instance_id: 0,
        };
        let payload = RemoveMemberOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<MemberID>RINCON_XYZ</MemberID>"));
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
    }

    // --- ReportTrackBufferingResult Tests ---

    #[test]
    fn test_report_track_buffering_result_builder() {
        let op = report_track_buffering_result_operation("RINCON_789".to_string(), 0)
            .build()
            .unwrap();
        assert_eq!(op.request().member_id, "RINCON_789");
        assert_eq!(op.request().result_code, 0);
        assert_eq!(op.metadata().action, "ReportTrackBufferingResult");
        assert_eq!(op.metadata().service, "GroupManagement");
    }

    #[test]
    fn test_report_track_buffering_result_payload() {
        let request = ReportTrackBufferingResultOperationRequest {
            member_id: "RINCON_ABC".to_string(),
            result_code: -1,
            instance_id: 0,
        };
        let payload = ReportTrackBufferingResultOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<MemberID>RINCON_ABC</MemberID>"));
        assert!(payload.contains("<ResultCode>-1</ResultCode>"));
    }

    // --- SetSourceAreaIds Tests ---

    #[test]
    fn test_set_source_area_ids_builder() {
        let op = set_source_area_ids_operation("area1,area2".to_string())
            .build()
            .unwrap();
        assert_eq!(op.request().desired_source_area_ids, "area1,area2");
        assert_eq!(op.metadata().action, "SetSourceAreaIds");
        assert_eq!(op.metadata().service, "GroupManagement");
    }

    #[test]
    fn test_set_source_area_ids_payload() {
        let request = SetSourceAreaIdsOperationRequest {
            desired_source_area_ids: "source-area-123".to_string(),
            instance_id: 0,
        };
        let payload = SetSourceAreaIdsOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<DesiredSourceAreaIds>source-area-123</DesiredSourceAreaIds>"));
    }

    // --- SERVICE constant test ---

    #[test]
    fn test_service_constant() {
        assert_eq!(AddMemberOperation::SERVICE, crate::service::Service::GroupManagement);
        assert_eq!(RemoveMemberOperation::SERVICE, crate::service::Service::GroupManagement);
        assert_eq!(ReportTrackBufferingResultOperation::SERVICE, crate::service::Service::GroupManagement);
        assert_eq!(SetSourceAreaIdsOperation::SERVICE, crate::service::Service::GroupManagement);
    }
}


// =============================================================================
// PROPERTY-BASED TESTS
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::operation::{UPnPOperation, ValidationLevel};
    use proptest::prelude::*;

    // =========================================================================
    // Property 1: AddMember boolean response parsing
    // =========================================================================
    // *For any* AddMember XML response containing ResetVolumeAfter with value "1",
    // parsing SHALL return `reset_volume_after: true`, and for value "0",
    // parsing SHALL return `reset_volume_after: false`.
    // **Validates: Requirements 1.5**
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Feature: group-management, Property 1: AddMember boolean response parsing
        #[test]
        fn prop_add_member_bool_parsing(reset_vol in proptest::bool::ANY) {
            let xml_value = if reset_vol { "1" } else { "0" };
            let xml_str = format!(r#"<AddMemberResponse>
                <CurrentTransportSettings>test-settings</CurrentTransportSettings>
                <CurrentURI>x-rincon:RINCON_TEST</CurrentURI>
                <GroupUUIDJoined>test-group-uuid</GroupUUIDJoined>
                <ResetVolumeAfter>{}</ResetVolumeAfter>
                <VolumeAVTransportURI>x-rincon:RINCON_VOL</VolumeAVTransportURI>
            </AddMemberResponse>"#, xml_value);

            let xml = xmltree::Element::parse(xml_str.as_bytes())
                .expect("XML should parse successfully");
            let response = AddMemberOperation::parse_response(&xml)
                .expect("Response parsing should succeed");

            prop_assert_eq!(
                response.reset_volume_after,
                reset_vol,
                "ResetVolumeAfter '{}' should parse to {}",
                xml_value,
                reset_vol
            );
        }
    }

    // =========================================================================
    // Property 2: Void operations always pass validation
    // =========================================================================
    // *For any* RemoveMemberOperationRequest, ReportTrackBufferingResultOperationRequest,
    // or SetSourceAreaIdsOperationRequest with valid string/integer field values,
    // validation SHALL succeed (return Ok).
    // **Validates: Requirements 2.4, 3.4, 4.4**
    // =========================================================================

    /// Strategy for generating arbitrary member IDs
    fn member_id_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[A-Za-z0-9_-]{0,50}")
            .unwrap()
    }

    /// Strategy for generating arbitrary source area IDs
    fn source_area_ids_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[A-Za-z0-9,_-]{0,100}")
            .unwrap()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Feature: group-management, Property 2: Void operations always pass validation (RemoveMember)
        #[test]
        fn prop_remove_member_validation_passes(member_id in member_id_strategy()) {
            let request = RemoveMemberOperationRequest {
                member_id,
                instance_id: 0,
            };
            let result = <RemoveMemberOperationRequest as Validate>::validate(&request, ValidationLevel::Basic);
            prop_assert!(
                result.is_ok(),
                "RemoveMember validation should always pass, got: {:?}",
                result
            );
        }

        /// Feature: group-management, Property 2: Void operations always pass validation (ReportTrackBufferingResult)
        #[test]
        fn prop_report_track_buffering_result_validation_passes(
            member_id in member_id_strategy(),
            result_code in prop::num::i32::ANY,
        ) {
            let request = ReportTrackBufferingResultOperationRequest {
                member_id,
                result_code,
                instance_id: 0,
            };
            let result = <ReportTrackBufferingResultOperationRequest as Validate>::validate(&request, ValidationLevel::Basic);
            prop_assert!(
                result.is_ok(),
                "ReportTrackBufferingResult validation should always pass, got: {:?}",
                result
            );
        }

        /// Feature: group-management, Property 2: Void operations always pass validation (SetSourceAreaIds)
        #[test]
        fn prop_set_source_area_ids_validation_passes(
            desired_source_area_ids in source_area_ids_strategy(),
        ) {
            let request = SetSourceAreaIdsOperationRequest {
                desired_source_area_ids,
                instance_id: 0,
            };
            let result = <SetSourceAreaIdsOperationRequest as Validate>::validate(&request, ValidationLevel::Basic);
            prop_assert!(
                result.is_ok(),
                "SetSourceAreaIds validation should always pass, got: {:?}",
                result
            );
        }
    }
}
