//! GroupRenderingControl service for group-wide audio rendering operations
//!
//! This service handles group-wide audio rendering operations (group volume, group mute)
//! for Sonos speaker groups. Operations should only be sent to the group coordinator.
//!
//! # Control Operations
//! ```rust,ignore
//! use sonos_api::services::group_rendering_control;
//!
//! let vol_op = group_rendering_control::set_group_volume(75).build()?;
//! client.execute("192.168.1.100", vol_op)?;
//! ```
//!
//! # Event Subscriptions
//! ```rust,ignore
//! let subscription = group_rendering_control::subscribe(&client, "192.168.1.100", "http://callback")?;
//! ```
//!
//! # Important Notes
//! - Operations should only be sent to the group coordinator
//! - Sending to non-coordinator speakers will result in error code 701

pub mod operations;

// Re-export operations for convenience
pub use operations::*;
