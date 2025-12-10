//! Sonos API operations organized by service
//! 
//! This module contains all the individual API operations, organized by
//! the UPnP service they belong to.

pub mod av_transport;
pub mod rendering_control;
pub mod group_rendering_control;
pub mod zone_group_topology;
pub mod device_properties;

// Re-export commonly used operations
pub use av_transport::{PlayOperation, PauseOperation, StopOperation, GetTransportInfoOperation};
pub use rendering_control::{GetVolumeOperation, SetVolumeOperation, SetRelativeVolumeOperation};