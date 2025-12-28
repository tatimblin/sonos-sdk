use soap_client::SoapClient;
use crate::{ApiError, Result, SonosOperation};

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