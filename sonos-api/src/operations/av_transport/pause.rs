//! Pause operation for AVTransport service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// Pause operation
pub struct PauseOperation;

/// Request for pause operation
#[derive(Serialize)]
pub struct PauseRequest {
    pub instance_id: u32,
}

/// Response for pause operation
#[derive(Deserialize)]
pub struct PauseResponse;

impl SonosOperation for PauseOperation {
    type Request = PauseRequest;
    type Response = PauseResponse;
    
    const SERVICE: Service = Service::AVTransport;
    const ACTION: &'static str = "Pause";
    
    fn build_payload(request: &Self::Request) -> String {
        format!("<InstanceID>{}</InstanceID>", request.instance_id)
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // Pause operation has no meaningful response data
        Ok(PauseResponse)
    }
}#[cfg
(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pause_payload_construction() {
        let request = PauseRequest {
            instance_id: 0,
        };
        
        let payload = PauseOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_pause_response_parsing() {
        // Pause operation returns empty response
        let xml_str = r#"<PauseResponse></PauseResponse>"#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = PauseOperation::parse_response(&xml);
        assert!(result.is_ok());
    }
}