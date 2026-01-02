//! Enhanced operation framework with composability and validation support
//!
//! This module provides the core framework for UPnP operations with advanced features:
//! - Composable operations that can be chained, batched, or made conditional
//! - Dual validation strategy (boundary vs comprehensive)
//! - Fluent builder pattern for operation construction
//! - Strong type safety with minimal boilerplate

mod builder;
mod composition;
pub mod macros;

pub use builder::*;
pub use composition::*;

// Legacy SonosOperation trait for backward compatibility
use serde::{Deserialize, Serialize};
use xmltree::Element;

use crate::error::ApiError;
use crate::service::Service;

/// Base trait for all Sonos API operations (LEGACY)
///
/// This trait defines the common interface that all Sonos UPnP operations must implement.
/// It provides type safety through associated types and ensures consistent patterns
/// for request/response handling across all operations.
///
/// **Note**: This is the legacy trait. New code should use `UPnPOperation` instead.
pub trait SonosOperation {
    /// The request type for this operation, must be serializable
    type Request: Serialize;

    /// The response type for this operation, must be deserializable
    type Response: for<'de> Deserialize<'de>;

    /// The UPnP service this operation belongs to
    const SERVICE: Service;

    /// The SOAP action name for this operation
    const ACTION: &'static str;

    /// Build the SOAP payload from the request data
    ///
    /// This method should construct the XML payload that goes inside the SOAP envelope.
    /// The payload should contain all the parameters needed for the UPnP action.
    ///
    /// # Arguments
    /// * `request` - The typed request data
    ///
    /// # Returns
    /// A string containing the XML payload (without SOAP envelope)
    fn build_payload(request: &Self::Request) -> String;

    /// Parse the SOAP response XML into the typed response
    ///
    /// This method extracts the relevant data from the SOAP response XML and
    /// converts it into the strongly-typed response structure.
    ///
    /// # Arguments
    /// * `xml` - The parsed XML element containing the response data
    ///
    /// # Returns
    /// The typed response data or an error if parsing fails
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError>;
}

use std::time::Duration;

/// Validation error types
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Parameter '{parameter}' value '{value}' is out of range ({min}..={max})")]
    RangeError {
        parameter: String,
        value: String,
        min: String,
        max: String,
    },

    #[error("Parameter '{parameter}' value '{value}' is invalid: {reason}")]
    InvalidValue {
        parameter: String,
        value: String,
        reason: String,
    },

    #[error("Required parameter '{parameter}' is missing")]
    MissingParameter { parameter: String },

    #[error("Parameter '{parameter}' failed validation: {message}")]
    Custom { parameter: String, message: String },
}

impl ValidationError {
    pub fn range_error(parameter: &str, min: impl std::fmt::Display, max: impl std::fmt::Display, value: impl std::fmt::Display) -> Self {
        Self::RangeError {
            parameter: parameter.to_string(),
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        }
    }

    pub fn invalid_value(parameter: &str, value: impl std::fmt::Display) -> Self {
        Self::InvalidValue {
            parameter: parameter.to_string(),
            value: value.to_string(),
            reason: "invalid format or content".to_string(),
        }
    }
}

/// Validation levels for operation parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationLevel {
    /// No validation - maximum performance
    None,
    /// Light validation at API boundary - basic type and range checks
    Boundary,
    /// Full validation including domain rules and complex constraints
    Comprehensive,
}

impl Default for ValidationLevel {
    fn default() -> Self {
        Self::Boundary
    }
}

/// Trait for types that can be validated
pub trait Validate {
    /// Perform light validation at the API boundary
    ///
    /// This should include basic type checks and simple range validation
    /// to fail fast on obviously invalid input.
    fn validate_boundary(&self) -> Result<(), ValidationError> {
        Ok(()) // Default: no boundary validation
    }

    /// Perform comprehensive validation including domain rules
    ///
    /// This includes all boundary validation plus complex business logic,
    /// regex patterns, cross-field validation, etc.
    fn validate_comprehensive(&self) -> Result<(), ValidationError> {
        self.validate_boundary() // Default: same as boundary validation
    }

