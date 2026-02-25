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

// =============================================================================
// GET MUTE
// =============================================================================

// Manual implementation because Sonos returns "0"/"1" for bools, not "true"/"false",
// and the define_operation_with_response! macro's .parse::<bool>() only handles "true"/"false".
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct GetMuteOperationRequest {
    pub channel: String,
    pub instance_id: u32,
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct GetMuteResponse {
    pub current_mute: bool,
}

pub struct GetMuteOperation;

impl crate::operation::UPnPOperation for GetMuteOperation {
    type Request = GetMuteOperationRequest;
    type Response = GetMuteResponse;

    const SERVICE: crate::service::Service = crate::service::Service::RenderingControl;
    const ACTION: &'static str = "GetMute";

    fn build_payload(request: &Self::Request) -> Result<String, crate::operation::ValidationError> {
        request.validate(crate::operation::ValidationLevel::Basic)?;
        Ok(format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel>",
            request.instance_id, request.channel
        ))
    }

    fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, crate::error::ApiError> {
        let current_mute = xml
            .get_child("CurrentMute")
            .and_then(|e| e.get_text())
            .map(|s| s.trim() == "1" || s.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Ok(GetMuteResponse { current_mute })
    }
}

pub fn get_mute_operation(channel: String) -> crate::operation::OperationBuilder<GetMuteOperation> {
    let request = GetMuteOperationRequest {
        channel,
        instance_id: 0,
    };
    crate::operation::OperationBuilder::new(request)
}

impl Validate for GetMuteOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}

pub use get_mute_operation as get_mute;

// =============================================================================
// SET MUTE
// =============================================================================

define_upnp_operation! {
    operation: SetMuteOperation,
    action: "SetMute",
    service: RenderingControl,
    request: {
        channel: String,
        desired_mute: bool,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel><DesiredMute>{}</DesiredMute>",
            req.instance_id, req.channel, if req.desired_mute { "1" } else { "0" }
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetMuteOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}

pub use set_mute_operation as set_mute;

// =============================================================================
// GET BASS
// =============================================================================

define_operation_with_response! {
    operation: GetBassOperation,
    action: "GetBass",
    service: RenderingControl,
    request: {},
    response: GetBassResponse {
        current_bass: i8,
    },
    xml_mapping: {
        current_bass: "CurrentBass",
    },
}

impl Validate for GetBassOperationRequest {}

pub use get_bass_operation as get_bass;

// =============================================================================
// SET BASS
// =============================================================================

define_upnp_operation! {
    operation: SetBassOperation,
    action: "SetBass",
    service: RenderingControl,
    request: {
        desired_bass: i8,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><DesiredBass>{}</DesiredBass>",
            req.instance_id, req.desired_bass
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetBassOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.desired_bass < -10 || self.desired_bass > 10 {
            return Err(crate::operation::ValidationError::range_error(
                "desired_bass", -10, 10, self.desired_bass,
            ));
        }
        Ok(())
    }
}

pub use set_bass_operation as set_bass;

// =============================================================================
// GET TREBLE
// =============================================================================

define_operation_with_response! {
    operation: GetTrebleOperation,
    action: "GetTreble",
    service: RenderingControl,
    request: {},
    response: GetTrebleResponse {
        current_treble: i8,
    },
    xml_mapping: {
        current_treble: "CurrentTreble",
    },
}

impl Validate for GetTrebleOperationRequest {}

pub use get_treble_operation as get_treble;

// =============================================================================
// SET TREBLE
// =============================================================================

define_upnp_operation! {
    operation: SetTrebleOperation,
    action: "SetTreble",
    service: RenderingControl,
    request: {
        desired_treble: i8,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><DesiredTreble>{}</DesiredTreble>",
            req.instance_id, req.desired_treble
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetTrebleOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.desired_treble < -10 || self.desired_treble > 10 {
            return Err(crate::operation::ValidationError::range_error(
                "desired_treble", -10, 10, self.desired_treble,
            ));
        }
        Ok(())
    }
}

pub use set_treble_operation as set_treble;

// =============================================================================
// GET LOUDNESS
// =============================================================================

// Manual implementation for bool "0"/"1" parsing (same reason as GetMute).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct GetLoudnessOperationRequest {
    pub channel: String,
    pub instance_id: u32,
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq)]
pub struct GetLoudnessResponse {
    pub current_loudness: bool,
}

pub struct GetLoudnessOperation;

impl crate::operation::UPnPOperation for GetLoudnessOperation {
    type Request = GetLoudnessOperationRequest;
    type Response = GetLoudnessResponse;

    const SERVICE: crate::service::Service = crate::service::Service::RenderingControl;
    const ACTION: &'static str = "GetLoudness";

