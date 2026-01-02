//! Operation builder pattern for fluent operation construction
//!
//! This module provides the builder pattern for constructing UPnP operations
//! with validation, timeout, and retry configuration.

use super::{UPnPOperation, ValidationLevel, ValidationError, Validate, RetryPolicy, OperationMetadata};
use std::marker::PhantomData;
use std::time::Duration;

/// Builder for constructing UPnP operations with configuration
///
/// The OperationBuilder allows for fluent construction of operations with
/// validation levels, timeouts, retry policies, and other configuration.
///
/// # Type Parameters
/// * `Op` - The UPnP operation type being built
pub struct OperationBuilder<Op: UPnPOperation> {
    request: Op::Request,
    validation: ValidationLevel,
    timeout: Option<Duration>,
    retry_policy: Option<RetryPolicy>,
    _phantom: PhantomData<Op>,
}

impl<Op: UPnPOperation> OperationBuilder<Op> {
    /// Create a new operation builder with the given request
    ///
    /// # Arguments
    /// * `request` - The typed request data for the operation
    ///
    /// # Returns
    /// A new operation builder with default configuration
    pub fn new(request: Op::Request) -> Self {
        Self {
            request,
            validation: ValidationLevel::default(),
            timeout: None,
            retry_policy: None,
            _phantom: PhantomData,
        }
    }

    /// Set the validation level for the operation
    ///
    /// # Arguments
    /// * `level` - The validation level to use
    ///
    /// # Returns
    /// The builder for method chaining
    pub fn with_validation(mut self, level: ValidationLevel) -> Self {
        self.validation = level;
        self
    }

    /// Set a timeout for the operation
    ///
    /// # Arguments
    /// * `timeout` - The timeout duration
    ///
    /// # Returns
    /// The builder for method chaining
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set a retry policy for the operation
    ///
    /// # Arguments
    /// * `policy` - The retry policy to use
    ///
    /// # Returns
    /// The builder for method chaining
    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = Some(policy);
        self
    }

    /// Disable retries for the operation
    ///
    /// # Returns
    /// The builder for method chaining
    pub fn without_retry(mut self) -> Self {
        self.retry_policy = Some(RetryPolicy::none());
        self
    }

    /// Build the final composable operation
    ///
    /// This validates the request according to the configured validation level
    /// and creates a ComposableOperation ready for execution.
    ///
    /// # Returns
    /// A ComposableOperation or a validation error
    pub fn build(self) -> Result<ComposableOperation<Op>, ValidationError> {
        // Validate the request according to the configured level
        self.request.validate(self.validation)?;

        Ok(ComposableOperation {
            request: self.request,
            validation: self.validation,
            timeout: self.timeout,
            retry_policy: self.retry_policy.unwrap_or_default(),
            metadata: Op::metadata(),
            _phantom: PhantomData,
        })
    }

    /// Build without validation (for performance-critical scenarios)
    ///
    /// This bypasses validation and creates the operation directly.
    /// Use with caution - invalid requests may cause runtime errors.
    ///
    /// # Returns
    /// A ComposableOperation without validation
    pub fn build_unchecked(self) -> ComposableOperation<Op> {
        ComposableOperation {
            request: self.request,
            validation: ValidationLevel::None,
            timeout: self.timeout,
            retry_policy: self.retry_policy.unwrap_or_default(),
            metadata: Op::metadata(),
            _phantom: PhantomData,
        }
    }

    /// Get the current validation level
    pub fn validation_level(&self) -> ValidationLevel {
        self.validation
    }

    /// Get the current timeout setting
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Get the current retry policy
    pub fn retry_policy(&self) -> Option<&RetryPolicy> {
        self.retry_policy.as_ref()
    }
}

/// A composable operation ready for execution
///
/// This represents a fully configured UPnP operation that can be executed
/// directly or composed with other operations through chaining, batching, etc.
///
/// # Type Parameters
/// * `Op` - The UPnP operation type
pub struct ComposableOperation<Op: UPnPOperation> {
    pub(crate) request: Op::Request,
    pub(crate) validation: ValidationLevel,
    pub(crate) timeout: Option<Duration>,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) metadata: OperationMetadata,
    _phantom: PhantomData<Op>,
}

impl<Op: UPnPOperation> ComposableOperation<Op> {
    /// Get the request data for this operation
    pub fn request(&self) -> &Op::Request {
        &self.request
    }

    /// Get the validation level used for this operation
    pub fn validation_level(&self) -> ValidationLevel {
        self.validation
    }

    /// Get the timeout for this operation
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Get the retry policy for this operation
    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    /// Get the operation metadata
    pub fn metadata(&self) -> &OperationMetadata {
        &self.metadata
    }

    /// Build the SOAP payload for this operation
    ///
    /// # Returns
    /// The XML payload string or a validation error
    pub fn build_payload(&self) -> Result<String, ValidationError> {
        Op::build_payload(&self.request)
    }

    /// Parse a response for this operation
    ///
    /// # Arguments
    /// * `xml` - The parsed XML response element
    ///
    /// # Returns
    /// The parsed response or an API error
    pub fn parse_response(&self, xml: &xmltree::Element) -> Result<Op::Response, crate::error::ApiError> {
        Op::parse_response(xml)
    }
}

