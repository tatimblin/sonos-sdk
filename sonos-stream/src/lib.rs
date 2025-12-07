//! # sonos-stream
//!
//! A micro-crate for managing UPnP event subscriptions for Sonos speakers using a broker pattern.
//!
//! This crate provides a clean abstraction for subscribing to speaker events without needing to
//! understand the underlying UPnP protocol details. It uses the strategy pattern to delegate
//! service-specific event parsing to separate implementations, keeping the core broker logic
//! service-agnostic.

mod error;
mod subscription;
mod types;

pub use error::*;
pub use subscription::*;
pub use types::*;
