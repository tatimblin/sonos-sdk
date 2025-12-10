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
        // TODO: Implement payload construction
        todo!("Pause operation implementation will be added in task 4")
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // TODO: Implement response parsing
        todo!("Pause operation implementation will be added in task 4")
    }
}