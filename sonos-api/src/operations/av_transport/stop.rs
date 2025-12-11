//! Stop operation for AVTransport service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// Stop operation
pub struct StopOperation;

/// Request for stop operation
#[derive(Serialize)]
pub struct StopRequest {
    pub instance_id: u32,
}

/// Response for stop operation
#[derive(Deserialize)]
pub struct StopResponse;

impl SonosOperation for StopOperation {
    type Request = StopRequest;
    type Response = StopResponse;
    
    const SERVICE: Service = Service::AVTransport;
    const ACTION: &'static str = "Stop";
    
    fn build_payload(request: &Self::Request) -> String {
        format!("<InstanceID>{}</InstanceID>", request.instance_id)
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // Stop operation has no meaningful response data
        Ok(StopResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stop_payload_construction() {
        let request = StopRequest {
            instance_id: 0,
        };
        
        let payload = StopOperation::build_payload(&request);
        assert_eq!(payload, "<InstanceID>0</InstanceID>");
    }

    #[test]
    fn test_stop_response_parsing() {
        // Stop operation returns empty response
        let xml_str = r#"<StopResponse></StopResponse>"#;
        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        
        let result = StopOperation::parse_response(&xml);
        assert!(result.is_ok());
    }
}