    /// Validate with the specified level
    fn validate(&self, level: ValidationLevel) -> Result<(), ValidationError> {
        match level {
            ValidationLevel::None => Ok(()),
            ValidationLevel::Boundary => self.validate_boundary(),
            ValidationLevel::Comprehensive => self.validate_comprehensive(),
        }
    }
}

/// Enhanced UPnP operation trait with composability support
///
/// This trait extends the original SonosOperation concept with:
/// - Composability: operations can be chained, batched, or made conditional
/// - Validation: flexible validation strategy with boundary and comprehensive levels
/// - Dependencies: operations can declare dependencies on other operations
/// - Batching: operations can indicate whether they can be batched with others
pub trait UPnPOperation {
    /// The request type for this operation, must be serializable and validatable
    type Request: Serialize + Validate;

    /// The response type for this operation, must be deserializable
    type Response: for<'de> Deserialize<'de>;

    /// The UPnP service this operation belongs to
    const SERVICE: Service;

    /// The SOAP action name for this operation
    const ACTION: &'static str;

    /// Build the SOAP payload from the request data with validation
    ///
    /// This method validates the request according to the validation level
    /// and then constructs the XML payload for the SOAP envelope.
    ///
    /// # Arguments
    /// * `request` - The typed request data
    ///
    /// # Returns
    /// A string containing the XML payload or a validation error
    fn build_payload(request: &Self::Request) -> Result<String, ValidationError>;

    /// Parse the SOAP response XML into the typed response
    ///
    /// This method extracts the relevant data from the SOAP response XML and
    /// converts it into the strongly-typed response structure.
    ///
    /// # Arguments
    /// * `xml` - The parsed XML element containing the response data
    ///
    /// # Returns
    /// The typed response data or an error if parsing fails
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError>;

    /// Get the list of operations this operation depends on
    ///
    /// This is used for operation ordering and dependency resolution
    /// in batch and sequence operations.
    ///
    /// # Returns
    /// A slice of action names that must be executed before this operation
    fn dependencies() -> &'static [&'static str] {
        &[]
    }

    /// Check if this operation can be batched with another operation
    ///
    /// Some operations may have conflicts or dependencies that prevent
    /// them from being executed in parallel.
    ///
    /// # Type Parameters
    /// * `T` - Another UPnP operation type to check compatibility with
    ///
    /// # Returns
    /// True if the operations can be safely executed in parallel
    fn can_batch_with<T: UPnPOperation>() -> bool {
        true // Default: most operations can be batched
    }

    /// Get human-readable operation metadata
    ///
    /// This is useful for debugging, logging, and SDK development
    fn metadata() -> OperationMetadata {
        OperationMetadata {
            service: Self::SERVICE.name(),
            action: Self::ACTION,
            dependencies: Self::dependencies(),
        }
    }
}

/// Metadata about a UPnP operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationMetadata {
    /// The service name (e.g., "AVTransport")
    pub service: &'static str,
    /// The action name (e.g., "Play")
    pub action: &'static str,
    /// List of operations this operation depends on
    pub dependencies: &'static [&'static str],
}

/// Retry policy for operation execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay between retries
    pub base_delay: Duration,
    /// Whether to use exponential backoff
    pub exponential_backoff: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            exponential_backoff: true,
        }
    }
}

impl RetryPolicy {
    /// Create a retry policy with no retries
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            base_delay: Duration::ZERO,
            exponential_backoff: false,
        }
    }

    /// Create a retry policy with fixed delays
    pub fn fixed(max_retries: u32, delay: Duration) -> Self {
        Self {
            max_retries,
            base_delay: delay,
            exponential_backoff: false,
        }
    }

    /// Create a retry policy with exponential backoff
    pub fn exponential(max_retries: u32, base_delay: Duration) -> Self {
        Self {
            max_retries,
            base_delay,
            exponential_backoff: true,
        }
    }

    /// Calculate the delay for a given retry attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 || attempt > self.max_retries {
            return Duration::ZERO;
        }

        if self.exponential_backoff {
            self.base_delay * 2_u32.pow(attempt - 1)
        } else {
            self.base_delay
        }
    }
}

