//! SetRelativeVolume operation for RenderingControl service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// SetRelativeVolume operation
pub struct SetRelativeVolumeOperation;

/// Request for SetRelativeVolume operation
#[derive(Serialize)]
pub struct SetRelativeVolumeRequest {
    pub instance_id: u32,
    pub channel: String,
    pub adjustment: i8,
}

/// Response for SetRelativeVolume operation
#[derive(Deserialize)]
pub struct SetRelativeVolumeResponse {
    #[serde(rename = "NewVolume")]
    pub new_volume: u8,
}

impl SonosOperation for SetRelativeVolumeOperation {
    type Request = SetRelativeVolumeRequest;
    type Response = SetRelativeVolumeResponse;
    
    const SERVICE: Service = Service::RenderingControl;
    const ACTION: &'static str = "SetRelativeVolume";
    
    fn build_payload(request: &Self::Request) -> String {
        format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel><Adjustment>{}</Adjustment>",
            request.instance_id, request.channel, request.adjustment
        )
    }
    
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError> {
        let volume_str = xml
            .get_child("NewVolume")
            .and_then(|e| e.get_text())
            .ok_or_else(|| ApiError::ParseError("Missing NewVolume element".to_string()))?;
            
        let new_volume = volume_str.parse::<u8>()
            .map_err(|e| ApiError::ParseError(format!("Invalid volume value: {}", e)))?;
            
        Ok(SetRelativeVolumeResponse { new_volume })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_relative_volume_payload_construction_positive() {
        let request = SetRelativeVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            adjustment: 10,
        };
        
        let payload = SetRelativeVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel><Adjustment>10</Adjustment>");
    }

    #[test]
    fn test_set_relative_volume_payload_construction_negative() {
        let request = SetRelativeVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            adjustment: -15,
        };
        
        let payload = SetRelativeVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel><Adjustment>-15</Adjustment>");
    }

    #[test]
    fn test_set_relative_volume_payload_construction_zero() {
        let request = SetRelativeVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            adjustment: 0,
        };
        
        let payload = SetRelativeVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel><Adjustment>0</Adjustment>");
    }

    #[test]
    fn test_set_relative_volume_response_parsing() {
        let xml_str = r#"
            <SetRelativeVolumeResponse>
                <NewVolume>65</NewVolume>
            </SetRelativeVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = SetRelativeVolumeOperation::parse_response(&xml).unwrap();
        assert_eq!(result.new_volume, 65);
    }

    #[test]
    fn test_set_relative_volume_response_parsing_zero_result() {
        let xml_str = r#"
            <SetRelativeVolumeResponse>
                <NewVolume>0</NewVolume>
            </SetRelativeVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = SetRelativeVolumeOperation::parse_response(&xml).unwrap();
        assert_eq!(result.new_volume, 0);
    }

    #[test]
    fn test_set_relative_volume_response_parsing_max_result() {
        let xml_str = r#"
            <SetRelativeVolumeResponse>
                <NewVolume>100</NewVolume>
            </SetRelativeVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = SetRelativeVolumeOperation::parse_response(&xml).unwrap();
        assert_eq!(result.new_volume, 100);
    }

    #[test]
    fn test_set_relative_volume_response_parsing_missing_volume() {
        let xml_str = r#"
            <SetRelativeVolumeResponse>
            </SetRelativeVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = SetRelativeVolumeOperation::parse_response(&xml);
        assert!(result.is_err());
        if let Err(ApiError::ParseError(msg)) = result {
            assert!(msg.contains("Missing NewVolume element"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_set_relative_volume_response_parsing_invalid_volume() {
        let xml_str = r#"
            <SetRelativeVolumeResponse>
                <NewVolume>invalid</NewVolume>
            </SetRelativeVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = SetRelativeVolumeOperation::parse_response(&xml);
        assert!(result.is_err());
        if let Err(ApiError::ParseError(msg)) = result {
            assert!(msg.contains("Invalid volume value"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_set_relative_volume_payload_construction_different_channel() {
        let request = SetRelativeVolumeRequest {
            instance_id: 0,
            channel: "LF".to_string(),
            adjustment: 5,
        };
        
        let payload = SetRelativeVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>LF</Channel><Adjustment>5</Adjustment>");
    }
}