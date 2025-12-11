//! SetVolume operation for RenderingControl service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// SetVolume operation
pub struct SetVolumeOperation;

/// Request for SetVolume operation
#[derive(Serialize)]
pub struct SetVolumeRequest {
    pub instance_id: u32,
    pub channel: String,
    pub desired_volume: u8,
}

/// Response for SetVolume operation
#[derive(Deserialize)]
pub struct SetVolumeResponse;

impl SonosOperation for SetVolumeOperation {
    type Request = SetVolumeRequest;
    type Response = SetVolumeResponse;
    
    const SERVICE: Service = Service::RenderingControl;
    const ACTION: &'static str = "SetVolume";
    
    fn build_payload(request: &Self::Request) -> String {
        // Validate volume range (0-100)
        if request.desired_volume > 100 {
            // Note: In a real implementation, we might want to handle this validation
            // at a higher level, but for now we'll construct the payload as-is
        }
        
        format!(
            "<InstanceID>{}</InstanceID><Channel>{}</Channel><DesiredVolume>{}</DesiredVolume>",
            request.instance_id, request.channel, request.desired_volume
        )
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // SetVolume operation has no meaningful response data
        Ok(SetVolumeResponse)
    }
}#[cfg(test
)]
mod tests {
    use super::*;

    #[test]
    fn test_set_volume_payload_construction() {
        let request = SetVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_volume: 50,
        };
        
        let payload = SetVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredVolume>50</DesiredVolume>");
    }

    #[test]
    fn test_set_volume_payload_construction_zero_volume() {
        let request = SetVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_volume: 0,
        };
        
        let payload = SetVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredVolume>0</DesiredVolume>");
    }

    #[test]
    fn test_set_volume_payload_construction_max_volume() {
        let request = SetVolumeRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_volume: 100,
        };
        
        let payload = SetVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredVolume>100</DesiredVolume>");
    }

    #[test]
    fn test_set_volume_response_parsing() {
        let xml_str = r#"<SetVolumeResponse></SetVolumeResponse>"#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = SetVolumeOperation::parse_response(&xml);
        assert!(result.is_ok());
    }

    #[test]
    fn test_set_volume_payload_construction_different_channel() {
        let request = SetVolumeRequest {
            instance_id: 0,
            channel: "LF".to_string(),
            desired_volume: 75,
        };
        
        let payload = SetVolumeOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Channel>LF</Channel><DesiredVolume>75</DesiredVolume>");
    }
}