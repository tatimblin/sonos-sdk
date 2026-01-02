//! UPnP event subscription operations
//!
//! This module provides operations for managing UPnP event subscriptions
//! across all Sonos services. These operations handle the HTTP-based
//! subscription protocol rather than SOAP.

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