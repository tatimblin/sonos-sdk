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
        // TODO: Implement payload construction
        todo!("Play operation implementation will be added in task 4")
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // TODO: Implement response parsing
        todo!("Play operation implementation will be added in task 4")
    }
}