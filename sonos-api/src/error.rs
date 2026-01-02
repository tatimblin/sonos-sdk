use soap_client::SoapError;
use thiserror::Error;

/// High-level API errors for Sonos operations
///
/// This enum provides domain-specific error types that abstract away the underlying
/// SOAP communication details and provide meaningful error information for common
/// failure scenarios when controlling Sonos devices.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Network communication error
    ///
    /// This error occurs when there are network-level issues communicating
    /// with the device, such as connection timeouts, DNS resolution failures,
    /// or the device being unreachable.
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Response parsing error
    ///
    /// This error occurs when the device returns a valid response but
    /// the response content cannot be parsed into the expected format.
    /// This covers XML parsing errors, unexpected response formats, and event parsing issues.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// SOAP fault returned by device
    ///
    /// This error occurs when the device returns a SOAP fault response,
    /// indicating that the request was malformed or the operation failed.
    #[error("SOAP fault: error code {0}")]
    SoapFault(u16),

    /// Invalid parameter value
    ///
    /// This error is returned when an operation parameter has an invalid value.
    /// This covers volume out of range, invalid device states, malformed URLs, etc.
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Subscription operation failed
    ///
    /// This error occurs when UPnP subscription operations (create, renew, unsubscribe) fail.
    /// This covers subscription failures, renewal failures, expired subscriptions, etc.
    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    /// Device operation error
    ///
    /// This error covers device-specific issues like not being a group coordinator,
    /// unsupported operations, or invalid device states.
    #[error("Device error: {0}")]
    DeviceError(String),
}

impl ApiError {
    /// Create a new NetworkError
    pub fn network_error<S: Into<String>>(message: S) -> Self {
        Self::NetworkError(message.into())
    }

    /// Create a new ParseError
    pub fn parse_error<S: Into<String>>(message: S) -> Self {
        Self::ParseError(message.into())
    }

    /// Create a new InvalidParameter error
    pub fn invalid_parameter<S: Into<String>>(message: S) -> Self {
        Self::InvalidParameter(message.into())
    }

    /// Create a new SubscriptionError
    pub fn subscription_error<S: Into<String>>(message: S) -> Self {
        Self::SubscriptionError(message.into())
    }

    /// Create a new DeviceError
    pub fn device_error<S: Into<String>>(message: S) -> Self {
        Self::DeviceError(message.into())
    }

    /// Create an invalid volume error (convenience method)
    pub fn invalid_volume(volume: u8) -> Self {
        Self::InvalidParameter(format!("Invalid volume: {} (must be 0-100)", volume))
    }

    /// Create a device unreachable error (convenience method)
    pub fn device_unreachable<S: Into<String>>(device: S) -> Self {
        Self::NetworkError(format!("Device unreachable: {}", device.into()))
    }

    /// Create a not coordinator error (convenience method)
    pub fn not_coordinator<S: Into<String>>(device: S) -> Self {
        Self::DeviceError(format!("Device is not a group coordinator: {}", device.into()))
    }

    /// Create a subscription failed error (convenience method)
    pub fn subscription_failed<S: Into<String>>(message: S) -> Self {
        Self::SubscriptionError(format!("Subscription failed: {}", message.into()))
    }

    /// Create a renewal failed error (convenience method)
    pub fn renewal_failed<S: Into<String>>(message: S) -> Self {
        Self::SubscriptionError(format!("Subscription renewal failed: {}", message.into()))
    }

    /// Create a subscription expired error (convenience method)
    pub fn subscription_expired() -> Self {
        Self::SubscriptionError("Subscription expired".to_string())
    }

    /// Create an invalid callback URL error (convenience method)
    pub fn invalid_callback_url<S: Into<String>>(url: S) -> Self {
        Self::InvalidParameter(format!("Invalid callback URL: {}", url.into()))
    }

    /// Create an invalid state error (convenience method)
    pub fn invalid_state<S: Into<String>>(message: S) -> Self {
        Self::InvalidParameter(format!("Invalid device state: {}", message.into()))
    }

    /// Create an event parsing failed error (convenience method)
    pub fn event_parsing_failed<S: Into<String>>(message: S) -> Self {
        Self::ParseError(format!("Event parsing failed: {}", message.into()))
    }

    /// Create an unsupported operation error (convenience method)
    pub fn unsupported_operation() -> Self {
        Self::DeviceError("Operation not supported by device".to_string())
    }
}

/// Type alias for results that can return an ApiError
pub type Result<T> = std::result::Result<T, ApiError>;

/// Convert from SoapError to ApiError
impl From<SoapError> for ApiError {
    fn from(error: SoapError) -> Self {
        match error {
            SoapError::Network(msg) => ApiError::NetworkError(msg),
            SoapError::Parse(msg) => ApiError::ParseError(msg),
            SoapError::Fault(code) => ApiError::SoapFault(code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = ApiError::invalid_volume(150);
        assert!(matches!(error, ApiError::InvalidParameter(_)));

        let error = ApiError::device_unreachable("192.168.1.100");
        assert!(matches!(error, ApiError::NetworkError(_)));

        let error = ApiError::subscription_failed("timeout");
        assert!(matches!(error, ApiError::SubscriptionError(_)));
    }

    #[test]
    fn test_soap_error_conversion() {
        let soap_error = SoapError::Network("connection timeout".to_string());
        let api_error: ApiError = soap_error.into();
        assert!(matches!(api_error, ApiError::NetworkError(_)));

        let soap_error = SoapError::Parse("invalid XML".to_string());
        let api_error: ApiError = soap_error.into();
        assert!(matches!(api_error, ApiError::ParseError(_)));

        let soap_error = SoapError::Fault(500);
        let api_error: ApiError = soap_error.into();
        assert!(matches!(api_error, ApiError::SoapFault(500)));
    }

    #[test]
    fn test_convenience_methods() {
        let error = ApiError::not_coordinator("192.168.1.100");
        let error_str = format!("{}", error);
        assert!(error_str.contains("not a group coordinator"));

        let error = ApiError::subscription_expired();
        let error_str = format!("{}", error);
        assert!(error_str.contains("expired"));
    }
}