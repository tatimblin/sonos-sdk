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
        // TODO: Implement payload construction
        todo!("Stop operation implementation will be added in task 4")
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // TODO: Implement response parsing
        todo!("Stop operation implementation will be added in task 4")
    }
}