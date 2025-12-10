//! RenderingControl service operations
//! 
//! Operations for controlling volume and audio rendering on Sonos devices.

mod get_volume;
mod set_volume;
mod set_relative_volume;

pub use get_volume::GetVolumeOperation;
pub use set_volume::SetVolumeOperation;
pub use set_relative_volume::SetRelativeVolumeOperation;