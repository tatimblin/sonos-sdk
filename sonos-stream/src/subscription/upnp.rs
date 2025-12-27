//! UPnP subscription implementation.

use crate::error::SubscriptionError;
use super::Subscription;
use crate::types::{ServiceType, SpeakerId};
use std::time::{Duration, SystemTime};
use async_trait::async_trait;
use sonos_api::{UpnpSubscriptionClient, Service};

/// Default UPnP subscription implementation.
///
/// This struct provides a concrete implementation of the `Subscription` trait
/// that handles the standard UPnP subscription lifecycle operations.
#[derive(Debug)]
pub struct UPnPSubscription {
    /// UPnP subscription ID (SID) returned by the device
    sid: String,
    /// The speaker this subscription is for
    speaker_id: SpeakerId,
    /// The service type this subscription is for
    service_type: ServiceType,
    /// The endpoint URL for this subscription
    endpoint_url: String,
    /// When this subscription expires
    expires_at: SystemTime,
    /// Whether the subscription is currently active
    active: bool,
    /// Timeout duration for this subscription
    timeout_seconds: u32,
}

impl UPnPSubscription {
    /// Convert ServiceType to sonos-api Service enum
    fn service_type_to_service(service_type: ServiceType) -> Service {
        match service_type {
            ServiceType::AVTransport => Service::AVTransport,
            ServiceType::RenderingControl => Service::RenderingControl,
            ServiceType::ZoneGroupTopology => Service::ZoneGroupTopology,
        }
    }

    /// Extract device IP and port from speaker ID
    fn extract_device_info(speaker_id: &SpeakerId) -> (String, u16) {
        // SpeakerId format is typically "RINCON_<MAC>01400" where the device IP needs to be resolved
        // For now, we'll extract from the endpoint URL in the create_subscription method
        // This is a placeholder - in practice, you'd need device discovery info
        ("192.168.1.100".to_string(), 1400)
    }
    /// Create a new UPnP subscription.
    pub fn new(
        sid: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
        endpoint_url: String,
        timeout_seconds: u32,
    ) -> Self {
        let expires_at = SystemTime::now() + Duration::from_secs(timeout_seconds as u64);
        
        Self {
            sid,
            speaker_id,
            service_type,
            endpoint_url,
            expires_at,
            active: true,
            timeout_seconds,
        }
    }

    /// Create a new UPnP subscription by sending a SUBSCRIBE request.
    ///
    /// This method handles the initial subscription creation by:
    /// 1. Sending a SUBSCRIBE request to the endpoint
    /// 2. Parsing the response to extract SID and timeout
    /// 3. Creating and returning a UPnPSubscription instance
    ///
    /// # Parameters
    ///
    /// - `speaker_id`: The ID of the speaker to subscribe to
    /// - `service_type`: The service type being subscribed to
    /// - `endpoint_url`: The full endpoint URL for the subscription
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
        // Extract device IP and port from endpoint URL
        let (device_ip, device_port) = Self::extract_device_info_from_url(&endpoint_url)?;
        
        // Create UPnP client
        let upnp_client = UpnpSubscriptionClient::new()
            .map_err(|e| SubscriptionError::NetworkError(format!("Failed to create UPnP client: {}", e)))?;
        
        // Convert service type
        let service = Self::service_type_to_service(service_type);
        
        // Create subscription
        let subscription_response = upnp_client
            .subscribe(&device_ip, device_port, service, &callback_url, timeout_seconds)
            .await
            .map_err(|e| match e {
                sonos_api::SubscriptionError::NetworkError(msg) => SubscriptionError::NetworkError(msg),
                sonos_api::SubscriptionError::DeviceUnreachable(msg) => SubscriptionError::NetworkError(msg),
                sonos_api::SubscriptionError::SubscriptionFailed(msg) => SubscriptionError::UnsubscribeFailed(msg),
                sonos_api::SubscriptionError::TimeoutError => SubscriptionError::NetworkError("Request timeout".to_string()),
                _ => SubscriptionError::NetworkError(format!("Subscription failed: {}", e)),
            })?;

        // Create UPnPSubscription instance
        Ok(Self::new(
            subscription_response.sid,
            speaker_id,
            service_type,
            endpoint_url,
            subscription_response.timeout_seconds,
        ))
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

    /// Send a UPnP UNSUBSCRIBE request using the shared UPnP client.
    async fn send_unsubscribe_request(&self) -> Result<(), SubscriptionError> {
        // Extract device IP and port from endpoint URL
        let (device_ip, device_port) = Self::extract_device_info_from_url(&self.endpoint_url)?;
        
        // Create UPnP client
        let upnp_client = UpnpSubscriptionClient::new()
            .map_err(|e| SubscriptionError::NetworkError(format!("Failed to create UPnP client: {}", e)))?;
        
        // Convert service type
        let service = Self::service_type_to_service(self.service_type);
        
        // Unsubscribe
        upnp_client
            .unsubscribe(&device_ip, device_port, service, &self.sid)
            .await
            .map_err(|e| match e {
                sonos_api::SubscriptionError::UnsubscribeFailed(msg) => SubscriptionError::UnsubscribeFailed(msg),
                sonos_api::SubscriptionError::NetworkError(msg) => SubscriptionError::NetworkError(msg),
                sonos_api::SubscriptionError::DeviceUnreachable(msg) => SubscriptionError::NetworkError(msg),
                _ => SubscriptionError::NetworkError(format!("Unsubscribe failed: {}", e)),
            })?;

        Ok(())
    }

