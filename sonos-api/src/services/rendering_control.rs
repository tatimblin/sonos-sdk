use crate::{define_upnp_operation, define_operation_with_response, Validate};
use paste::paste;

// Operation with complex response parsing and channel validation
define_operation_with_response! {
    operation: GetVolumeOperation,
    action: "GetVolume",
    service: RenderingControl,
    request: {
        channel: String,
    },
    response: GetVolumeResponse {
        current_volume: u8,
    },
    xml_mapping: {
        current_volume: "CurrentVolume",
    },
}

// Custom validation implementation for GetVolumeOperation (channel validation)
impl Validate for GetVolumeOperationRequest {
    fn validate_comprehensive(&self) -> Result<(), crate::operation::ValidationError> {
        // Validate channel names
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}

// Operation with volume range validation and channel validation
define_upnp_operation! {
    operation: SetVolumeOperation,
    action: "SetVolume",
    service: RenderingControl,
    request: {
        channel: String,
        desired_volume: u8,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel><DesiredVolume>{}</DesiredVolume>",
            req.instance_id, req.channel, req.desired_volume
        )
    },
    parse: |_xml| Ok(()),
}

// Custom validation implementation for SetVolumeOperation (range + channel validation)
impl Validate for SetVolumeOperationRequest {
    fn validate_boundary(&self) -> Result<(), crate::operation::ValidationError> {
        if self.desired_volume > 100 {
            return Err(crate::operation::ValidationError::range_error("desired_volume", 0, 100, self.desired_volume));
        }
        Ok(())
    }

    fn validate_comprehensive(&self) -> Result<(), crate::operation::ValidationError> {
        self.validate_boundary()?;

        // Validate channel names
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}

// Operation with adjustment range validation, channel validation, and response parsing
define_operation_with_response! {
    operation: SetRelativeVolumeOperation,
    action: "SetRelativeVolume",
    service: RenderingControl,
    request: {
        channel: String,
        adjustment: i8,
    },
    response: SetRelativeVolumeResponse {
        new_volume: u8,
    },
    xml_mapping: {
        new_volume: "NewVolume",
    },
}

// Custom validation implementation for SetRelativeVolumeOperation (range + channel validation)
impl Validate for SetRelativeVolumeOperationRequest {
    fn validate_boundary(&self) -> Result<(), crate::operation::ValidationError> {
        // i8 range is already enforced by type, but we can check reasonable bounds
        if self.adjustment < -100 || self.adjustment > 100 {
            return Err(crate::operation::ValidationError::range_error("adjustment", -100, 100, self.adjustment));
        }
        Ok(())
    }

    fn validate_comprehensive(&self) -> Result<(), crate::operation::ValidationError> {
        self.validate_boundary()?;

        // Validate channel names
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}

// The macros automatically generate the convenience functions:
// - get_volume_operation()
// - set_volume_operation()
// - set_relative_volume_operation()

// Legacy convenience functions for backward compatibility
pub use get_volume_operation as get_volume;
pub use set_volume_operation as set_volume;
pub use set_relative_volume_operation as set_relative_volume;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{ValidationLevel, UPnPOperation};

    #[test]
    fn test_volume_operations() {
        let get_vol = get_volume_operation("Master".to_string()).build().unwrap();
        assert_eq!(get_vol.request().channel, "Master");

        let set_vol = set_volume_operation("Master".to_string(), 75).build().unwrap();
        assert_eq!(set_vol.request().desired_volume, 75);
    }

    #[test]
    fn test_volume_validation() {
        let request = SetVolumeOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_volume: 150, // Invalid volume
        };
        assert!(request.validate_boundary().is_err());

        let request = SetVolumeOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(), // Invalid channel
            desired_volume: 50,
        };
        assert!(request.validate_comprehensive().is_err());
    }

    #[test]
    fn test_relative_volume_validation() {
        let request = SetRelativeVolumeOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            adjustment: 100, // Maximum valid adjustment within validation range
        };
        assert!(request.validate_boundary().is_ok());

        // Test with invalid channel
        let request = SetRelativeVolumeOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(),
            adjustment: 10,
        };
        assert!(request.validate_comprehensive().is_err());
    }

    #[test]
    fn test_volume_operations_composition() {
        let get_vol = get_volume_operation("Master".to_string()).build().unwrap();
        let set_vol = set_volume_operation("Master".to_string(), 50).build().unwrap();

        // Test that volume operations can be chained
        let sequence = get_vol.and_then(set_vol);
        let (first, second) = sequence.operations();

        assert_eq!(first.metadata().action, "GetVolume");
        assert_eq!(second.metadata().action, "SetVolume");
    }
}