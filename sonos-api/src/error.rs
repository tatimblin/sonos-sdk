use soap_client::SoapError;
use thiserror::Error;

/// High-level API errors for Sonos operations
/// 
/// This enum provides domain-specific error types that abstract away the underlying
/// SOAP communication details and provide meaningful error information for common
/// failure scenarios when controlling Sonos devices.
#[derive(Debug, Error)]
pub enum ApiError {
    /// SOAP communication error
    /// 
    /// Wraps errors from the underlying SOAP client, including network failures,
    /// XML parsing errors, and SOAP faults returned by the device.
    #[error("SOAP communication error")]
    Soap(SoapError),
    
    /// Device is unreachable or not responding
    /// 
    /// This error occurs when the device cannot be contacted over the network,
    /// typically due to network connectivity issues or the device being offline.
    #[error("Device unreachable: {0}")]
    DeviceUnreachable(String),
    
    /// Invalid volume value provided
    /// 
    /// Volume values must be between 0 and 100 (inclusive). This error is returned
    /// when an operation receives a volume value outside this valid range.
    #[error("Invalid volume: {0} (must be 0-100)")]
    InvalidVolume(u8),
    
    /// Device is not a group coordinator
    /// 
    /// Some operations (like playback control) can only be performed on the group
    /// coordinator device. This error is returned when attempting such operations
    /// on a non-coordinator device.
    #[error("Device is not a group coordinator: {0}")]
    NotCoordinator(String),
    
    /// Operation not supported by this device
    /// 
    /// Some operations may not be available on all Sonos device models or
    /// firmware versions. This error indicates the requested operation is
    /// not supported by the target device.
    #[error("Operation not supported by device")]
    UnsupportedOperation,
    
    /// Response parsing error
    /// 
    /// This error occurs when the device returns a valid SOAP response but
    /// the response content cannot be parsed into the expected format.
    /// This may indicate API changes or unexpected response formats.
    #[error("Response parsing error: {0}")]
    ParseError(String),
    
    /// Invalid device state for operation
    /// 
    /// Some operations require the device to be in a specific state. For example,
    /// seeking may only work when media is playing. This error indicates the
    /// device is not in the required state for the requested operation.
    #[error("Invalid device state for operation: {0}")]
    InvalidState(String),
    
    /// Invalid parameter value
    /// 
    /// This error is returned when an operation parameter has an invalid value
    /// that doesn't fit into more specific error categories.
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

impl ApiError {
    /// Create a new DeviceUnreachable error
    pub fn device_unreachable<S: Into<String>>(device: S) -> Self {
        Self::DeviceUnreachable(device.into())
    }
    
    /// Create a new NotCoordinator error
    pub fn not_coordinator<S: Into<String>>(device: S) -> Self {
        Self::NotCoordinator(device.into())
    }
    
    /// Create a new ParseError
    pub fn parse_error<S: Into<String>>(message: S) -> Self {
        Self::ParseError(message.into())
    }
    
    /// Create a new InvalidState error
    pub fn invalid_state<S: Into<String>>(message: S) -> Self {
        Self::InvalidState(message.into())
    }
    
    /// Create a new InvalidParameter error
    pub fn invalid_parameter<S: Into<String>>(message: S) -> Self {
        Self::InvalidParameter(message.into())
    }
}

/// Convenience type alias for Results with ApiError
pub type Result<T> = std::result::Result<T, ApiError>;

// Implement conversion from common SOAP error codes to domain-specific errors
impl From<SoapError> for ApiError {
    fn from(soap_error: SoapError) -> Self {
        match &soap_error {
            SoapError::Network(_) => {
                // Network errors typically indicate device unreachability
                ApiError::DeviceUnreachable("Network communication failed".to_string())
            }
            SoapError::Fault(error_code) => {
                // Map common UPnP error codes to domain-specific errors
                match *error_code {
                    701 => ApiError::InvalidParameter("Invalid instance ID".to_string()),
                    702 => ApiError::InvalidParameter("Invalid current tag value".to_string()),
                    703 => ApiError::InvalidParameter("Invalid new tag value".to_string()),
                    704 => ApiError::InvalidParameter("Required tag missing".to_string()),
                    705 => ApiError::InvalidParameter("Required tag value missing".to_string()),
                    706 => ApiError::InvalidParameter("Parameter mismatch".to_string()),
                    708 => ApiError::UnsupportedOperation,
                    709 => ApiError::InvalidParameter("Invalid search criteria".to_string()),
                    710 => ApiError::InvalidParameter("Invalid sort criteria".to_string()),
                    711 => ApiError::InvalidParameter("Invalid container ID".to_string()),
                    712 => ApiError::InvalidParameter("Invalid object ID".to_string()),
                    713 => ApiError::UnsupportedOperation,
                    714 => ApiError::InvalidState("Cannot process request".to_string()),
                    _ => ApiError::Soap(soap_error),
                }
            }
            _ => ApiError::Soap(soap_error),
        }
    }
}