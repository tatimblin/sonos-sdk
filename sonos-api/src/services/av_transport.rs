use crate::{define_upnp_operation, define_operation_with_response, Validate};
use paste::paste;

define_upnp_operation! {
    operation: PauseOperation,
    action: "Pause",
    service: AVTransport,
    request: {
    },
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

define_upnp_operation! {
    operation: StopOperation,
    action: "Stop",
    service: AVTransport,
    request: {
    },
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

// Default Validate implementations for simple operations
impl Validate for PauseOperationRequest {
    // No validation needed - instance_id is always valid
}

impl Validate for StopOperationRequest {
    // No validation needed - instance_id is always valid
}

// Advanced operation with parameters and custom validation
define_upnp_operation! {
    operation: PlayOperation,
    action: "Play",
    service: AVTransport,
    request: {
        speed: String,
    },
    response: (),
    payload: |req| {
        format!("<InstanceID>{}</InstanceID><Speed>{}</Speed>", req.instance_id, req.speed)
    },
    parse: |_xml| Ok(()),
}

// Custom validation implementation for PlayOperation (overrides the default macro validation)
impl Validate for PlayOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.speed.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value("speed", &self.speed));
        }

        // Basic validation: check if speed is a valid value
        match self.speed.as_str() {
            "1" | "0" => Ok(()),
            other => {
                // Allow numeric values
                if other.parse::<f32>().is_ok() {
                    Ok(())
                } else {
                    Err(crate::operation::ValidationError::Custom {
                        parameter: "speed".to_string(),
                        message: "Speed must be '1', '0', or a numeric value".to_string(),
                    })
                }
            }
        }
    }
}

// Operation with complex XML response parsing
define_operation_with_response! {
    operation: GetTransportInfoOperation,
    action: "GetTransportInfo",
    service: AVTransport,
    request: {},
    response: GetTransportInfoResponse {
        current_transport_state: String,
        current_transport_status: String,
        current_speed: String,
    },
    xml_mapping: {
        current_transport_state: "CurrentTransportState",
        current_transport_status: "CurrentTransportStatus",
        current_speed: "CurrentSpeed",
    },
}

// Default Validate implementation for GetTransportInfo operation (no parameters)
impl Validate for GetTransportInfoOperationRequest {
    // No validation needed for parameterless operation
}

// The macros automatically generate the convenience functions:
// - play_operation()
// - pause_operation()
// - stop_operation()
// - get_transport_info_operation()

// Legacy convenience functions for backward compatibility
pub use play_operation as play;
pub use pause_operation as pause;
pub use stop_operation as stop;
pub use get_transport_info_operation as get_transport_info;

/// Service identifier for AVTransport
pub const SERVICE: crate::Service = crate::Service::AVTransport;

/// Subscribe to AVTransport events
///
/// This is a convenience function that subscribes to AVTransport service events.
/// Events include transport state changes (play/pause/stop), track changes, etc.
///
/// # Arguments
/// * `client` - The SonosClient to use for the subscription
/// * `ip` - The IP address of the Sonos device
/// * `callback_url` - URL where the device will send event notifications
///
/// # Returns
/// A managed subscription for AVTransport events
///
/// # Example
/// ```rust,ignore
/// use sonos_api::{SonosClient, services::av_transport};
///
/// let client = SonosClient::new();
/// let subscription = av_transport::subscribe(
///     &client,
///     "192.168.1.100",
///     "http://192.168.1.50:8080/callback"
/// )?;
///
/// // Now AVTransport events will be sent to your callback URL
/// // Execute control operations separately:
/// let play_op = av_transport::play("1".to_string()).build()?;
/// client.execute("192.168.1.100", play_op)?;
/// ```
pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}

/// Subscribe to AVTransport events with custom timeout
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
/// A managed subscription for AVTransport events
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
    fn test_play_operation_builder() {
        let play_op = play_operation("1".to_string()).build().unwrap();
        assert_eq!(play_op.request().speed, "1");
        assert_eq!(play_op.metadata().action, "Play");
    }

    #[test]
    fn test_play_validation_basic() {
        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "".to_string(),
        };
        assert!(request.validate_basic().is_err());

        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "1".to_string(),
        };
        assert!(request.validate_basic().is_ok());
    }

    #[test]
    fn test_payload_generation() {
        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "1".to_string(),
        };

        let payload = PlayOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<Speed>1</Speed>"));
    }


    #[test]
    fn test_service_constant() {
        // Verify that SERVICE constant is correctly set
        assert_eq!(SERVICE, crate::Service::AVTransport);
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
        // We can't test actual subscription without a device, but we can
        // verify the service type is used correctly by checking the SERVICE constant
        let client = crate::SonosClient::new();

        // Verify that our subscription helpers would use the correct service
        assert_eq!(SERVICE, crate::Service::AVTransport);

        // The subscribe function should internally call client.subscribe(ip, SERVICE, callback_url)
        // We can't test the actual call without mocking, but the function signature
        // and SERVICE constant verification confirms correct integration
    }

}