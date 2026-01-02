use soap_client::SoapClient;
use crate::{ApiError, Result, SonosOperation, Service, ManagedSubscription};
use crate::operation::{
    UPnPOperation, ComposableOperation, OperationSequence, OperationBatch,
    ConditionalOperation, SequenceResult, BatchResult, ConditionalResult
};
use std::time::{Duration, Instant};

/// A client for executing Sonos operations against actual devices
/// 
/// This client bridges the gap between the stateless operation definitions
/// and actual network requests to Sonos speakers. It uses the soap-client
/// crate to handle the underlying SOAP communication.
///
/// # Subscription Management
///
/// The primary API for managing UPnP event subscriptions is `create_managed_subscription()`,
/// which returns a `ManagedSubscription` that handles all lifecycle management:
///
/// ```rust
/// use sonos_api::{SonosClient, Service};
///
/// let client = SonosClient::new();
/// let subscription = client.create_managed_subscription(
///     "192.168.1.100",
///     Service::AVTransport,
///     "http://callback.url",
///     1800
/// )?;
///
/// // Subscription handles renewal and cleanup automatically
/// ```
#[derive(Debug, Clone)]
pub struct SonosClient {
    soap_client: SoapClient,
}

impl SonosClient {
    /// Create a new Sonos client using the shared SOAP client
    ///
    /// This uses the global shared SOAP client instance for maximum resource efficiency.
    /// All SonosClient instances created this way share the same underlying HTTP client
    /// and connection pool, reducing memory usage and improving performance.
    pub fn new() -> Self {
        Self {
            soap_client: SoapClient::get().clone(),
        }
    }

    /// Create a Sonos client with a custom SOAP client (for advanced use cases)
    ///
    /// Most applications should use `SonosClient::new()` instead. This method is
    /// provided for cases where custom SOAP client configuration is needed.
    pub fn with_soap_client(soap_client: SoapClient) -> Self {
        Self { soap_client }
    }

    /// Execute a Sonos operation against a device
    /// 
    /// This method takes any operation that implements `SonosOperation`,
    /// constructs the appropriate SOAP request, sends it to the device,
    /// and parses the response.
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `request` - The operation request data
    /// 
    /// # Returns
    /// The parsed response data or an error
    /// 
    /// # Example
    /// ```rust
    /// use sonos_api::client::SonosClient;
    /// use sonos_api::operations::av_transport::{GetTransportInfoOperation, GetTransportInfoRequest};
    /// 
    /// let client = SonosClient::new();
    /// let request = GetTransportInfoRequest { instance_id: 0 };
    /// 
    /// // Execute the operation (this would require an actual device)
    /// // let response = client.execute::<GetTransportInfoOperation>("192.168.1.100", &request)?;
    /// ```
    pub fn execute<Op: SonosOperation>(
        &self,
        ip: &str,
        request: &Op::Request,
    ) -> Result<Op::Response> {
        let service_info = Op::SERVICE.info();
        let payload = Op::build_payload(request);
        
        let xml = self.soap_client
            .call(
                ip,
                service_info.endpoint,
                service_info.service_uri,
                Op::ACTION,
                &payload,
            )
            .map_err(|e| match e {
                soap_client::SoapError::Network(msg) => ApiError::NetworkError(msg),
                soap_client::SoapError::Parse(msg) => ApiError::ParseError(msg),
                soap_client::SoapError::Fault(code) => ApiError::SoapFault(code),
            })?;
            
        Op::parse_response(&xml)
    }

    /// Execute an enhanced UPnP operation with composability features
    ///
    /// This method executes a ComposableOperation that was built using the new
    /// enhanced operation framework with validation, retry policies, and timeouts.
    ///
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `operation` - A ComposableOperation instance
    ///
    /// # Returns
    /// The parsed response data or an error
    ///
    /// # Example
    /// ```rust,ignore
    /// use sonos_api::operation::{OperationBuilder, ValidationLevel};
    /// use sonos_api::services::av_transport;
    ///
    /// let client = SonosClient::new();
    /// let play_op = av_transport::play("1".to_string())
    ///     .with_validation(ValidationLevel::Comprehensive)
    ///     .build()?;
    ///
    /// let response = client.execute_enhanced("192.168.1.100", play_op)?;
    /// ```
    pub fn execute_enhanced<Op: UPnPOperation>(
        &self,
        ip: &str,
        operation: ComposableOperation<Op>,
    ) -> Result<Op::Response> {
        // Apply timeout if specified
        let start_time = Instant::now();

        // Build payload (includes validation)
        let payload = operation.build_payload()
            .map_err(|e| ApiError::ParseError(format!("Validation error: {}", e)))?;

        let service_info = Op::SERVICE.info();

        let mut attempt = 0;
        let retry_policy = operation.retry_policy();

        loop {
            // Check timeout before each attempt
            if let Some(timeout) = operation.timeout() {
                if start_time.elapsed() >= timeout {
                    return Err(ApiError::NetworkError("Operation timeout".to_string()));
                }
            }

            // Execute SOAP call
            match self.soap_client.call(
                ip,
                service_info.endpoint,
                service_info.service_uri,
                Op::ACTION,
                &payload,
            ) {
                Ok(xml) => {
                    return operation.parse_response(&xml);
                }
                Err(e) => {
                    attempt += 1;

                    // Check if we should retry
                    if attempt <= retry_policy.max_retries {
                        let delay = retry_policy.delay_for_attempt(attempt);
                        if delay > Duration::ZERO {
                            std::thread::sleep(delay);
                        }
                        continue;
                    }

                    // No more retries, return error
                    return Err(match e {
                        soap_client::SoapError::Network(msg) => ApiError::NetworkError(msg),
                        soap_client::SoapError::Parse(msg) => ApiError::ParseError(msg),
                        soap_client::SoapError::Fault(code) => ApiError::SoapFault(code),
                    });
                }
            }
        }
    }

