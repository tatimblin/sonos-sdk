//! UPnP subscription implementation.

use crate::error::SubscriptionError;
use super::Subscription;
use crate::types::{ServiceType, SpeakerId};
use std::time::Duration;
use async_trait::async_trait;
use sonos_api::{SonosClient, Service, ManagedSubscription};

/// UPnP subscription wrapper that adapts ManagedSubscription to the Subscription trait
///
/// This struct provides a bridge between sonos-api's ManagedSubscription and
/// sonos-stream's Subscription trait, handling the necessary conversions and
/// maintaining compatibility with the existing broker architecture.
#[derive(Debug)]
pub struct UPnPSubscription {
    /// The underlying managed subscription from sonos-api
    managed_subscription: ManagedSubscription,
    /// The speaker this subscription is for
    speaker_id: SpeakerId,
    /// The service type this subscription is for
    service_type: ServiceType,
}

impl UPnPSubscription {
    /// Convert ServiceType to sonos-api Service enum
    fn to_sonos_api_service(service_type: ServiceType) -> Service {
        match service_type {
            ServiceType::AVTransport => Service::AVTransport,
            ServiceType::RenderingControl => Service::RenderingControl,
            ServiceType::ZoneGroupTopology => Service::ZoneGroupTopology,
        }
    }

    /// Create a new UPnP subscription by sending a SUBSCRIBE request.
    ///
    /// This method handles the initial subscription creation by:
    /// 1. Extracting device IP from the endpoint URL
    /// 2. Creating a ManagedSubscription via SonosClient
    /// 3. Wrapping it in a UPnPSubscription for trait compatibility
    ///
    /// # Parameters
    ///
    /// - `speaker_id`: The ID of the speaker to subscribe to
    /// - `service_type`: The service type being subscribed to
    /// - `endpoint_url`: The full endpoint URL for the subscription (used to extract device IP)
    /// - `callback_url`: The callback URL for receiving events
    /// - `timeout_seconds`: Requested timeout in seconds
    ///
    /// # Returns
    ///
    /// A new UPnPSubscription instance on success.
    ///
    /// # Errors
    ///
    /// Returns `SubscriptionError::NetworkError` if the HTTP request fails.
    /// Returns `SubscriptionError::UnsubscribeFailed` if the subscription request is rejected.
    pub async fn create_subscription(
        speaker_id: SpeakerId,
        service_type: ServiceType,
        endpoint_url: String,
        callback_url: String,
        timeout_seconds: u32,
    ) -> Result<Self, SubscriptionError> {
        // Extract device IP from endpoint URL
        let (device_ip, _device_port) = Self::extract_device_info_from_url(&endpoint_url)?;
        
        // Create SonosClient
        let client = SonosClient::new();
        
        // Convert service type
        let service = Self::to_sonos_api_service(service_type);
        
        // Create managed subscription
        let managed_subscription = client
            .create_managed_subscription(&device_ip, service, &callback_url, timeout_seconds)
            .map_err(|e| match e {
                sonos_api::ApiError::NetworkError(msg) => SubscriptionError::NetworkError(msg),
                sonos_api::ApiError::DeviceUnreachable(msg) => SubscriptionError::NetworkError(msg),
                sonos_api::ApiError::SubscriptionFailed(msg) => SubscriptionError::UnsubscribeFailed(msg),
                sonos_api::ApiError::ParseError(msg) => SubscriptionError::NetworkError(format!("Parse error: {}", msg)),
                sonos_api::ApiError::SoapFault(code) => SubscriptionError::UnsubscribeFailed(format!("SOAP fault: {}", code)),
                sonos_api::ApiError::InvalidCallbackUrl(msg) => SubscriptionError::UnsubscribeFailed(format!("Invalid callback URL: {}", msg)),
                sonos_api::ApiError::UnsupportedOperation => SubscriptionError::UnsubscribeFailed("Subscription not supported by device".to_string()),
                sonos_api::ApiError::InvalidParameter(msg) => SubscriptionError::UnsubscribeFailed(format!("Invalid parameter: {}", msg)),
                _ => SubscriptionError::NetworkError(format!("Subscription failed: {}", e)),
            })?;

        Ok(Self {
            managed_subscription,
            speaker_id,
            service_type,
        })
    }

