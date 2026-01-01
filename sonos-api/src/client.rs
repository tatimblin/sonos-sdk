use soap_client::SoapClient;
use crate::{ApiError, Result, SonosOperation, Service, ManagedSubscription};

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