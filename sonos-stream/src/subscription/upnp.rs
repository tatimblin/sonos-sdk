//! UPnP subscription implementation.

use crate::error::SubscriptionError;
use super::Subscription;
use crate::types::{ServiceType, SpeakerId};
use std::time::{Duration, SystemTime};

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
    pub fn create_subscription(
        speaker_id: SpeakerId,
        service_type: ServiceType,
        endpoint_url: String,
        callback_url: String,
        timeout_seconds: u32,
    ) -> Result<Self, SubscriptionError> {
        // Create HTTP client with timeout
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| SubscriptionError::NetworkError(format!("Failed to create HTTP client: {}", e)))?;

        // Extract host from endpoint URL for HOST header
        let host = Self::extract_host_from_url_static(&endpoint_url)
            .unwrap_or_else(|| "localhost:1400".to_string());

        // Send SUBSCRIBE request
        let response = client
            .request(
                reqwest::Method::from_bytes(b"SUBSCRIBE").unwrap(),
                &endpoint_url,
            )
            .header("HOST", host)
            .header("CALLBACK", format!("<{}>", callback_url))
            .header("NT", "upnp:event")
            .header("TIMEOUT", format!("Second-{}", timeout_seconds))
            .send()
            .map_err(|e| SubscriptionError::NetworkError(format!("SUBSCRIBE request failed: {}", e)))?;

        // Check response status
        if !response.status().is_success() {
            return Err(SubscriptionError::UnsubscribeFailed(format!(
                "SUBSCRIBE failed: HTTP {} - {}",
                response.status(),
                response.text().unwrap_or_default()
            )));
        }

        // Extract SID from response headers
        let sid = response
            .headers()
            .get("SID")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| SubscriptionError::UnsubscribeFailed(
                "Missing SID header in SUBSCRIBE response".to_string()
            ))?
            .to_string();

        // Extract timeout from response headers (optional, fallback to requested timeout)
        let actual_timeout_seconds = response
            .headers()
            .get("TIMEOUT")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                // Parse "Second-1800" format
                if s.starts_with("Second-") {
                    s.strip_prefix("Second-")?.parse::<u32>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(timeout_seconds);

        // Create UPnPSubscription instance
        Ok(Self::new(
            sid,
            speaker_id,
            service_type,
            endpoint_url,
            actual_timeout_seconds,
        ))
    }

    /// Static version of extract_host_from_url for use during subscription creation.
    fn extract_host_from_url_static(endpoint_url: &str) -> Option<String> {
        let url = url::Url::parse(endpoint_url).ok()?;
        let host = url.host_str()?;
        
        if let Some(port) = url.port() {
            Some(format!("{}:{}", host, port))
        } else {
            Some(host.to_string())
        }
    }

    /// Send a UPnP UNSUBSCRIBE request.
    fn send_unsubscribe_request(&self) -> Result<(), SubscriptionError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| SubscriptionError::NetworkError(e.to_string()))?;

        let response = client
            .request(
                reqwest::Method::from_bytes(b"UNSUBSCRIBE").unwrap(),
                &self.endpoint_url,
            )
            .header("HOST", self.extract_host_from_url())
            .header("SID", &self.sid)
            .send()
            .map_err(|e| SubscriptionError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SubscriptionError::UnsubscribeFailed(format!(
                "UNSUBSCRIBE failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Send a subscription renewal request.
    fn send_renewal_request(&mut self) -> Result<(), SubscriptionError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| SubscriptionError::NetworkError(e.to_string()))?;

        let response = client
            .request(
                reqwest::Method::from_bytes(b"SUBSCRIBE").unwrap(),
                &self.endpoint_url,
            )
            .header("HOST", self.extract_host_from_url())
            .header("SID", &self.sid)
            .header("TIMEOUT", format!("Second-{}", self.timeout_seconds))
            .send()
            .map_err(|e| SubscriptionError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SubscriptionError::RenewalFailed(format!(
                "Renewal failed: HTTP {}",
                response.status()
            )));
        }

        // Update expiration time
        self.expires_at = SystemTime::now() + Duration::from_secs(self.timeout_seconds as u64);

        Ok(())
    }

    /// Extract host header from endpoint URL.
    fn extract_host_from_url(&self) -> String {
        // Extract host from URL like "http://192.168.1.100:1400/path"
        if let Ok(url) = url::Url::parse(&self.endpoint_url) {
            if let Some(host) = url.host_str() {
                if let Some(port) = url.port() {
                    return format!("{}:{}", host, port);
                } else {
                    return host.to_string();
                }
            }
        }
        
        // Fallback - assume standard Sonos port
        "localhost:1400".to_string()
    }
}

impl Subscription for UPnPSubscription {
    fn subscription_id(&self) -> &str {
        &self.sid
    }

    fn renew(&mut self) -> Result<(), SubscriptionError> {
        if !self.active {
            return Err(SubscriptionError::Expired);
        }

        self.send_renewal_request()
    }

    fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        if !self.active {
            return Err(SubscriptionError::UnsubscribeFailed(
                "Already unsubscribed".to_string(),
            ));
        }

        let result = self.send_unsubscribe_request();
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

    #[test]
    fn test_upnp_subscription_unsubscribe() {
        let mut subscription = UPnPSubscription::new(
            "uuid:12345".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event".to_string(),
            1800,
        );

        // Mark as inactive to avoid actual HTTP request
        subscription.active = false;
        
        let result = subscription.unsubscribe();
        assert!(result.is_err());
        assert!(!subscription.is_active());
    }

    #[test]
    fn test_upnp_subscription_renewal_when_inactive() {
        let mut subscription = UPnPSubscription::new(
            "uuid:12345".to_string(),
            SpeakerId::new("speaker1"),
            ServiceType::AVTransport,
            "http://192.168.1.100:1400/MediaRenderer/AVTransport/Event".to_string(),
            1800,
        );

        subscription.active = false;
        let result = subscription.renew();
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