    /// Extract device IP and port from endpoint URL
    fn extract_device_info_from_url(endpoint_url: &str) -> Result<(String, u16), SubscriptionError> {
        let url = url::Url::parse(endpoint_url)
            .map_err(|e| SubscriptionError::NetworkError(format!("Invalid endpoint URL: {}", e)))?;
        
        let host = url.host_str()
            .ok_or_else(|| SubscriptionError::NetworkError("No host in endpoint URL".to_string()))?;
        
        let port = url.port().unwrap_or(1400);
        
        Ok((host.to_string(), port))
    }
}

#[async_trait]
impl Subscription for UPnPSubscription {
    fn subscription_id(&self) -> &str {
        self.managed_subscription.subscription_id()
    }

    async fn renew(&mut self) -> Result<(), SubscriptionError> {
        self.managed_subscription.renew().map_err(|e| match e {
            sonos_api::ApiError::NetworkError(msg) => SubscriptionError::NetworkError(msg),
            sonos_api::ApiError::DeviceUnreachable(msg) => SubscriptionError::NetworkError(msg),
            sonos_api::ApiError::RenewalFailed(msg) => SubscriptionError::RenewalFailed(msg),
            sonos_api::ApiError::SubscriptionExpired => SubscriptionError::Expired,
            sonos_api::ApiError::SoapFault(code) => SubscriptionError::RenewalFailed(format!("SOAP fault: {}", code)),
            sonos_api::ApiError::ParseError(msg) => SubscriptionError::RenewalFailed(format!("Parse error: {}", msg)),
            sonos_api::ApiError::UnsupportedOperation => SubscriptionError::RenewalFailed("Renewal not supported by device".to_string()),
            sonos_api::ApiError::InvalidParameter(msg) => SubscriptionError::RenewalFailed(format!("Invalid parameter: {}", msg)),
            _ => SubscriptionError::RenewalFailed(format!("Renewal failed: {}", e)),
        })
    }

    async fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        self.managed_subscription.unsubscribe().map_err(|e| match e {
            sonos_api::ApiError::NetworkError(msg) => SubscriptionError::NetworkError(msg),
            sonos_api::ApiError::DeviceUnreachable(msg) => SubscriptionError::NetworkError(msg),
            sonos_api::ApiError::SoapFault(code) => SubscriptionError::UnsubscribeFailed(format!("SOAP fault: {}", code)),
            sonos_api::ApiError::SubscriptionExpired => SubscriptionError::Expired,
            sonos_api::ApiError::ParseError(msg) => SubscriptionError::UnsubscribeFailed(format!("Parse error: {}", msg)),
            sonos_api::ApiError::UnsupportedOperation => SubscriptionError::UnsubscribeFailed("Unsubscribe not supported by device".to_string()),
            sonos_api::ApiError::InvalidParameter(msg) => SubscriptionError::UnsubscribeFailed(format!("Invalid parameter: {}", msg)),
            _ => SubscriptionError::UnsubscribeFailed(format!("Unsubscribe failed: {}", e)),
        })
    }

    fn is_active(&self) -> bool {
        self.managed_subscription.is_active()
    }

    fn time_until_renewal(&self) -> Option<Duration> {
        self.managed_subscription.time_until_renewal()
    }

    fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    fn service_type(&self) -> ServiceType {
        self.service_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_device_info_from_url() {
        let result = UPnPSubscription::extract_device_info_from_url(
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event"
        );
        
        assert!(result.is_ok());
        let (ip, port) = result.unwrap();
        assert_eq!(ip, "192.168.1.100");
        assert_eq!(port, 1400);
    }

    #[test]
    fn test_service_type_conversion() {
        assert_eq!(
            UPnPSubscription::to_sonos_api_service(ServiceType::AVTransport),
            Service::AVTransport
        );
        assert_eq!(
            UPnPSubscription::to_sonos_api_service(ServiceType::RenderingControl),
            Service::RenderingControl
        );
        assert_eq!(
            UPnPSubscription::to_sonos_api_service(ServiceType::ZoneGroupTopology),
            Service::ZoneGroupTopology
        );
    }
}