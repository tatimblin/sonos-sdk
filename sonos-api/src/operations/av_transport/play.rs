//! Play operation for AVTransport service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// Play operation
pub struct PlayOperation;

/// Request for play operation
#[derive(Serialize)]
pub struct PlayRequest {
    pub instance_id: u32,
    pub speed: String,
}

/// Response for play operation
#[derive(Deserialize)]
pub struct PlayResponse;

impl SonosOperation for PlayOperation {
    type Request = PlayRequest;
    type Response = PlayResponse;
    
    const SERVICE: Service = Service::AVTransport;
    const ACTION: &'static str = "Play";
    
    fn build_payload(request: &Self::Request) -> String {
        format!(
            "<InstanceID>{}</InstanceID><Speed>{}</Speed>",
            request.instance_id, request.speed
        )
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // Play operation has no meaningful response data
        Ok(PlayResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_play_payload_construction() {
        let request = PlayRequest {
            instance_id: 0,
            speed: "1".to_string(),
        };
        
        let payload = PlayOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID><Speed>1</Speed>");
    }

    #[test]
    fn test_play_response_parsing() {
        // Play operation returns empty response
        let xml_str = r#"<PlayResponse></PlayResponse>"#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = PlayOperation::parse_response(&xml);
        assert!(result.is_ok());
    }
}