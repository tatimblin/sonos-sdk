//! Error types for the Sonos API

use thiserror::Error;
use soap_client::SoapError;

/// High-level API errors
#[derive(Debug, Error)]
pub enum ApiError {
    /// SOAP communication error
    #[error("SOAP communication error")]
    Soap(#[from] SoapError),
    
    /// Device is unreachable
    #[error("Device unreachable: {0}")]
    DeviceUnreachable(String),
    
    /// Invalid volume value
    #[error("Invalid volume: {0} (must be 0-100)")]
    InvalidVolume(u8),
    
    /// Device is not a group coordinator
    #[error("Device is not a group coordinator: {0}")]
    NotCoordinator(String),
    
    /// Operation not supported by device
    #[error("Operation not supported by device")]
    UnsupportedOperation,
    
    /// Response parsing error
    #[error("Response parsing error: {0}")]
    ParseError(String),
}