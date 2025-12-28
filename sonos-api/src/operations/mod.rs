//! Sonos API operations organized by service
//! 
//! This module contains all the individual API operations, organized by
//! the UPnP service they belong to.

pub mod av_transport;
pub mod rendering_control;
pub mod group_rendering_control;
pub mod zone_group_topology;
pub mod device_properties;
pub mod events;

// Re-export commonly used operations
pub use av_transport::{
    PlayOperation, PlayRequest, PlayResponse,
    PauseOperation, PauseRequest, PauseResponse,
    StopOperation, StopRequest, StopResponse,
    GetTransportInfoOperation, GetTransportInfoRequest, GetTransportInfoResponse, PlayState
};
pub use rendering_control::{GetVolumeOperation, SetVolumeOperation, SetRelativeVolumeOperation};
pub use events::{
    SubscribeOperation, SubscribeRequest, SubscribeResponse,
    UnsubscribeOperation, UnsubscribeRequest, UnsubscribeResponse,
    RenewOperation, RenewRequest, RenewResponse,
};