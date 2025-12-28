//! High-level Sonos API for device control
//! 
//! This crate provides a type-safe, trait-based API for controlling Sonos devices.
//! It uses the private `soap-client` crate for low-level SOAP communication.

pub mod client;
pub mod error;
pub mod operation;
pub mod service;
pub mod operations;

pub use client::SonosClient;
pub use error::{ApiError, Result};
pub use operation::SonosOperation;
pub use service::{Service, ServiceInfo};