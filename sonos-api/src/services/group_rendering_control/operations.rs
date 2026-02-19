//! GroupRenderingControl service operations
//!
//! This module provides operations for controlling group-wide audio rendering settings
//! on Sonos speaker groups. All operations should be sent to the group coordinator only.
//!
//! # Operations
//! - `get_group_volume` - Get the current group volume level
//! - `set_group_volume` - Set the group volume level (0-100)
//! - `set_relative_group_volume` - Adjust group volume relatively (-100 to +100)
//! - `get_group_mute` - Get the current group mute state
//! - `set_group_mute` - Set the group mute state
//! - `snapshot_group_volume` - Snapshot volume ratios for proportional changes

use crate::{define_operation_with_response, define_upnp_operation, Validate};
use paste::paste;

// =============================================================================
// GET GROUP VOLUME
// =============================================================================

define_operation_with_response! {
    operation: GetGroupVolumeOperation,
    action: "GetGroupVolume",
    service: GroupRenderingControl,
    request: {},
    response: GetGroupVolumeResponse {
        current_volume: u16,
    },
    xml_mapping: {
        current_volume: "CurrentVolume",
    },
}

impl Validate for GetGroupVolumeOperationRequest {}

pub use get_group_volume_operation as get_group_volume;

// =============================================================================
// SET GROUP VOLUME
// =============================================================================

define_upnp_operation! {
    operation: SetGroupVolumeOperation,
    action: "SetGroupVolume",
    service: GroupRenderingControl,
    request: {
        desired_volume: u16,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><DesiredVolume>{}</DesiredVolume>",
            req.instance_id, req.desired_volume
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetGroupVolumeOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.desired_volume > 100 {
            return Err(crate::operation::ValidationError::range_error(
                "desired_volume",
                0,
                100,
                self.desired_volume,
            ));
        }
        Ok(())
    }
}

pub use set_group_volume_operation as set_group_volume;

// =============================================================================
// SET RELATIVE GROUP VOLUME
// =============================================================================

define_operation_with_response! {
    operation: SetRelativeGroupVolumeOperation,
    action: "SetRelativeGroupVolume",
    service: GroupRenderingControl,
    request: {
        adjustment: i16,
    },
    response: SetRelativeGroupVolumeResponse {
        new_volume: u16,
    },
    xml_mapping: {
        new_volume: "NewVolume",
    },
}

impl Validate for SetRelativeGroupVolumeOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.adjustment < -100 || self.adjustment > 100 {
            return Err(crate::operation::ValidationError::range_error(
                "adjustment",
                -100,
                100,
                self.adjustment,
            ));
        }
        Ok(())
    }
}

pub use set_relative_group_volume_operation as set_relative_group_volume;

// =============================================================================
// GET GROUP MUTE
// =============================================================================

define_operation_with_response! {
    operation: GetGroupMuteOperation,
    action: "GetGroupMute",
    service: GroupRenderingControl,
    request: {},
    response: GetGroupMuteResponse {
        current_mute: bool,
    },
    xml_mapping: {
        current_mute: "CurrentMute",
    },
}

impl Validate for GetGroupMuteOperationRequest {}

pub use get_group_mute_operation as get_group_mute;

// =============================================================================
// SET GROUP MUTE
// =============================================================================

