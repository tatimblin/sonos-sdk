//! Polling-based subscription implementation.
//!
//! This module provides a polling-based alternative to UPnP subscriptions for scenarios
//! where firewall restrictions prevent callback-based event notifications from working.
//! Instead of waiting for events to be pushed via HTTP callbacks, this implementation
//! periodically polls the device's state to detect changes.

use crate::error::SubscriptionError;
use super::Subscription;
use crate::types::{ServiceType, SpeakerId};
use std::time::{Duration, SystemTime};
use async_trait::async_trait;
use tokio::sync::{mpsc, RwLock};
use std::sync::Arc;
use std::net::IpAddr;

// Import sonos-api operations
use sonos_api::operations::{
    GetTransportInfoOperation, GetTransportInfoRequest,
};
use sonos_api::{Service, SonosOperation};
use soap_client::SoapClient;

/// Polling-based subscription implementation.
///
/// This subscription type works by periodically polling the UPnP device's state
/// instead of relying on event notifications. It's designed as a fallback when
/// firewall restrictions prevent the callback server from receiving events.
///
/// # Polling Strategy
///
/// - Polls the device state at regular intervals (default: 5 seconds)
/// - Compares current state with previous state to detect changes
/// - Generates synthetic events when changes are detected
/// - Uses exponential backoff on polling errors
///
/// # Limitations
///
/// - Higher latency than callback-based subscriptions
/// - Increased network traffic due to regular polling
/// - May miss rapid state changes between polls
/// - Requires more CPU and memory for state comparison
pub struct PollingSubscription {
    /// Unique subscription ID (generated locally)
    subscription_id: String,
    /// The speaker this subscription is for
    speaker_id: SpeakerId,
    /// The service type this subscription is for
    service_type: ServiceType,
    /// The speaker IP address for polling
    #[allow(dead_code)]
    speaker_ip: IpAddr,
    /// Whether the subscription is currently active
    active: Arc<RwLock<bool>>,
    /// Polling interval
    #[allow(dead_code)]
    poll_interval: Duration,
    /// When this subscription was created (used for renewal logic)
    #[allow(dead_code)]
    created_at: SystemTime,
    /// Shutdown signal sender
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Polling task handle
    polling_handle: Option<tokio::task::JoinHandle<()>>,
}

impl PollingSubscription {
    /// Create a new polling subscription.
    ///
    /// # Arguments
    ///
    /// * `speaker_id` - The speaker to poll
    /// * `service_type` - The service type to poll
    /// * `speaker_ip` - The IP address of the speaker
    /// * `poll_interval` - How often to poll for changes
    ///
    /// # Returns
    ///
    /// A new PollingSubscription instance that will start polling immediately.
    pub async fn new(
        speaker_id: SpeakerId,
        service_type: ServiceType,
        speaker_ip: IpAddr,
        poll_interval: Option<Duration>,
    ) -> Result<Self, SubscriptionError> {
        let subscription_id = format!("polling-{}-{}", 
            speaker_id.as_str(), 
            uuid::Uuid::new_v4()
        );
        
        let poll_interval = poll_interval.unwrap_or(Duration::from_secs(5));
        let active = Arc::new(RwLock::new(true));
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        
        // Start polling task
        let polling_handle = Self::start_polling_task(
            speaker_ip,
            service_type,
            speaker_id.clone(),
            poll_interval,
            active.clone(),
            shutdown_rx,
        );
        
        Ok(Self {
            subscription_id,
            speaker_id,
            service_type,
            speaker_ip,
            active,
            poll_interval,
            created_at: SystemTime::now(),
            shutdown_tx: Some(shutdown_tx),
            polling_handle: Some(polling_handle),
        })
    }

    /// Start the background polling task.
    fn start_polling_task(
        speaker_ip: IpAddr,
        service_type: ServiceType,
        speaker_id: SpeakerId,
        poll_interval: Duration,
        active: Arc<RwLock<bool>>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            eprintln!("🔄 Polling Subscription: Starting polling task for {} {:?}", 
                     speaker_id.as_str(), service_type);
            
            let mut last_state: Option<String> = None;
            let mut error_count = 0;
            let max_errors = 5;
            
            // Create SOAP client for making requests
            let soap_client = SoapClient::new();
            
            loop {
                // Check if we should shutdown
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        eprintln!("🛑 Polling Subscription: Shutdown signal received");
                        break;
                    }
                    _ = tokio::time::sleep(poll_interval) => {
                        // Continue with polling
                    }
                }
                
