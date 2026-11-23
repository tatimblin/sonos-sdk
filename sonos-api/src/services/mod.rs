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
pub mod events;
pub mod group_management;
pub mod group_rendering_control;
pub mod rendering_control;
pub mod zone_group_topology;

use crate::operation::{ComposableOperation, UPnPOperation, ValidationError};
use crate::SonosClient;

/// Run one of a `poll()`'s optional reads, treating failure as "not available".
///
/// The failure is swallowed by design (the field polls as `None`), so it is
/// logged here: otherwise a speaker that rejects, say, `GetBass` is invisible.
pub(crate) fn poll_optional<Op: UPnPOperation>(
    client: &SonosClient,
    ip: &str,
    operation: std::result::Result<ComposableOperation<Op>, ValidationError>,
) -> Option<Op::Response> {
    let result = operation
        .map_err(|e| crate::ApiError::ParseError(e.to_string()))
        .and_then(|op| client.execute_enhanced(ip, op));
    match result {
        Ok(response) => Some(response),
        Err(e) => {
            tracing::debug!(
                %ip,
                service = ?Op::SERVICE,
                action = Op::ACTION,
                error = %e,
                "optional poll read failed; leaving it unset"
            );
            None
        }
    }
}