/// Result type for sequence operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceResult<T> {
    /// All operations in the sequence completed successfully
    Success(T),
}

/// Result type for batch operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchResult<T> {
    /// All operations in the batch completed (some may have failed)
    Complete(T),
}

/// Result type for conditional operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalResult<T> {
    /// The operation was executed and completed with this result
    Executed(T),
    /// The operation was skipped due to condition not being met
    Skipped,
}

impl<T> ConditionalResult<T> {
    /// Check if the operation was executed
    pub fn was_executed(&self) -> bool {
        matches!(self, ConditionalResult::Executed(_))
    }

    /// Check if the operation was skipped
    pub fn was_skipped(&self) -> bool {
        matches!(self, ConditionalResult::Skipped)
    }

    /// Get the result if the operation was executed
    pub fn result(self) -> Option<T> {
        match self {
            ConditionalResult::Executed(result) => Some(result),
            ConditionalResult::Skipped => None,
        }
    }

    /// Get a reference to the result if the operation was executed
    pub fn result_ref(&self) -> Option<&T> {
        match self {
            ConditionalResult::Executed(result) => Some(result),
            ConditionalResult::Skipped => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error_creation() {
        let error = ValidationError::range_error("volume", 0, 100, 150);
        assert!(error.to_string().contains("volume"));
        assert!(error.to_string().contains("150"));
        assert!(error.to_string().contains("0..=100"));
    }

    #[test]
    fn test_validation_level_default() {
        assert_eq!(ValidationLevel::default(), ValidationLevel::Boundary);
    }

    #[test]
    fn test_retry_policy_none() {
        let policy = RetryPolicy::none();
        assert_eq!(policy.max_retries, 0);
        assert_eq!(policy.delay_for_attempt(1), Duration::ZERO);
    }

    #[test]
    fn test_retry_policy_fixed() {
        let policy = RetryPolicy::fixed(3, Duration::from_millis(500));
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(500));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(500));
    }

    #[test]
    fn test_retry_policy_exponential() {
        let policy = RetryPolicy::exponential(3, Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(4), Duration::ZERO); // Beyond max_retries
    }

    // Mock validation implementation for testing
    struct TestRequest {
        value: i32,
    }

    impl Validate for TestRequest {
        fn validate_boundary(&self) -> Result<(), ValidationError> {
            if self.value < 0 {
                Err(ValidationError::range_error("value", 0, 100, self.value))
            } else {
                Ok(())
            }
        }

        fn validate_comprehensive(&self) -> Result<(), ValidationError> {
            self.validate_boundary()?;
            if self.value > 100 {
                Err(ValidationError::range_error("value", 0, 100, self.value))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn test_validation_levels() {
        let valid_request = TestRequest { value: 50 };
        assert!(valid_request.validate(ValidationLevel::None).is_ok());
        assert!(valid_request.validate(ValidationLevel::Boundary).is_ok());
        assert!(valid_request.validate(ValidationLevel::Comprehensive).is_ok());

        let boundary_invalid = TestRequest { value: -10 };
        assert!(boundary_invalid.validate(ValidationLevel::None).is_ok());
        assert!(boundary_invalid.validate(ValidationLevel::Boundary).is_err());
        assert!(boundary_invalid.validate(ValidationLevel::Comprehensive).is_err());

        let comprehensive_invalid = TestRequest { value: 150 };
        assert!(comprehensive_invalid.validate(ValidationLevel::None).is_ok());
        assert!(comprehensive_invalid.validate(ValidationLevel::Boundary).is_ok());
        assert!(comprehensive_invalid.validate(ValidationLevel::Comprehensive).is_err());
    }
}