    /// Send a subscription renewal request using the shared UPnP client.
    async fn send_renewal_request(&mut self) -> Result<(), SubscriptionError> {
        // Extract device IP and port from endpoint URL
        let (device_ip, device_port) = Self::extract_device_info_from_url(&self.endpoint_url)?;
        
        // Create UPnP client
        let upnp_client = UpnpSubscriptionClient::new()
            .map_err(|e| SubscriptionError::NetworkError(format!("Failed to create UPnP client: {}", e)))?;
        
        // Convert service type
        let service = Self::service_type_to_service(self.service_type);
        
        // Renew subscription
        let actual_timeout_seconds = upnp_client
            .renew_subscription(&device_ip, device_port, service, &self.sid, self.timeout_seconds)
            .await
            .map_err(|e| match e {
                sonos_api::SubscriptionError::SubscriptionFailed(msg) => SubscriptionError::RenewalFailed(msg),
                sonos_api::SubscriptionError::NetworkError(msg) => SubscriptionError::NetworkError(msg),
                sonos_api::SubscriptionError::DeviceUnreachable(msg) => SubscriptionError::NetworkError(msg),
                _ => SubscriptionError::NetworkError(format!("Renewal failed: {}", e)),
            })?;

        // Update timeout and expiration time
        self.timeout_seconds = actual_timeout_seconds;
        self.expires_at = SystemTime::now() + Duration::from_secs(actual_timeout_seconds as u64);

        Ok(())
    }
}

#[async_trait]
impl Subscription for UPnPSubscription {
    fn subscription_id(&self) -> &str {
        &self.sid
    }

    async fn renew(&mut self) -> Result<(), SubscriptionError> {
        if !self.active {
            return Err(SubscriptionError::Expired);
        }

        self.send_renewal_request().await
    }

    async fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        if !self.active {
            return Err(SubscriptionError::UnsubscribeFailed(
                "Already unsubscribed".to_string(),
            ));
        }

        let result = self.send_unsubscribe_request().await;
        self.active = false;
        result
    }

    fn is_active(&self) -> bool {
        self.active && SystemTime::now() < self.expires_at
    }

    fn time_until_renewal(&self) -> Option<Duration> {
        if !self.active {
            return None;
        }

        let now = SystemTime::now();
        if now >= self.expires_at {
            return Some(Duration::ZERO);
        }

        let time_until_expiry = self.expires_at.duration_since(now).ok()?;
        let renewal_threshold = Duration::from_secs(300); // 5 minutes

        if time_until_expiry <= renewal_threshold {
            Some(time_until_expiry)
        } else {
            None
        }
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
    fn test_upnp_subscription_creation() {
        let subscription = UPnPSubscription::new(
            "uuid:12345".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event".to_string(),
            1800,
        );

        assert_eq!(subscription.subscription_id(), "uuid:12345");
        assert_eq!(subscription.speaker_id().as_str(), "speaker1");
        assert_eq!(subscription.service_type(), ServiceType::AVTransport);
        assert!(subscription.is_active());
    }

    #[tokio::test]
    async fn test_upnp_subscription_unsubscribe() {
        let mut subscription = UPnPSubscription::new(
            "uuid:12345".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event".to_string(),
            1800,
        );

        // Mark as inactive to avoid actual HTTP request
        subscription.active = false;
        
        let result = subscription.unsubscribe().await;
        assert!(result.is_err());
        assert!(!subscription.is_active());
    }

    #[tokio::test]
    async fn test_upnp_subscription_renewal_when_inactive() {
        let mut subscription = UPnPSubscription::new(
            "uuid:12345".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event".to_string(),
            1800,
        );

        subscription.active = false;
        let result = subscription.renew().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SubscriptionError::Expired));
    }

    #[test]
    fn test_upnp_subscription_time_until_renewal() {
        let subscription = UPnPSubscription::new(
            "uuid:12345".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event".to_string(),
            200, // Short timeout to trigger renewal threshold
        );

        // Should need renewal since timeout is less than 5 minutes
        let time_until = subscription.time_until_renewal();
        assert!(time_until.is_some());
    }

    #[test]
    fn test_extract_host_from_url() {
        let subscription = UPnPSubscription::new(
            "uuid:12345".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event".to_string(),
            1800,
        );

        let host = subscription.extract_host_from_url();
        assert_eq!(host, "192.168.1.100:1400");
    }
}