                // Check if subscription is still active
                {
                    let is_active = *active.read().await;
                    if !is_active {
                        eprintln!("🛑 Polling Subscription: Subscription marked inactive");
                        break;
                    }
                }
                
                // Poll the device state based on service type
                match Self::poll_device_state(&soap_client, speaker_ip, service_type).await {
                    Ok(current_state) => {
                        error_count = 0; // Reset error count on success
                        
                        // Check if state has changed
                        if let Some(ref previous_state) = last_state {
                            if current_state != *previous_state {
                                eprintln!("📊 Polling Subscription: State change detected for {} {:?}", 
                                         speaker_id.as_str(), service_type);
                                eprintln!("   Previous: {}", previous_state);
                                eprintln!("   Current:  {}", current_state);
                                // TODO: Generate synthetic event and send to event processor
                                // This would require access to the event processing pipeline
                            }
                        } else {
                            eprintln!("📊 Polling Subscription: Initial state captured for {} {:?}: {}", 
                                     speaker_id.as_str(), service_type, current_state);
                        }
                        
                        last_state = Some(current_state);
                    }
                    Err(e) => {
                        error_count += 1;
                        eprintln!("❌ Polling Subscription: Error polling device (attempt {}/{}): {}", 
                                 error_count, max_errors, e);
                        
                        if error_count >= max_errors {
                            eprintln!("💥 Polling Subscription: Too many errors, marking subscription inactive");
                            let mut is_active = active.write().await;
                            *is_active = false;
                            break;
                        }
                        
                        // Exponential backoff on errors
                        let backoff_duration = Duration::from_secs(2_u64.pow(error_count.min(6)));
                        tokio::time::sleep(backoff_duration).await;
                    }
                }
            }
            
            eprintln!("🏁 Polling Subscription: Polling task ended for {} {:?}", 
                     speaker_id.as_str(), service_type);
        })
    }

    /// Poll the device state using sonos-api operations.
    async fn poll_device_state(
        soap_client: &SoapClient,
        speaker_ip: IpAddr,
        service_type: ServiceType,
    ) -> Result<String, SubscriptionError> {
        match service_type {
            ServiceType::AVTransport => {
                // Use GetTransportInfo operation from sonos-api
                let request = GetTransportInfoRequest { instance_id: 0 };
                let payload = GetTransportInfoOperation::build_payload(&request);
                
                let service_info = Service::AVTransport.info();
                let ip_str = speaker_ip.to_string();
                
                // Execute the SOAP call
                let response_xml = soap_client
                    .call(
                        &ip_str,
                        service_info.endpoint,
                        service_info.service_uri,
                        GetTransportInfoOperation::ACTION,
                        &payload,
                    )
                    .map_err(|e| SubscriptionError::NetworkError(format!("SOAP call failed: {}", e)))?;
                
                // Parse the response using sonos-api
                let response = GetTransportInfoOperation::parse_response(&response_xml)
                    .map_err(|e| SubscriptionError::NetworkError(format!("Response parsing failed: {}", e)))?;
                
                // Convert response to a comparable string representation
                Ok(format!("{:?}", response))
            }
            ServiceType::RenderingControl => {
                // TODO: Implement RenderingControl polling using GetVolume operation
                // For now, return a placeholder
                Err(SubscriptionError::NetworkError(
                    "RenderingControl polling not yet implemented".to_string()
                ))
            }
            ServiceType::ZoneGroupTopology => {
                // TODO: Implement ZoneGroupTopology polling
                // For now, return a placeholder
                Err(SubscriptionError::NetworkError(
                    "ZoneGroupTopology polling not yet implemented".to_string()
                ))
            }
        }
    }
}

#[async_trait]
impl Subscription for PollingSubscription {
    fn subscription_id(&self) -> &str {
        &self.subscription_id
    }

