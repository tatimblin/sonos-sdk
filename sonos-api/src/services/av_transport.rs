use crate::{simple_operation, define_upnp_operation, define_operation_with_response, Validate};
use paste::paste;

simple_operation!(PauseOperation, "Pause", AVTransport, {}, ());
simple_operation!(StopOperation, "Stop", AVTransport, {}, ());

// Default Validate implementations for simple operations with no parameters
impl Validate for PauseOperationRequest {
    // No validation needed for parameterless operation
}

impl Validate for StopOperationRequest {
    // No validation needed for parameterless operation
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
    fn validate_boundary(&self) -> Result<(), crate::operation::ValidationError> {
        if self.speed.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value("speed", &self.speed));
        }
        Ok(())
    }

    fn validate_comprehensive(&self) -> Result<(), crate::operation::ValidationError> {
        self.validate_boundary()?;

        // Comprehensive validation: check if speed is a valid value
        match self.speed.as_str() {
            "1" | "0" => Ok(()),
            other => {
                // Allow numeric values for comprehensive validation
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{ValidationLevel, UPnPOperation};

    #[test]
    fn test_play_operation_builder() {
        let play_op = play_operation("1".to_string()).build().unwrap();
        assert_eq!(play_op.request().speed, "1");
        assert_eq!(play_op.metadata().action, "Play");
    }

    #[test]
    fn test_play_validation_boundary() {
        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "".to_string(),
        };
        assert!(request.validate_boundary().is_err());

        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "1".to_string(),
        };
        assert!(request.validate_boundary().is_ok());
    }

    #[test]
    fn test_play_validation_comprehensive() {
        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "invalid".to_string(),
        };
        assert!(request.validate_comprehensive().is_err());

        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "0.5".to_string(),
        };
        assert!(request.validate_comprehensive().is_ok());
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
    fn test_operation_composition() {
        let play_op = play_operation("1".to_string()).build().unwrap();
        let pause_op = pause_operation().build().unwrap();

        // Test chaining operations
        let sequence = play_op.and_then(pause_op);
        let (first, second) = sequence.operations();

        assert_eq!(first.request().speed, "1");
        assert_eq!(first.metadata().action, "Play");
        assert_eq!(second.metadata().action, "Pause");
    }

    #[test]
    fn test_operation_batching() {
        let info_op = get_transport_info_operation().build().unwrap();
        let stop_op = stop_operation().build().unwrap();

        // Test batching operations
        let batch = info_op.concurrent_with(stop_op);
        let (first, second) = batch.operations();

        assert_eq!(first.metadata().action, "GetTransportInfo");
        assert_eq!(second.metadata().action, "Stop");
    }

    #[test]
    fn test_conditional_operations() {
        let play_op = play_operation("1".to_string()).build().unwrap();

        // Test conditional execution
        let conditional = play_op.condition(|| true);
        assert!(conditional.should_execute());

        let play_op2 = play("1".to_string()).build().unwrap();
        let conditional2 = play_op2.condition(|| false);
        assert!(!conditional2.should_execute());
    }
}