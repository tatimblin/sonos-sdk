//! Service modules with enhanced UPnP operations
//!
//! This module contains service definitions using the new enhanced operation framework.
//! Each service provides operations with composability, validation, and builder patterns.
//!
//! # Usage
//!
//! Import services individually to avoid naming conflicts:
//!
//! ```rust,ignore
//! use sonos_api::services::av_transport;
//! use sonos_api::services::rendering_control;
//!
//! // Service-specific operations
//! let play_op = av_transport::play("1".to_string()).build()?;
//! let volume_op = rendering_control::set_volume("Master".to_string(), 50).build()?;
//!
//! // Service-specific subscriptions
//! let av_subscription = av_transport::subscribe(&client, "192.168.1.100", "http://callback")?;
//! let rc_subscription = rendering_control::subscribe(&client, "192.168.1.100", "http://callback")?;
//! ```

pub mod av_transport;
pub mod group_management;
pub mod group_rendering_control;
pub mod rendering_control;
pub mod zone_group_topology;
pub mod events;