define_upnp_operation! {
    operation: SetGroupMuteOperation,
    action: "SetGroupMute",
    service: GroupRenderingControl,
    request: {
        desired_mute: bool,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><DesiredMute>{}</DesiredMute>",
            req.instance_id,
            if req.desired_mute { "1" } else { "0" }
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetGroupMuteOperationRequest {}

pub use set_group_mute_operation as set_group_mute;

// =============================================================================
// SNAPSHOT GROUP VOLUME
// =============================================================================

define_upnp_operation! {
    operation: SnapshotGroupVolumeOperation,
    action: "SnapshotGroupVolume",
    service: GroupRenderingControl,
    request: {},
    response: (),
    payload: |req| {
        format!("<InstanceID>{}</InstanceID>", req.instance_id)
    },
    parse: |_xml| Ok(()),
}

impl Validate for SnapshotGroupVolumeOperationRequest {}

pub use snapshot_group_volume_operation as snapshot_group_volume;

// =============================================================================
// SERVICE CONSTANT AND SUBSCRIPTION HELPERS
// =============================================================================

/// Service identifier for GroupRenderingControl
pub const SERVICE: crate::Service = crate::Service::GroupRenderingControl;

/// Subscribe to GroupRenderingControl events
pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}

/// Subscribe to GroupRenderingControl events with custom timeout
pub fn subscribe_with_timeout(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
    timeout_seconds: u32,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe_with_timeout(ip, SERVICE, callback_url, timeout_seconds)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::UPnPOperation;

    #[test]
    fn test_get_group_volume_builder() {
        let op = get_group_volume().build().unwrap();
        assert_eq!(op.metadata().action, "GetGroupVolume");
        assert_eq!(op.request().instance_id, 0);
    }

    #[test]
    fn test_set_group_volume_builder() {
        let op = set_group_volume(75).build().unwrap();
        assert_eq!(op.request().desired_volume, 75);
        assert_eq!(op.request().instance_id, 0);
        assert_eq!(op.metadata().action, "SetGroupVolume");
    }

    #[test]
    fn test_set_relative_group_volume_builder() {
        let op = set_relative_group_volume(10).build().unwrap();
        assert_eq!(op.request().adjustment, 10);
        assert_eq!(op.request().instance_id, 0);
        assert_eq!(op.metadata().action, "SetRelativeGroupVolume");
    }

    #[test]
    fn test_get_group_mute_builder() {
        let op = get_group_mute().build().unwrap();
        assert_eq!(op.metadata().action, "GetGroupMute");
        assert_eq!(op.request().instance_id, 0);
    }

    #[test]
    fn test_set_group_mute_builder() {
        let op = set_group_mute(true).build().unwrap();
        assert!(op.request().desired_mute);
        assert_eq!(op.request().instance_id, 0);
        assert_eq!(op.metadata().action, "SetGroupMute");
    }

    #[test]
    fn test_snapshot_group_volume_builder() {
        let op = snapshot_group_volume().build().unwrap();
        assert_eq!(op.metadata().action, "SnapshotGroupVolume");
        assert_eq!(op.request().instance_id, 0);
    }

    #[test]
    fn test_get_group_volume_payload() {
        let request = GetGroupVolumeOperationRequest { instance_id: 0 };
        let payload = GetGroupVolumeOperation::build_payload(&request).unwrap();
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_set_group_volume_payload() {
        let request = SetGroupVolumeOperationRequest {
            instance_id: 0,
            desired_volume: 75,
        };
        let payload = SetGroupVolumeOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<DesiredVolume>75</DesiredVolume>"));
    }

    #[test]
    fn test_set_relative_group_volume_payload() {
        let request = SetRelativeGroupVolumeOperationRequest {
            instance_id: 0,
            adjustment: -25,
        };
        let payload = SetRelativeGroupVolumeOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<Adjustment>-25</Adjustment>"));
    }

    #[test]
    fn test_get_group_mute_payload() {
        let request = GetGroupMuteOperationRequest { instance_id: 0 };
        let payload = GetGroupMuteOperation::build_payload(&request).unwrap();
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_set_group_mute_payload_true() {
        let request = SetGroupMuteOperationRequest {
            instance_id: 0,
            desired_mute: true,
        };
        let payload = SetGroupMuteOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<DesiredMute>1</DesiredMute>"));
    }

    #[test]
    fn test_set_group_mute_payload_false() {
        let request = SetGroupMuteOperationRequest {
            instance_id: 0,
            desired_mute: false,
        };
        let payload = SetGroupMuteOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<DesiredMute>0</DesiredMute>"));
    }

    #[test]
    fn test_snapshot_group_volume_payload() {
        let request = SnapshotGroupVolumeOperationRequest { instance_id: 0 };
        let payload = SnapshotGroupVolumeOperation::build_payload(&request).unwrap();
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_set_group_volume_rejects_over_100() {
        let request = SetGroupVolumeOperationRequest {
            instance_id: 0,
            desired_volume: 101,
        };
        assert!(request.validate_basic().is_err());
    }

    #[test]
    fn test_set_group_volume_accepts_boundary_values() {
        // Test volume = 0
        let request = SetGroupVolumeOperationRequest {
            instance_id: 0,
            desired_volume: 0,
        };
        assert!(request.validate_basic().is_ok());

        // Test volume = 100
        let request = SetGroupVolumeOperationRequest {
            instance_id: 0,
            desired_volume: 100,
        };
        assert!(request.validate_basic().is_ok());
    }

    #[test]
    fn test_set_relative_group_volume_rejects_under_minus_100() {
        let request = SetRelativeGroupVolumeOperationRequest {
            instance_id: 0,
            adjustment: -101,
        };
        assert!(request.validate_basic().is_err());
    }

    #[test]
    fn test_set_relative_group_volume_rejects_over_100() {
        let request = SetRelativeGroupVolumeOperationRequest {
            instance_id: 0,
            adjustment: 101,
        };
        assert!(request.validate_basic().is_err());
    }

    #[test]
    fn test_set_relative_group_volume_accepts_boundary_values() {
        // Test adjustment = -100
        let request = SetRelativeGroupVolumeOperationRequest {
            instance_id: 0,
            adjustment: -100,
        };
        assert!(request.validate_basic().is_ok());

        // Test adjustment = 0
        let request = SetRelativeGroupVolumeOperationRequest {
            instance_id: 0,
            adjustment: 0,
        };
        assert!(request.validate_basic().is_ok());

        // Test adjustment = 100
        let request = SetRelativeGroupVolumeOperationRequest {
            instance_id: 0,
            adjustment: 100,
        };
        assert!(request.validate_basic().is_ok());
    }

    #[test]
    fn test_service_constant() {
        assert_eq!(SERVICE, crate::Service::GroupRenderingControl);
    }

    #[test]
    fn test_subscribe_function_signature() {
        let client = crate::SonosClient::new();
        // Verify subscribe function has correct signature (compiles)
        let _subscribe_fn = || subscribe(&client, "192.168.1.100", "http://callback.url");
        assert!(true);
    }

    #[test]
    fn test_subscribe_with_timeout_function_signature() {
        let client = crate::SonosClient::new();
        // Verify subscribe_with_timeout function has correct signature (compiles)
        let _subscribe_fn =
            || subscribe_with_timeout(&client, "192.168.1.100", "http://callback.url", 3600);
        assert!(true);
    }
}