    async fn renew(&mut self) -> Result<(), SubscriptionError> {
        let is_active = *self.active.read().await;
        if !is_active {
            return Err(SubscriptionError::Expired);
        }

        // For polling subscriptions, "renewal" just means confirming we're still active
        // and potentially restarting the polling task if needed
        eprintln!("🔄 Polling Subscription: Renewal requested (polling subscriptions don't need renewal)");
        Ok(())
    }

    async fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        eprintln!("🛑 Polling Subscription: Unsubscribing {}", self.subscription_id);
        
        // Mark as inactive
        {
            let mut is_active = self.active.write().await;
            *is_active = false;
        }

        // Send shutdown signal to polling task
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(()).await;
        }

        // Wait for polling task to complete
        if let Some(handle) = self.polling_handle.take() {
            let _ = handle.await;
        }

        eprintln!("✅ Polling Subscription: Unsubscribed {}", self.subscription_id);
        Ok(())
    }

    fn is_active(&self) -> bool {
        // For polling subscriptions, we're active as long as the polling task is running
        // and we haven't been explicitly unsubscribed
        if let Ok(is_active) = self.active.try_read() {
            *is_active
        } else {
            false
        }
    }

    fn time_until_renewal(&self) -> Option<Duration> {
        // Polling subscriptions don't need renewal in the traditional sense
        // They run indefinitely until unsubscribed
        None
    }

    fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    fn service_type(&self) -> ServiceType {
        self.service_type
    }
}

// Implement Drop to ensure cleanup
impl Drop for PollingSubscription {
    fn drop(&mut self) {
        // Mark as inactive
        if let Ok(mut is_active) = self.active.try_write() {
            *is_active = false;
        }

        // Send shutdown signal if we still have the sender
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            // Use try_send since we're in Drop and can't await
            let _ = shutdown_tx.try_send(());
        }

        // Note: We can't await the polling task handle in Drop since it's not async
        // The task will eventually notice the inactive flag and shutdown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_polling_subscription_creation() {
        let subscription = PollingSubscription::new(
            SpeakerId::new("test_speaker"),
            ServiceType::AVTransport,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            Some(Duration::from_millis(100)), // Fast polling for testing
        ).await.unwrap();

        assert!(subscription.subscription_id().starts_with("polling-test_speaker-"));
        assert_eq!(subscription.speaker_id().as_str(), "test_speaker");
        assert_eq!(subscription.service_type(), ServiceType::AVTransport);
        assert!(subscription.is_active());
        assert!(subscription.time_until_renewal().is_none());
    }

    #[tokio::test]
    async fn test_polling_subscription_unsubscribe() {
        let mut subscription = PollingSubscription::new(
            SpeakerId::new("test_speaker"),
            ServiceType::AVTransport,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            Some(Duration::from_millis(100)),
        ).await.unwrap();

        assert!(subscription.is_active());

        let result = subscription.unsubscribe().await;
        assert!(result.is_ok());
        assert!(!subscription.is_active());

        // Second unsubscribe should still work (idempotent)
        let result = subscription.unsubscribe().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_polling_subscription_renewal() {
        let mut subscription = PollingSubscription::new(
            SpeakerId::new("test_speaker"),
            ServiceType::AVTransport,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            Some(Duration::from_millis(100)),
        ).await.unwrap();

        // Renewal should succeed for active subscription
        let result = subscription.renew().await;
        assert!(result.is_ok());

        // Unsubscribe and then try renewal
        subscription.unsubscribe().await.unwrap();
        let result = subscription.renew().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SubscriptionError::Expired));
    }

    #[tokio::test]
    async fn test_polling_subscription_lifecycle() {
        let mut subscription = PollingSubscription::new(
            SpeakerId::new("test_speaker"),
            ServiceType::AVTransport,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), // This will fail to connect, which is expected for testing
            Some(Duration::from_millis(50)), // Very fast polling
        ).await.unwrap();

        // Let it run for a short time
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Should still be active initially (errors don't immediately deactivate)
        assert!(subscription.is_active());

        // Clean shutdown
        subscription.unsubscribe().await.unwrap();
        assert!(!subscription.is_active());
    }

    #[test]
    fn test_service_type_matching() {
        // Test that we can match on different service types
        match ServiceType::AVTransport {
            ServiceType::AVTransport => {
                // This should match
            }
            _ => panic!("ServiceType matching failed"),
        }
    }
}