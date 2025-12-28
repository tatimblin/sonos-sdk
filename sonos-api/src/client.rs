use soap_client::SoapClient;
use crate::{ApiError, Result, SonosOperation, Service};
use crate::operations::events::{
    SubscribeOperation, SubscribeRequest, SubscribeResponse,
    UnsubscribeOperation, UnsubscribeRequest, UnsubscribeResponse,
    RenewOperation, RenewRequest, RenewResponse,
};

/// A client for executing Sonos operations against actual devices
/// 
/// This client bridges the gap between the stateless operation definitions
/// and actual network requests to Sonos speakers. It uses the soap-client
/// crate to handle the underlying SOAP communication.
#[derive(Debug, Clone)]
pub struct SonosClient {
    soap_client: SoapClient,
}

impl SonosClient {
    /// Create a new Sonos client with default configuration
    pub fn new() -> Self {
        Self {
            soap_client: SoapClient::new(),
        }
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

    /// Subscribe to UPnP events for a specific service
    /// 
    /// This method creates a new UPnP event subscription for the specified service
    /// on the target device. Events will be sent to the provided callback URL.
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `service` - The service to subscribe to (e.g., Service::AVTransport)
    /// * `request` - The subscription request parameters
    /// 
    /// # Returns
    /// The subscription response containing SID and timeout
    /// 
    /// # Example
    /// ```rust
    /// use sonos_api::{SonosClient, Service, operations::events::SubscribeRequest};
    /// 
    /// let client = SonosClient::new();
    /// let request = SubscribeRequest {
    ///     callback_url: "http://192.168.1.50:8080/callback".to_string(),
    ///     timeout_seconds: 1800,
    /// };
    /// 
    /// // Subscribe to AVTransport events
    /// // let response = client.subscribe("192.168.1.100", Service::AVTransport, &request)?;
    /// ```
    pub fn subscribe(
        &self,
        ip: &str,
        service: Service,
        request: &SubscribeRequest,
    ) -> Result<SubscribeResponse> {
        SubscribeOperation::execute(&self.soap_client, ip, service, request)
    }

    /// Unsubscribe from UPnP events
    /// 
    /// This method cancels an existing UPnP event subscription using the
    /// subscription ID (SID) returned from a previous subscribe call.
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `service` - The service to unsubscribe from
    /// * `request` - The unsubscribe request parameters
    /// 
    /// # Returns
    /// An empty response on success
    /// 
    /// # Example
    /// ```rust
    /// use sonos_api::{SonosClient, Service, operations::events::UnsubscribeRequest};
    /// 
    /// let client = SonosClient::new();
    /// let request = UnsubscribeRequest {
    ///     sid: "uuid:12345678-1234-1234-1234-123456789012".to_string(),
    /// };
    /// 
    /// // Unsubscribe from AVTransport events
    /// // client.unsubscribe("192.168.1.100", Service::AVTransport, &request)?;
    /// ```
    pub fn unsubscribe(
        &self,
        ip: &str,
        service: Service,
        request: &UnsubscribeRequest,
    ) -> Result<UnsubscribeResponse> {
        UnsubscribeOperation::execute(&self.soap_client, ip, service, request)
    }

    /// Renew an existing UPnP event subscription
    /// 
    /// This method extends the timeout of an existing subscription to prevent
    /// it from expiring. Should be called before the subscription expires.
    /// 
    /// # Arguments
    /// * `ip` - The IP address of the Sonos device
    /// * `service` - The service to renew subscription for
    /// * `request` - The renewal request parameters
    /// 
    /// # Returns
    /// The renewal response containing the actual timeout granted
    /// 
    /// # Example
    /// ```rust
    /// use sonos_api::{SonosClient, Service, operations::events::RenewRequest};
    /// 
    /// let client = SonosClient::new();
    /// let request = RenewRequest {
    ///     sid: "uuid:12345678-1234-1234-1234-123456789012".to_string(),
    ///     timeout_seconds: 1800,
    /// };
    /// 
    /// // Renew AVTransport subscription
    /// // let response = client.renew_subscription("192.168.1.100", Service::AVTransport, &request)?;
    /// ```
    pub fn renew_subscription(
        &self,
        ip: &str,
        service: Service,
        request: &RenewRequest,
    ) -> Result<RenewResponse> {
        RenewOperation::execute(&self.soap_client, ip, service, request)
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