//! GetTransportInfo operation for AVTransport service

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service, SonosOperation};

/// GetTransportInfo operation
pub struct GetTransportInfoOperation;

/// Request for GetTransportInfo operation
#[derive(Serialize)]
pub struct GetTransportInfoRequest {
    pub instance_id: u32,
}

/// Transport state enum
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlayState {
    Playing,
    #[serde(rename = "PAUSED_PLAYBACK")]
    Paused,
    Stopped,
    Transitioning,
}

/// Response for GetTransportInfo operation
#[derive(Debug, Clone, Deserialize)]
pub struct GetTransportInfoResponse {
    #[serde(rename = "CurrentTransportState")]
    pub current_transport_state: PlayState,
    #[serde(rename = "CurrentTransportStatus")]
    pub current_transport_status: String,
    #[serde(rename = "CurrentSpeed")]
    pub current_speed: String,
}

impl SonosOperation for GetTransportInfoOperation {
    type Request = GetTransportInfoRequest;
    type Response = GetTransportInfoResponse;
    
    const SERVICE: Service = Service::AVTransport;
    const ACTION: &'static str = "GetTransportInfo";
    
    fn build_payload(request: &Self::Request) -> String {
        // TODO: Implement payload construction
        todo!("GetTransportInfo operation implementation will be added in task 4")
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // TODO: Implement response parsing
        todo!("GetTransportInfo operation implementation will be added in task 4")
    }
}