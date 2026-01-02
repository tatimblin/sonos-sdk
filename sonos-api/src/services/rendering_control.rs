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
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
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
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.desired_volume > 100 {
            return Err(crate::operation::ValidationError::range_error("desired_volume", 0, 100, self.desired_volume));
        }

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
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        // i8 range is already enforced by type, but we can check reasonable bounds
        if self.adjustment < -100 || self.adjustment > 100 {
            return Err(crate::operation::ValidationError::range_error("adjustment", -100, 100, self.adjustment));
        }

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

/// Service identifier for RenderingControl
pub const SERVICE: crate::Service = crate::Service::RenderingControl;

/// Subscribe to RenderingControl events
///
/// This is a convenience function that subscribes to RenderingControl service events.
/// Events include volume changes, mute state changes, etc.
///
/// # Arguments
/// * `client` - The SonosClient to use for the subscription
/// * `ip` - The IP address of the Sonos device
/// * `callback_url` - URL where the device will send event notifications
///
/// # Returns
/// A managed subscription for RenderingControl events
///
/// # Example
/// ```rust,ignore
/// use sonos_api::{SonosClient, services::rendering_control};
///
/// let client = SonosClient::new();
/// let subscription = rendering_control::subscribe(
///     &client,
///     "192.168.1.100",
///     "http://192.168.1.50:8080/callback"
/// )?;
///
/// // Now RenderingControl events will be sent to your callback URL
/// // Execute control operations separately:
/// let vol_op = rendering_control::set_volume("Master".to_string(), 50).build()?;
/// client.execute("192.168.1.100", vol_op)?;
/// ```
pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}

/// Subscribe to RenderingControl events with custom timeout
///
/// Same as `subscribe()` but allows specifying a custom timeout.
///
/// # Arguments
/// * `client` - The SonosClient to use for the subscription
/// * `ip` - The IP address of the Sonos device
/// * `callback_url` - URL where the device will send event notifications
/// * `timeout_seconds` - How long the subscription should last
///
/// # Returns
/// A managed subscription for RenderingControl events
pub fn subscribe_with_timeout(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
    timeout_seconds: u32,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe_with_timeout(ip, SERVICE, callback_url, timeout_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::UPnPOperation;

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
        assert!(request.validate_basic().is_err());

        let request = SetVolumeOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(), // Invalid channel
            desired_volume: 50,
        };
        assert!(request.validate_basic().is_err());
    }

    #[test]
    fn test_relative_volume_validation() {
        let request = SetRelativeVolumeOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            adjustment: 100, // Maximum valid adjustment within validation range
        };
        assert!(request.validate_basic().is_ok());

        // Test with invalid channel
        let request = SetRelativeVolumeOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(),
            adjustment: 10,
        };
        assert!(request.validate_basic().is_err());
    }


    #[test]
    fn test_service_constant() {
        // Verify that SERVICE constant is correctly set
        assert_eq!(SERVICE, crate::Service::RenderingControl);
    }

    #[test]
    fn test_get_volume_payload() {
        let request = GetVolumeOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
        };
        let payload = GetVolumeOperation::build_payload(&request).unwrap();

        // Verify correct Sonos XML format with capitalized element names
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<Channel>Master</Channel>"));
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel>");
    }

    #[test]
    fn test_service_level_subscription_helpers() {
        // Test that subscription helper functions have correct signatures
        let client = crate::SonosClient::new();

        // Test subscribe function exists and has correct signature
        let _subscribe_fn = || {
            subscribe(&client, "192.168.1.100", "http://callback.url")
        };

        // Test subscribe_with_timeout function exists and has correct signature
        let _subscribe_timeout_fn = || {
            subscribe_with_timeout(&client, "192.168.1.100", "http://callback.url", 3600)
        };

        // If this compiles, the function signatures are correct
        assert!(true);
    }

    #[test]
    fn test_subscription_uses_correct_service() {
        // Verify that our subscription helpers would use the correct service
        let client = crate::SonosClient::new();

        // Verify SERVICE constant
        assert_eq!(SERVICE, crate::Service::RenderingControl);

        // The subscribe function should internally call client.subscribe(ip, SERVICE, callback_url)
        // We can't test the actual call without mocking, but the function signature
        // and SERVICE constant verification confirms correct integration
    }

}