impl<Op: UPnPOperation> std::fmt::Debug for ComposableOperation<Op> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComposableOperation")
            .field("service", &self.metadata.service)
            .field("action", &self.metadata.action)
            .field("validation", &self.validation)
            .field("timeout", &self.timeout)
            .field("retry_policy", &self.retry_policy)
            .finish()
    }
}

impl<Op: UPnPOperation> Clone for ComposableOperation<Op>
where
    Op::Request: Clone
{
    fn clone(&self) -> Self {
        Self {
            request: self.request.clone(),
            validation: self.validation,
            timeout: self.timeout,
            retry_policy: self.retry_policy.clone(),
            metadata: self.metadata.clone(),
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{ValidationLevel, ValidationError, Validate};
    use crate::service::Service;
    use serde::{Serialize, Deserialize};
    use xmltree::Element;

    // Mock types for testing
    #[derive(Serialize, Clone, Debug, PartialEq)]
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

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestResponse {
        result: String,
    }

    struct TestOperation;

    impl UPnPOperation for TestOperation {
        type Request = TestRequest;
        type Response = TestResponse;

        const SERVICE: Service = Service::AVTransport;
        const ACTION: &'static str = "TestAction";

        fn build_payload(request: &Self::Request) -> Result<String, ValidationError> {
            request.validate(ValidationLevel::Boundary)?;
            Ok(format!("<TestRequest><Value>{}</Value></TestRequest>", request.value))
        }

        fn parse_response(xml: &Element) -> Result<Self::Response, crate::error::ApiError> {
            Ok(TestResponse {
                result: xml.get_child("Result")
                    .and_then(|e| e.get_text())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "default".to_string()),
            })
        }
    }

    #[test]
    fn test_operation_builder_new() {
        let request = TestRequest { value: 50 };
        let builder = OperationBuilder::<TestOperation>::new(request);

        assert_eq!(builder.validation_level(), ValidationLevel::Boundary);
        assert_eq!(builder.timeout(), None);
        assert!(builder.retry_policy().is_none());
    }

    #[test]
    fn test_operation_builder_fluent() {
        let request = TestRequest { value: 50 };
        let builder = OperationBuilder::<TestOperation>::new(request)
            .with_validation(ValidationLevel::Comprehensive)
            .with_timeout(Duration::from_secs(30))
            .with_retry(RetryPolicy::fixed(5, Duration::from_millis(200)));

        assert_eq!(builder.validation_level(), ValidationLevel::Comprehensive);
        assert_eq!(builder.timeout(), Some(Duration::from_secs(30)));
        assert_eq!(builder.retry_policy().unwrap().max_retries, 5);
    }

    #[test]
    fn test_operation_builder_build_success() {
        let request = TestRequest { value: 50 };
        let operation = OperationBuilder::<TestOperation>::new(request)
            .with_validation(ValidationLevel::Comprehensive)
            .build()
            .expect("Should build successfully");

        assert_eq!(operation.request().value, 50);
        assert_eq!(operation.validation_level(), ValidationLevel::Comprehensive);
        assert_eq!(operation.metadata().action, "TestAction");
    }

    #[test]
    fn test_operation_builder_build_validation_error() {
        let request = TestRequest { value: 150 }; // Invalid value
        let result = OperationBuilder::<TestOperation>::new(request)
            .with_validation(ValidationLevel::Comprehensive)
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("150"));
    }

    #[test]
    fn test_operation_builder_build_unchecked() {
        let request = TestRequest { value: 150 }; // Invalid value
        let operation = OperationBuilder::<TestOperation>::new(request)
            .with_validation(ValidationLevel::Comprehensive)
            .build_unchecked(); // Should succeed despite invalid value

        assert_eq!(operation.request().value, 150);
        assert_eq!(operation.validation_level(), ValidationLevel::None);
    }

    #[test]
    fn test_composable_operation_build_payload() {
        let request = TestRequest { value: 42 };
        let operation = OperationBuilder::<TestOperation>::new(request)
            .build()
            .expect("Should build successfully");

        let payload = operation.build_payload().expect("Should build payload");
        assert!(payload.contains("<Value>42</Value>"));
    }

    #[test]
    fn test_composable_operation_debug() {
        let request = TestRequest { value: 42 };
        let operation = OperationBuilder::<TestOperation>::new(request)
            .with_timeout(Duration::from_secs(10))
            .build()
            .expect("Should build successfully");

        let debug_str = format!("{:?}", operation);
        assert!(debug_str.contains("TestAction"));
        assert!(debug_str.contains("AVTransport"));
    }

    #[test]
    fn test_composable_operation_clone() {
        let request = TestRequest { value: 42 };
        let operation = OperationBuilder::<TestOperation>::new(request)
            .build()
            .expect("Should build successfully");

        let cloned = operation.clone();
        assert_eq!(operation.request().value, cloned.request().value);
        assert_eq!(operation.validation_level(), cloned.validation_level());
    }
}