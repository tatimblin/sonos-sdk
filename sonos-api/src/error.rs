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
    /// Create a subscription expired error (used by subscription management)
    pub fn subscription_expired() -> Self {
        Self::SubscriptionError("Subscription expired".to_string())
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

/// Convert from ValidationError to ApiError
impl From<crate::operation::ValidationError> for ApiError {
    fn from(validation_error: crate::operation::ValidationError) -> Self {
        match validation_error {
            crate::operation::ValidationError::InvalidValue { parameter, value, reason } => {
                ApiError::InvalidParameter(format!("Invalid value '{}' for parameter '{}': {}", value, parameter, reason))
            }
            crate::operation::ValidationError::RangeError { parameter, value, min, max } => {
                ApiError::InvalidParameter(format!(
                    "Parameter '{}' value {} is out of range [{}, {}]",
                    parameter, value, min, max
                ))
            }
            crate::operation::ValidationError::Custom { parameter, message } => {
                ApiError::InvalidParameter(format!("Parameter '{}': {}", parameter, message))
            }
            crate::operation::ValidationError::MissingParameter { parameter } => {
                ApiError::InvalidParameter(format!("Required parameter '{}' is missing", parameter))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_expired() {
        let error = ApiError::subscription_expired();
        assert!(matches!(error, ApiError::SubscriptionError(_)));
        let error_str = format!("{}", error);
        assert!(error_str.contains("expired"));
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
    fn test_error_display() {
        let network_err = ApiError::NetworkError("connection failed".to_string());
        assert_eq!(format!("{}", network_err), "Network error: connection failed");

        let parse_err = ApiError::ParseError("invalid XML".to_string());
        assert_eq!(format!("{}", parse_err), "Parse error: invalid XML");

        let soap_fault = ApiError::SoapFault(500);
        assert_eq!(format!("{}", soap_fault), "SOAP fault: error code 500");
    }
}