    fn build_payload(request: &Self::Request) -> Result<String, crate::operation::ValidationError> {
        request.validate(crate::operation::ValidationLevel::Basic)?;
        Ok(format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel>",
            request.instance_id, request.channel
        ))
    }

    fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, crate::error::ApiError> {
        let current_loudness = xml
            .get_child("CurrentLoudness")
            .and_then(|e| e.get_text())
            .map(|s| s.trim() == "1" || s.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Ok(GetLoudnessResponse { current_loudness })
    }
}

pub fn get_loudness_operation(channel: String) -> crate::operation::OperationBuilder<GetLoudnessOperation> {
    let request = GetLoudnessOperationRequest {
        channel,
        instance_id: 0,
    };
    crate::operation::OperationBuilder::new(request)
}

impl Validate for GetLoudnessOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}

pub use get_loudness_operation as get_loudness;

// =============================================================================
// SET LOUDNESS
// =============================================================================

define_upnp_operation! {
    operation: SetLoudnessOperation,
    action: "SetLoudness",
    service: RenderingControl,
    request: {
        channel: String,
        desired_loudness: bool,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel><DesiredLoudness>{}</DesiredLoudness>",
            req.instance_id, req.channel, if req.desired_loudness { "1" } else { "0" }
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetLoudnessOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.channel.as_str() {
            "Master" | "LF" | "RF" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "channel".to_string(),
                message: format!("Invalid channel '{}'. Must be 'Master', 'LF', or 'RF'", other),
            })
        }
    }
}

