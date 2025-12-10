//! GetVolume operation for RenderingControl service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// GetVolume operation
pub struct GetVolumeOperation;

/// Request for GetVolume operation
#[derive(Serialize)]
pub struct GetVolumeRequest {
    pub instance_id: u32,
    pub channel: String,
}

/// Response for GetVolume operation
#[derive(Deserialize)]
pub struct GetVolumeResponse {
    #[serde(rename = "CurrentVolume")]
    pub current_volume: u8,
}

impl SonosOperation for GetVolumeOperation {
    type Request = GetVolumeRequest;
    type Response = GetVolumeResponse;
    
    const SERVICE: Service = Service::RenderingControl;
    const ACTION: &'static str = "GetVolume";
    
    fn build_payload(request: &Self::Request) -> String {
        format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel>",
            request.instance_id, request.channel
        )
    }
    
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError> {
        let volume_str = xml
            .get_child("CurrentVolume")
            .and_then(|e| e.get_text())
            .ok_or_else(|| ApiError::ParseError("Missing CurrentVolume element".to_string()))?;
            
        let current_volume = volume_str.parse::<u8>()
            .map_err(|e| ApiError::ParseError(format!("Invalid volume value: {}", e)))?;
            
        Ok(GetVolumeResponse { current_volume })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_volume_payload_construction() {
        let request = GetVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
        };
        
        let payload = GetVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel>");
    }

    #[test]
    fn test_get_volume_response_parsing() {
        let xml_str = r#"
            <GetVolumeResponse>
                <CurrentVolume>75</CurrentVolume>
            </GetVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetVolumeOperation::parse_response(&xml).unwrap();
        assert_eq!(result.current_volume, 75);
    }

    #[test]
    fn test_get_volume_response_parsing_zero_volume() {
        let xml_str = r#"
            <GetVolumeResponse>
                <CurrentVolume>0</CurrentVolume>
            </GetVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetVolumeOperation::parse_response(&xml).unwrap();
        assert_eq!(result.current_volume, 0);
    }

    #[test]
    fn test_get_volume_response_parsing_max_volume() {
        let xml_str = r#"
            <GetVolumeResponse>
                <CurrentVolume>100</CurrentVolume>
            </GetVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetVolumeOperation::parse_response(&xml).unwrap();
        assert_eq!(result.current_volume, 100);
    }

    #[test]
    fn test_get_volume_response_parsing_missing_volume() {
        let xml_str = r#"
            <GetVolumeResponse>
            </GetVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetVolumeOperation::parse_response(&xml);
        assert!(result.is_err());
        if let Err(ApiError::ParseError(msg)) = result {
            assert!(msg.contains("Missing CurrentVolume element"));
        } else {
            panic!("Expected ParseError");
        }
    }

    #[test]
    fn test_get_volume_response_parsing_invalid_volume() {
        let xml_str = r#"
            <GetVolumeResponse>
                <CurrentVolume>invalid</CurrentVolume>
            </GetVolumeResponse>
        "#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = GetVolumeOperation::parse_response(&xml);
        assert!(result.is_err());
        if let Err(ApiError::ParseError(msg)) = result {
            assert!(msg.contains("Invalid volume value"));
        } else {
            panic!("Expected ParseError");
        }
    }
}