    /// Execute a sequence of operations in order
    ///
    /// Operations in the sequence will be executed one after another. If any operation
    /// fails, the sequence stops and returns an error. The sequence result contains
    /// the results of all successfully executed operations.
    ///
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `sequence` - An OperationSequence to execute
    ///
    /// # Returns
    /// A SequenceResult containing all operation results or an error
    pub fn execute_sequence<Op1, Op2>(
        &self,
        ip: &str,
        sequence: OperationSequence<(ComposableOperation<Op1>, ComposableOperation<Op2>)>,
    ) -> Result<SequenceResult<(Op1::Response, Op2::Response)>>
    where
        Op1: UPnPOperation,
        Op2: UPnPOperation,
    {
        let (op1, op2) = sequence.into_operations();

        // Execute first operation
        let result1 = self.execute_enhanced(ip, op1)?;

        // Execute second operation
        let result2 = self.execute_enhanced(ip, op2)?;

        Ok(SequenceResult::Success((result1, result2)))
    }

    /// Execute a batch of operations concurrently
    ///
    /// Operations in the batch will be executed simultaneously where possible.
    /// The batch result contains the results of all operations, including any failures.
    ///
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `batch` - An OperationBatch to execute
    ///
    /// # Returns
    /// A BatchResult containing all operation results
    pub fn execute_batch<Op1, Op2>(
        &self,
        ip: &str,
        batch: OperationBatch<(ComposableOperation<Op1>, ComposableOperation<Op2>)>,
    ) -> Result<BatchResult<(Result<Op1::Response>, Result<Op2::Response>)>>
    where
        Op1: UPnPOperation,
        Op2: UPnPOperation,
    {
        let (op1, op2) = batch.into_operations();

        // For now, execute sequentially (true parallel execution would need async)
        // This can be enhanced later with proper async support
        let result1 = self.execute_enhanced(ip, op1);
        let result2 = self.execute_enhanced(ip, op2);

        Ok(BatchResult::Complete((result1, result2)))
    }

    /// Execute a conditional operation
    ///
    /// The operation will only be executed if the condition is met.
    ///
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `conditional` - A ConditionalOperation to execute
    ///
    /// # Returns
    /// A ConditionalResult indicating whether the operation was executed and its result
    pub fn execute_conditional<Op, F>(
        &self,
        ip: &str,
        conditional: ConditionalOperation<Op, F>,
    ) -> Result<ConditionalResult<Op::Response>>
    where
        Op: UPnPOperation,
        F: Fn() -> bool,
    {
        let (operation, predicate) = conditional.into_parts();

        if predicate() {
            let result = self.execute_enhanced(ip, operation)?;
            Ok(ConditionalResult::Executed(result))
        } else {
            Ok(ConditionalResult::Skipped)
        }
    }

    /// Create a managed subscription with lifecycle management
    ///
    /// This method creates a UPnP subscription and returns a `ManagedSubscription`
    /// that provides lifecycle management methods.
    ///
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `service` - The service to subscribe to
    /// * `callback_url` - The URL where events should be sent
    /// * `timeout_seconds` - Initial timeout for the subscription
    ///
    /// # Returns
    /// A `ManagedSubscription` that provides renewal and cleanup methods
    ///
    /// # Example
    /// ```rust
    /// use sonos_api::{SonosClient, Service};
    ///
    /// let client = SonosClient::new();
    /// let subscription = client.create_managed_subscription(
    ///     "192.168.1.100",
    ///     Service::AVTransport,
    ///     "http://192.168.1.50:8080/callback",
    ///     1800
    /// )?;
    ///
    /// // Check if renewal is needed and renew if so
    /// if subscription.needs_renewal() {
    ///     subscription.renew()?;
    /// }
    ///
    /// // Clean up when done
    /// subscription.unsubscribe()?;
    /// ```
    pub fn create_managed_subscription(
        &self,
        ip: &str,
        service: Service,
        callback_url: &str,
        timeout_seconds: u32,
    ) -> Result<ManagedSubscription> {
        ManagedSubscription::create(
            ip.to_string(),
            service,
            callback_url.to_string(),
            timeout_seconds,
            self.soap_client.clone(),
        )
    }
}

impl Default for SonosClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let _client = SonosClient::new();
        let _default_client = SonosClient::default();
    }
}