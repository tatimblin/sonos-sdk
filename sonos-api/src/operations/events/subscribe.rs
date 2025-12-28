//! Subscribe operation for UPnP event subscriptions

use serde::{Deserialize, Serialize};
use crate::{ApiError, Result, Service};

/// Subscribe operation for UPnP event subscriptions
/// 
/// This operation handles creating new UPnP event subscriptions for any service.
/// Unlike regular SOAP operations, this uses HTTP SUBSCRIBE method instead of POST.
pub struct SubscribeOperation;

/// Request for Subscribe operation
#[derive(Debug, Clone, Serialize)]
pub struct SubscribeRequest {
    /// The callback URL where events should be sent
    pub callback_url: String,
    /// Requested subscription timeout in seconds
    pub timeout_seconds: u32,
}

/// Response for Subscribe operation
#[derive(Debug, Clone, Deserialize)]
pub struct SubscribeResponse {
    /// Subscription ID returned by the device
    pub sid: String,
    /// Actual timeout granted by the device (in seconds)
    pub timeout_seconds: u32,
}

impl SubscribeOperation {
    /// Execute a subscription request for a specific service
    /// 
    /// This method uses the soap-client's subscribe functionality to create
    /// a UPnP event subscription for the specified service.
    /// 
    /// # Arguments
    /// * `soap_client` - The SOAP client to use for the request
    /// * `ip` - Device IP address
    /// * `service` - The service to subscribe to
    /// * `request` - The subscription request parameters
    /// 
    /// # Returns
    /// The subscription response containing SID and timeout
    pub fn execute(
        soap_client: &soap_client::SoapClient,
        ip: &str,
        service: Service,
        request: &SubscribeRequest,
    ) -> Result<SubscribeResponse> {
        let service_info = service.info();
        
        let subscription_response = soap_client
            .subscribe(
                ip,
                1400, // Standard Sonos port
                service_info.event_endpoint,
                &request.callback_url,
                request.timeout_seconds,
            )
            .map_err(|e| match e {
                soap_client::SoapError::Network(msg) => ApiError::NetworkError(msg),
                soap_client::SoapError::Parse(msg) => ApiError::ParseError(msg),
                soap_client::SoapError::Fault(code) => ApiError::SoapFault(code),
            })?;
            
        Ok(SubscribeResponse {
            sid: subscription_response.sid,
            timeout_seconds: subscription_response.timeout_seconds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe_request_creation() {
        let request = SubscribeRequest {
            callback_url: "http://192.168.1.50:8080/callback".to_string(),
            timeout_seconds: 1800,
        };
        
        assert_eq!(request.callback_url, "http://192.168.1.50:8080/callback");
        assert_eq!(request.timeout_seconds, 1800);
    }

    #[test]
    fn test_subscribe_response_creation() {
        let response = SubscribeResponse {
            sid: "uuid:12345678-1234-1234-1234-123456789012".to_string(),
            timeout_seconds: 1800,
        };
        
        assert_eq!(response.sid, "uuid:12345678-1234-1234-1234-123456789012");
        assert_eq!(response.timeout_seconds, 1800);
    }
}