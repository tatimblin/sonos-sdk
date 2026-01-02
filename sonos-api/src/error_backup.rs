use soap_client::SoapError;
use thiserror::Error;
use std::collections::HashMap;

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
    
    /// Subscription creation failed
    /// 
    /// This error occurs when a UPnP subscription request fails, typically due to
    /// device rejection, invalid callback URL, or network issues during subscription.
    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),
    
    /// Subscription renewal failed
    /// 
    /// This error occurs when attempting to renew an existing subscription fails,
    /// which may happen if the subscription has expired or the device is unreachable.
    #[error("Subscription renewal failed: {0}")]
    RenewalFailed(String),
    
    /// Subscription has expired
    /// 
    /// This error indicates that a subscription operation was attempted on an
    /// expired subscription that needs to be renewed or recreated.
    #[error("Subscription expired")]
    SubscriptionExpired,
    
    /// Invalid callback URL
    /// 
    /// This error occurs when the provided callback URL for event subscriptions
    /// is malformed or not accessible by the Sonos device.
    #[error("Invalid callback URL: {0}")]
    InvalidCallbackUrl(String),
    
    /// Event parsing failed
    /// 
    /// This error occurs when event XML received from a UPnP subscription
    /// cannot be parsed into the expected event structure.
    #[error("Event parsing failed: {0}")]
    EventParsingFailed(String),
    
    /// Network communication error
    /// 
    /// This error occurs when there are network-level issues communicating
    /// with the device, such as connection timeouts or DNS resolution failures.
    #[error("Network error: {0}")]
    NetworkError(String),
    
    /// SOAP fault returned by device
    ///
    /// This error occurs when the device returns a SOAP fault response,
    /// indicating that the request was malformed or the operation failed.
    #[error("SOAP fault: error code {0}")]
    SoapFault(u16),

    /// Batch operation failed
    ///
    /// This error occurs when one or more operations in a batch fail.
    /// Contains a mapping of operation names to their respective errors.
    #[error("Batch operation failed: {failed_count} of {total_count} operations failed")]
    BatchFailed {
        failed_count: usize,
        total_count: usize,
        errors: HashMap<String, Box<ApiError>>,
    },

    /// Sequence operation failed
    ///
    /// This error occurs when an operation in a sequence fails, preventing
    /// subsequent operations from executing.
    #[error("Sequence failed at operation '{operation}' (step {step_index} of {total_steps}): {source}")]
    SequenceFailed {
        operation: String,
        step_index: usize,
        total_steps: usize,
        source: Box<ApiError>,
    },

    /// Conditional operation predicate failed
    ///
    /// This error occurs when a conditional operation's predicate function
    /// panics or otherwise fails to evaluate properly.
    #[error("Conditional operation predicate failed: {0}")]
    PredicateFailed(String),

    /// Operation timeout
    ///
    /// This error occurs when an operation exceeds its configured timeout
    /// duration without completing successfully.
    #[error("Operation timed out after {timeout_ms}ms")]
    OperationTimeout {
        timeout_ms: u64,
    },

    /// Retry limit exceeded
    ///
    /// This error occurs when an operation fails repeatedly and exceeds
    /// its configured maximum retry attempts.
    #[error("Operation failed after {attempts} retry attempts")]
    RetryLimitExceeded {
        attempts: usize,
        last_error: Box<ApiError>,
    },
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
    
    /// Create a new SubscriptionFailed error
    pub fn subscription_failed<S: Into<String>>(message: S) -> Self {
        Self::SubscriptionFailed(message.into())
    }
    
    /// Create a new RenewalFailed error
    pub fn renewal_failed<S: Into<String>>(message: S) -> Self {
        Self::RenewalFailed(message.into())
    }
    
    /// Create a new InvalidCallbackUrl error
    pub fn invalid_callback_url<S: Into<String>>(url: S) -> Self {
        Self::InvalidCallbackUrl(url.into())
    }
    
    /// Create a new EventParsingFailed error
    pub fn event_parsing_failed<S: Into<String>>(message: S) -> Self {
        Self::EventParsingFailed(message.into())
    }

    /// Create a new BatchFailed error
    pub fn batch_failed(failed_count: usize, total_count: usize, errors: HashMap<String, Box<ApiError>>) -> Self {
        Self::BatchFailed {
            failed_count,
            total_count,
            errors,
        }
    }

    /// Create a new SequenceFailed error
    pub fn sequence_failed<S: Into<String>>(
        operation: S,
        step_index: usize,
        total_steps: usize,
        source: ApiError,
    ) -> Self {
        Self::SequenceFailed {
            operation: operation.into(),
            step_index,
            total_steps,
            source: Box::new(source),
        }
    }

    /// Create a new PredicateFailed error
    pub fn predicate_failed<S: Into<String>>(message: S) -> Self {
        Self::PredicateFailed(message.into())
    }

    /// Create a new OperationTimeout error
    pub fn operation_timeout(timeout_ms: u64) -> Self {
        Self::OperationTimeout { timeout_ms }
    }

    /// Create a new RetryLimitExceeded error
    pub fn retry_limit_exceeded(attempts: usize, last_error: ApiError) -> Self {
        Self::RetryLimitExceeded {
            attempts,
            last_error: Box::new(last_error),
        }
    }
}

