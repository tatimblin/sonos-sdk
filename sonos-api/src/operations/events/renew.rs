//! Renew operation for UPnP event subscriptions

use serde::{Deserialize, Serialize};
use crate::{ApiError, Result, Service};

/// Renew operation for UPnP event subscriptions
/// 
/// This operation handles renewing existing UPnP event subscriptions for any service.
/// Unlike regular SOAP operations, this uses HTTP SUBSCRIBE method with SID header.
pub struct RenewOperation;

/// Request for Renew operation
#[derive(Debug, Clone, Serialize)]
pub struct RenewRequest {
    /// The subscription ID to renew
    pub sid: String,
    /// Requested renewal timeout in seconds
    pub timeout_seconds: u32,
}

/// Response for Renew operation
#[derive(Debug, Clone, Deserialize)]
pub struct RenewResponse {
    /// The actual timeout granted by the device (in seconds)
    pub timeout_seconds: u32,
}

impl RenewOperation {
    /// Execute a subscription renewal request for a specific service
    /// 
    /// This method uses the soap-client's renew_subscription functionality to
    /// extend an existing UPnP event subscription for the specified service.
    /// 
    /// # Arguments
    /// * `soap_client` - The SOAP client to use for the request
    /// * `ip` - Device IP address
    /// * `service` - The service to renew subscription for
    /// * `request` - The renewal request parameters
    /// 
    /// # Returns
    /// The renewal response containing the actual timeout granted
    pub fn execute(
        soap_client: &soap_client::SoapClient,
        ip: &str,
        service: Service,
        request: &RenewRequest,
    ) -> Result<RenewResponse> {
        let service_info = service.info();
        
        let actual_timeout_seconds = soap_client
            .renew_subscription(
                ip,
                1400, // Standard Sonos port
                service_info.event_endpoint,
                &request.sid,
                request.timeout_seconds,
            )
            .map_err(|e| match e {
                soap_client::SoapError::Network(msg) => ApiError::NetworkError(msg),
                soap_client::SoapError::Parse(msg) => ApiError::ParseError(msg),
                soap_client::SoapError::Fault(code) => ApiError::SoapFault(code),
            })?;
            
        Ok(RenewResponse {
            timeout_seconds: actual_timeout_seconds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renew_request_creation() {
        let request = RenewRequest {
            sid: "uuid:12345678-1234-1234-1234-123456789012".to_string(),
            timeout_seconds: 1800,
        };
        
        assert_eq!(request.sid, "uuid:12345678-1234-1234-1234-123456789012");
        assert_eq!(request.timeout_seconds, 1800);
    }

    #[test]
    fn test_renew_response_creation() {
        let response = RenewResponse {
            timeout_seconds: 1800,
        };
        
        assert_eq!(response.timeout_seconds, 1800);
    }
}