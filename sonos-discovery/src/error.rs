//! Error types for the discovery system.

/// Error type for discovery operations.
///
/// Represents various failure modes that can occur during device discovery,
/// including network issues, parsing failures, and timeouts.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// Network-related errors (socket creation, HTTP requests, etc.)
    #[error("Network error: {0}")]
    NetworkError(String),
    /// Parsing errors (XML, SSDP response, etc.)
    #[error("Parse error: {0}")]
    ParseError(String),
    /// Operation timed out waiting for responses
    #[error("Operation timed out")]
    Timeout,
    /// Invalid device data or non-Sonos device detected
    #[error("Invalid device: {0}")]
    InvalidDevice(String),
}

/// Convenience Result type alias for discovery operations.
///
/// Equivalent to `std::result::Result<T, DiscoveryError>`.
pub type Result<T> = std::result::Result<T, DiscoveryError>;
