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
        // TODO: Implement payload construction
        todo!("SetRelativeVolume operation implementation will be added in task 5")
    }
    
    fn parse_response(_xml: &Element) -> Result<Self::Response, ApiError> {
        // TODO: Implement response parsing
        todo!("SetRelativeVolume operation implementation will be added in task 5")
    }
}