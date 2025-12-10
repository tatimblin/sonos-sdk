//! High-level Sonos API for device control
//! 
//! This crate provides a type-safe, trait-based API for controlling Sonos devices.
//! It uses the private `soap-client` crate for low-level SOAP communication.

mod error;
mod operation;
mod service;
mod controller;
pub mod operations;

pub use error::ApiError;
pub use operation::SonosOperation;
pub use service::Service;
pub use controller::DeviceController;

/// Result type alias for API operations
pub type Result<T> = std::result::Result<T, ApiError>;