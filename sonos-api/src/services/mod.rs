//! Service modules with enhanced UPnP operations
//!
//! This module contains service definitions using the new enhanced operation framework.
//! Each service provides operations with composability, validation, and builder patterns.

pub mod av_transport;
pub mod rendering_control;

pub use av_transport::*;
pub use rendering_control::*;