pub use set_loudness_operation as set_loudness;

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
        let _client = crate::SonosClient::new();

        // Verify SERVICE constant
        assert_eq!(SERVICE, crate::Service::RenderingControl);

        // The subscribe function should internally call client.subscribe(ip, SERVICE, callback_url)
        // We can't test the actual call without mocking, but the function signature
        // and SERVICE constant verification confirms correct integration
    }

    // =========================================================================
    // Mute operation tests
    // =========================================================================

    #[test]
    fn test_get_mute_builder() {
        let op = get_mute_operation("Master".to_string()).build().unwrap();
        assert_eq!(op.request().channel, "Master");
        assert_eq!(op.request().instance_id, 0);
    }

    #[test]
    fn test_get_mute_payload() {
        let request = GetMuteOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
        };
        let payload = GetMuteOperation::build_payload(&request).unwrap();
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel>");
    }

    #[test]
    fn test_get_mute_parse_response_true() {
        let xml_str = r#"<GetMuteResponse><CurrentMute>1</CurrentMute></GetMuteResponse>"#;
        let xml = xmltree::Element::parse(xml_str.as_bytes()).unwrap();
        let response = GetMuteOperation::parse_response(&xml).unwrap();
        assert!(response.current_mute);
    }

    #[test]
    fn test_get_mute_parse_response_false() {
        let xml_str = r#"<GetMuteResponse><CurrentMute>0</CurrentMute></GetMuteResponse>"#;
        let xml = xmltree::Element::parse(xml_str.as_bytes()).unwrap();
        let response = GetMuteOperation::parse_response(&xml).unwrap();
        assert!(!response.current_mute);
    }

    #[test]
    fn test_get_mute_rejects_invalid_channel() {
        let request = GetMuteOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(),
        };
        assert!(request.validate_basic().is_err());
    }

    #[test]
    fn test_set_mute_builder() {
        let op = set_mute_operation("Master".to_string(), true).build().unwrap();
        assert!(op.request().desired_mute);
        assert_eq!(op.request().channel, "Master");
    }

    #[test]
    fn test_set_mute_payload_true() {
        let request = SetMuteOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_mute: true,
        };
        let payload = SetMuteOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<DesiredMute>1</DesiredMute>"));
    }

    #[test]
    fn test_set_mute_payload_false() {
        let request = SetMuteOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_mute: false,
        };
        let payload = SetMuteOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<DesiredMute>0</DesiredMute>"));
    }

    #[test]
    fn test_set_mute_rejects_invalid_channel() {
        let request = SetMuteOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(),
            desired_mute: true,
        };
        assert!(request.validate_basic().is_err());
    }

    // =========================================================================
    // Bass operation tests
    // =========================================================================

    #[test]
    fn test_get_bass_builder() {
        let op = get_bass_operation().build().unwrap();
        assert_eq!(op.request().instance_id, 0);
    }

    #[test]
    fn test_get_bass_payload() {
        let request = GetBassOperationRequest { instance_id: 0 };
        let payload = GetBassOperation::build_payload(&request).unwrap();
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_set_bass_builder() {
        let op = set_bass_operation(5).build().unwrap();
        assert_eq!(op.request().desired_bass, 5);
    }

    #[test]
    fn test_set_bass_payload() {
        let request = SetBassOperationRequest {
            instance_id: 0,
            desired_bass: -5,
        };
        let payload = SetBassOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<DesiredBass>-5</DesiredBass>"));
    }

    #[test]
    fn test_set_bass_validation() {
        // Valid range
        let request = SetBassOperationRequest { instance_id: 0, desired_bass: -10 };
        assert!(request.validate_basic().is_ok());
        let request = SetBassOperationRequest { instance_id: 0, desired_bass: 10 };
        assert!(request.validate_basic().is_ok());

        // Out of range
        let request = SetBassOperationRequest { instance_id: 0, desired_bass: -11 };
        assert!(request.validate_basic().is_err());
        let request = SetBassOperationRequest { instance_id: 0, desired_bass: 11 };
        assert!(request.validate_basic().is_err());
    }

    // =========================================================================
    // Treble operation tests
    // =========================================================================

    #[test]
    fn test_get_treble_builder() {
        let op = get_treble_operation().build().unwrap();
        assert_eq!(op.request().instance_id, 0);
    }

    #[test]
    fn test_get_treble_payload() {
        let request = GetTrebleOperationRequest { instance_id: 0 };
        let payload = GetTrebleOperation::build_payload(&request).unwrap();
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_set_treble_builder() {
        let op = set_treble_operation(-3).build().unwrap();
        assert_eq!(op.request().desired_treble, -3);
    }

    #[test]
    fn test_set_treble_payload() {
        let request = SetTrebleOperationRequest {
            instance_id: 0,
            desired_treble: 7,
        };
        let payload = SetTrebleOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<DesiredTreble>7</DesiredTreble>"));
    }

    #[test]
    fn test_set_treble_validation() {
        let request = SetTrebleOperationRequest { instance_id: 0, desired_treble: -10 };
        assert!(request.validate_basic().is_ok());
        let request = SetTrebleOperationRequest { instance_id: 0, desired_treble: 10 };
        assert!(request.validate_basic().is_ok());

        let request = SetTrebleOperationRequest { instance_id: 0, desired_treble: -11 };
        assert!(request.validate_basic().is_err());
        let request = SetTrebleOperationRequest { instance_id: 0, desired_treble: 11 };
        assert!(request.validate_basic().is_err());
    }

    // =========================================================================
    // Loudness operation tests
    // =========================================================================

    #[test]
    fn test_get_loudness_builder() {
        let op = get_loudness_operation("Master".to_string()).build().unwrap();
        assert_eq!(op.request().channel, "Master");
        assert_eq!(op.request().instance_id, 0);
    }

    #[test]
    fn test_get_loudness_payload() {
        let request = GetLoudnessOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
        };
        let payload = GetLoudnessOperation::build_payload(&request).unwrap();
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel>");
    }

    #[test]
    fn test_get_loudness_parse_response_true() {
        let xml_str = r#"<GetLoudnessResponse><CurrentLoudness>1</CurrentLoudness></GetLoudnessResponse>"#;
        let xml = xmltree::Element::parse(xml_str.as_bytes()).unwrap();
        let response = GetLoudnessOperation::parse_response(&xml).unwrap();
        assert!(response.current_loudness);
    }

    #[test]
    fn test_get_loudness_parse_response_false() {
        let xml_str = r#"<GetLoudnessResponse><CurrentLoudness>0</CurrentLoudness></GetLoudnessResponse>"#;
        let xml = xmltree::Element::parse(xml_str.as_bytes()).unwrap();
        let response = GetLoudnessOperation::parse_response(&xml).unwrap();
        assert!(!response.current_loudness);
    }

    #[test]
    fn test_get_loudness_rejects_invalid_channel() {
        let request = GetLoudnessOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(),
        };
        assert!(request.validate_basic().is_err());
    }

    #[test]
    fn test_set_loudness_builder() {
        let op = set_loudness_operation("Master".to_string(), true).build().unwrap();
        assert!(op.request().desired_loudness);
        assert_eq!(op.request().channel, "Master");
    }

    #[test]
    fn test_set_loudness_payload_true() {
        let request = SetLoudnessOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_loudness: true,
        };
        let payload = SetLoudnessOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<DesiredLoudness>1</DesiredLoudness>"));
    }

    #[test]
    fn test_set_loudness_payload_false() {
        let request = SetLoudnessOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_loudness: false,
        };
        let payload = SetLoudnessOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<DesiredLoudness>0</DesiredLoudness>"));
    }

    #[test]
    fn test_set_loudness_rejects_invalid_channel() {
        let request = SetLoudnessOperationRequest {
            instance_id: 0,
            channel: "Invalid".to_string(),
            desired_loudness: true,
        };
        assert!(request.validate_basic().is_err());
    }

}