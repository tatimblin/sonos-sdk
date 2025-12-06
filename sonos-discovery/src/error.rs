//! Error types for the discovery system.

use std::fmt;

/// Error type for discovery operations.
///
/// Represents various failure modes that can occur during device discovery,
/// including network issues, parsing failures, and timeouts.
#[derive(Debug)]
pub enum DiscoveryError {
    /// Network-related errors (socket creation, HTTP requests, etc.)
    NetworkError(String),
    /// Parsing errors (XML, SSDP response, etc.)
    ParseError(String),
    /// Operation timed out waiting for responses
    Timeout,
    /// Invalid device data or non-Sonos device detected
    InvalidDevice(String),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscoveryError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            DiscoveryError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            DiscoveryError::Timeout => write!(f, "Operation timed out"),
            DiscoveryError::InvalidDevice(msg) => write!(f, "Invalid device: {}", msg),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Convenience Result type alias for discovery operations.
///
/// Equivalent to `std::result::Result<T, DiscoveryError>`.
pub type Result<T> = std::result::Result<T, DiscoveryError>;
