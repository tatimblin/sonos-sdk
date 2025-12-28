//! Unsubscribe operation for UPnP event subscriptions

use serde::Serialize;
use crate::{ApiError, Result, Service};

/// Unsubscribe operation for UPnP event subscriptions
/// 
/// This operation handles canceling existing UPnP event subscriptions for any service.
/// Unlike regular SOAP operations, this uses HTTP UNSUBSCRIBE method instead of POST.
pub struct UnsubscribeOperation;

/// Request for Unsubscribe operation
#[derive(Debug, Clone, Serialize)]
pub struct UnsubscribeRequest {
    /// The subscription ID to cancel
    pub sid: String,
}

/// Response for Unsubscribe operation (empty - success is indicated by no error)
#[derive(Debug, Clone)]
pub struct UnsubscribeResponse;

impl UnsubscribeOperation {
    /// Execute an unsubscribe request for a specific service
    /// 
    /// This method uses the soap-client's unsubscribe functionality to cancel
    /// an existing UPnP event subscription for the specified service.
    /// 
    /// # Arguments
    /// * `soap_client` - The SOAP client to use for the request
    /// * `ip` - Device IP address
    /// * `service` - The service to unsubscribe from
    /// * `request` - The unsubscribe request parameters
    /// 
    /// # Returns
    /// An empty response on success, or an error if the operation failed
    pub fn execute(
        soap_client: &soap_client::SoapClient,
        ip: &str,
        service: Service,
        request: &UnsubscribeRequest,
    ) -> Result<UnsubscribeResponse> {
        let service_info = service.info();
        
        soap_client
            .unsubscribe(
                ip,
                1400, // Standard Sonos port
                service_info.event_endpoint,
                &request.sid,
            )
            .map_err(|e| match e {
                soap_client::SoapError::Network(msg) => ApiError::NetworkError(msg),
                soap_client::SoapError::Parse(msg) => ApiError::ParseError(msg),
                soap_client::SoapError::Fault(code) => ApiError::SoapFault(code),
            })?;
            
        Ok(UnsubscribeResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsubscribe_request_creation() {
        let request = UnsubscribeRequest {
            sid: "uuid:12345678-1234-1234-1234-123456789012".to_string(),
        };
        
        assert_eq!(request.sid, "uuid:12345678-1234-1234-1234-123456789012");
    }

    #[test]
    fn test_unsubscribe_response_creation() {
        let _response = UnsubscribeResponse;
        // Just verify it can be created
    }
}