/// Convenience type alias for Results with ApiError
pub type Result<T> = std::result::Result<T, ApiError>;

/// Operation context for enhanced error reporting
///
/// This struct provides additional context about where an error occurred
/// within a composite operation (batch, sequence, conditional).
#[derive(Debug, Clone, PartialEq)]
pub struct OperationContext {
    /// The name of the operation that failed
    pub operation_name: String,
    /// The service the operation belongs to
    pub service: String,
    /// The action name within the service
    pub action: String,
    /// Additional context-specific metadata
    pub metadata: HashMap<String, String>,
}

impl OperationContext {
    /// Create a new operation context
    pub fn new<S1, S2, S3>(operation_name: S1, service: S2, action: S3) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
    {
        Self {
            operation_name: operation_name.into(),
            service: service.into(),
            action: action.into(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the context
    pub fn with_metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Enhanced error result that includes operation context
///
/// This type provides additional context about operation failures,
/// making it easier to debug issues in composite operations.
pub type ContextualResult<T> = std::result::Result<T, (ApiError, OperationContext)>;

/// Helper trait for adding context to errors
pub trait WithContext<T> {
    /// Add operation context to an error result
    fn with_context(self, context: OperationContext) -> ContextualResult<T>;

    /// Add operation context with convenience method
    fn with_operation_context<S1, S2, S3>(
        self,
        operation_name: S1,
        service: S2,
        action: S3,
    ) -> ContextualResult<T>
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>;
}

impl<T> WithContext<T> for Result<T> {
    fn with_context(self, context: OperationContext) -> ContextualResult<T> {
        self.map_err(|err| (err, context))
    }

    fn with_operation_context<S1, S2, S3>(
        self,
        operation_name: S1,
        service: S2,
        action: S3,
    ) -> ContextualResult<T>
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
    {
        let context = OperationContext::new(operation_name, service, action);
        self.with_context(context)
    }
}

/// Statistics for batch operation results
#[derive(Debug, Clone, PartialEq)]
pub struct BatchStatistics {
    /// Total number of operations in the batch
    pub total_operations: usize,
    /// Number of operations that succeeded
    pub successful_operations: usize,
    /// Number of operations that failed
    pub failed_operations: usize,
    /// Success rate as a percentage (0.0 to 100.0)
    pub success_rate: f64,
}

impl BatchStatistics {
    /// Create new batch statistics
    pub fn new(total: usize, successful: usize) -> Self {
        let failed = total - successful;
        let success_rate = if total > 0 {
            (successful as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        Self {
            total_operations: total,
            successful_operations: successful,
            failed_operations: failed,
            success_rate,
        }
    }

    /// Check if all operations succeeded
    pub fn is_complete_success(&self) -> bool {
        self.failed_operations == 0 && self.total_operations > 0
    }

    /// Check if all operations failed
    pub fn is_complete_failure(&self) -> bool {
        self.successful_operations == 0 && self.total_operations > 0
    }

    /// Check if this was a partial success (some operations succeeded, some failed)
    pub fn is_partial_success(&self) -> bool {
        self.successful_operations > 0 && self.failed_operations > 0
    }
}

// Implement conversion from ValidationError to ApiError
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
                    // Subscription-specific error codes
                    412 => ApiError::SubscriptionFailed("Precondition failed".to_string()),
                    500 => ApiError::SubscriptionFailed("Internal server error".to_string()),
                    501 => ApiError::UnsupportedOperation,
                    503 => ApiError::SubscriptionFailed("Service unavailable".to_string()),
                    _ => ApiError::Soap(soap_error),
                }
            }
            _ => ApiError::Soap(soap_error),
        }
    }
}