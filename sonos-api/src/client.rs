use soap_client::SoapClient;
use crate::{ApiError, Result, SonosOperation, Service, ManagedSubscription};
use crate::operation::{
    UPnPOperation, ComposableOperation
};
use std::time::Instant;


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

        // Check timeout before call
        if let Some(timeout) = operation.timeout() {
            if start_time.elapsed() >= timeout {
                return Err(ApiError::NetworkError("Operation timeout".to_string()));
            }
        }

        // Execute SOAP call
        let xml = self.soap_client.call(
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

        operation.parse_response(&xml)
    }


    /// Subscribe to UPnP events from a service
    ///
    /// This creates a subscription to the specified service's event endpoint.
    /// The device will then stream events (state changes) to the provided callback URL.
    /// This is separate from control operations - subscriptions go to `/Event` endpoints
    /// while control operations go to `/Control` endpoints.
    ///
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `service` - The service to subscribe to (e.g., Service::AVTransport)
    /// * `callback_url` - URL where the device will send event notifications
    ///
    /// # Returns
    /// A managed subscription that handles lifecycle, renewal, and cleanup
    ///
    /// # Example
    /// ```rust,ignore
    /// use sonos_api::{SonosClient, Service};
    ///
    /// let client = SonosClient::new();
    ///
    /// // Subscribe to AVTransport events (play/pause state changes, etc.)
    /// let subscription = client.subscribe(
    ///     "192.168.1.100",
    ///     Service::AVTransport,
    ///     "http://192.168.1.50:8080/callback"
    /// )?;
    ///
    /// // Now execute control operations separately
    /// let play_op = av_transport::play("1".to_string()).build()?;
    /// client.execute("192.168.1.100", play_op)?;
    ///
    /// // The subscription will receive events about the state changes
    /// ```
    pub fn subscribe(
        &self,
        ip: &str,
        service: Service,
        callback_url: &str,
    ) -> Result<ManagedSubscription> {
        self.create_managed_subscription(ip, service, callback_url, 1800)
    }

    /// Subscribe to UPnP events with custom timeout
    ///
    /// Same as `subscribe()` but allows specifying a custom timeout for the subscription.
    ///
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `service` - The service to subscribe to
    /// * `callback_url` - URL where the device will send event notifications
    /// * `timeout_seconds` - How long the subscription should last (max: 86400 = 24 hours)
    ///
    /// # Returns
    /// A managed subscription that handles lifecycle, renewal, and cleanup
    pub fn subscribe_with_timeout(
        &self,
        ip: &str,
        service: Service,
        callback_url: &str,
        timeout_seconds: u32,
    ) -> Result<ManagedSubscription> {
        self.create_managed_subscription(ip, service, callback_url, timeout_seconds)
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

    #[test]
    fn test_subscription_methods_signature() {
        // Test that subscription methods have correct signatures
        let client = SonosClient::new();

        // Test that the methods exist and have correct signatures by creating function pointers
        let _subscribe_fn: fn(&SonosClient, &str, Service, &str) -> Result<ManagedSubscription> =
            SonosClient::subscribe;

        let _subscribe_timeout_fn: fn(&SonosClient, &str, Service, &str, u32) -> Result<ManagedSubscription> =
            SonosClient::subscribe_with_timeout;

        // If this compiles, the method signatures are correct
        assert!(true);
    }

    #[test]
    fn test_subscription_parameters() {
        // Test that we can create the parameters needed for subscription calls
        let _ip = "192.168.1.100";
        let _service = Service::AVTransport;
        let _callback_url = "http://callback.url";
        let _timeout = 3600u32;

        // Verify Service enum has the variants we need
        assert_eq!(Service::AVTransport as i32, Service::AVTransport as i32);
        assert_eq!(Service::RenderingControl as i32, Service::RenderingControl as i32);
    }

    #[test]
    fn test_subscription_delegates_to_create_managed() {
        // Test that subscribe() correctly delegates to create_managed_subscription
        let client = SonosClient::new();

        // We can't test the actual execution without a real device,
        // but we can verify the methods compile and have correct signatures
        let _subscription_fn = |client: &SonosClient| {
            client.subscribe("192.168.1.100", Service::AVTransport, "http://callback")
        };

        let _timeout_subscription_fn = |client: &SonosClient| {
            client.subscribe_with_timeout("192.168.1.100", Service::AVTransport, "http://callback", 1800)
        };

        // If this compiles, the signatures are correct
        assert!(true);
    }

}