//! Subscription lifecycle management with SonosClient integration
//!
//! This module provides subscription management by integrating with SonosClient's
//! ManagedSubscription system and coordinating with the callback server for event routing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{Mutex, RwLock};

use callback_server::firewall_detection::FirewallStatus;
use sonos_api::{ManagedSubscription, Service, SonosClient};

use crate::error::{SubscriptionError, SubscriptionResult};
use crate::registry::{RegistrationId, SpeakerServicePair};

/// Wrapper around ManagedSubscription with additional context for event streaming
#[derive(Debug)]
pub struct ManagedSubscriptionWrapper {
    /// The actual SonosClient subscription
    subscription: ManagedSubscription,

    /// Registration ID this subscription belongs to
    registration_id: RegistrationId,

    /// Speaker/service pair for this subscription
    speaker_service_pair: SpeakerServicePair,

    /// Timestamp of the last event received for this subscription
    last_event_time: Arc<Mutex<Option<SystemTime>>>,

    /// Whether polling is currently active for this subscription
    is_polling_active: Arc<AtomicBool>,

    /// Creation timestamp
    created_at: SystemTime,

    /// Number of renewal attempts
    renewal_count: Arc<Mutex<u32>>,
}

impl ManagedSubscriptionWrapper {
    /// Create a new wrapper around a ManagedSubscription
    pub fn new(
        subscription: ManagedSubscription,
        registration_id: RegistrationId,
        speaker_service_pair: SpeakerServicePair,
    ) -> Self {
        Self {
            subscription,
            registration_id,
            speaker_service_pair,
            last_event_time: Arc::new(Mutex::new(None)),
            is_polling_active: Arc::new(AtomicBool::new(false)),
            created_at: SystemTime::now(),
            renewal_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the registration ID
    pub fn registration_id(&self) -> RegistrationId {
        self.registration_id
    }

    /// Get the speaker/service pair
    pub fn speaker_service_pair(&self) -> &SpeakerServicePair {
        &self.speaker_service_pair
    }

    /// Get the UPnP subscription ID
    pub fn subscription_id(&self) -> &str {
        self.subscription.subscription_id()
    }

    /// Check if the subscription is active
    pub fn is_active(&self) -> bool {
        self.subscription.is_active()
    }

    /// Check if the subscription needs renewal
    pub fn needs_renewal(&self) -> bool {
        self.subscription.needs_renewal()
    }

    /// Renew the subscription
    pub async fn renew(&self) -> SubscriptionResult<()> {
        self.subscription
            .renew()
            .map_err(|e| SubscriptionError::RenewalFailed(e.to_string()))?;

        // Increment renewal count
        let mut count = self.renewal_count.lock().await;
        *count += 1;

        Ok(())
    }

    /// Unsubscribe and clean up
    pub async fn unsubscribe(&self) -> SubscriptionResult<()> {
        self.subscription
            .unsubscribe()
            .map_err(|e| SubscriptionError::NetworkError(e.to_string()))?;
        Ok(())
    }

    /// Record that an event was received for this subscription
    pub async fn record_event_received(&self) {
        let mut last_event_time = self.last_event_time.lock().await;
        *last_event_time = Some(SystemTime::now());
    }

    /// Get the time of the last event received
    pub async fn last_event_time(&self) -> Option<SystemTime> {
        let last_event_time = self.last_event_time.lock().await;
        *last_event_time
    }

    /// Set whether polling is active for this subscription
    pub fn set_polling_active(&self, active: bool) {
        self.is_polling_active.store(active, Ordering::Relaxed);
    }

    /// Check if polling is active for this subscription
    pub fn is_polling_active(&self) -> bool {
        self.is_polling_active.load(Ordering::Relaxed)
    }

    /// Get creation timestamp
    pub fn created_at(&self) -> SystemTime {
        self.created_at
    }

    /// Get renewal count
    pub async fn renewal_count(&self) -> u32 {
        let count = self.renewal_count.lock().await;
        *count
    }
}

/// Manages subscriptions for registered speaker/service pairs
pub struct SubscriptionManager {
    /// SonosClient for creating and managing subscriptions
    sonos_client: SonosClient,

    /// Callback URL for UPnP event notifications
    callback_url: String,


    /// Active subscriptions indexed by registration ID
    active_subscriptions: Arc<RwLock<HashMap<RegistrationId, Arc<ManagedSubscriptionWrapper>>>>,

    /// Current firewall status (shared with other components)
    firewall_status: Arc<RwLock<FirewallStatus>>,
}

impl SubscriptionManager {
    /// Create a new SubscriptionManager
    pub fn new(callback_url: String) -> Self {
        Self {
            sonos_client: SonosClient::new(),
            callback_url,
            active_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            firewall_status: Arc::new(RwLock::new(FirewallStatus::Unknown)),
        }
    }

    /// Set the firewall status (called by firewall detection system)
    pub async fn set_firewall_status(&self, status: FirewallStatus) {
        let mut current_status = self.firewall_status.write().await;
        *current_status = status;
    }

    /// Get the current firewall status
    pub async fn firewall_status(&self) -> FirewallStatus {
        let status = self.firewall_status.read().await;
        *status
    }

    /// Create a subscription for a speaker/service pair
    pub async fn create_subscription(
        &self,
        registration_id: RegistrationId,
        pair: SpeakerServicePair,
    ) -> SubscriptionResult<Arc<ManagedSubscriptionWrapper>> {
        // Convert Service to the format expected by SonosClient (no conversion needed since we're using the same enum)
        let service = pair.service;

        // Create the subscription using SonosClient
        let subscription = self
            .sonos_client
            .subscribe(
                &pair.speaker_ip.to_string(),
                service,
                &self.callback_url,
            )
            .map_err(|e| SubscriptionError::CreationFailed(e.to_string()))?;

        // Wrap it with our additional context
        let wrapper = Arc::new(ManagedSubscriptionWrapper::new(
            subscription,
            registration_id,
            pair,
        ));

        // Store in our active subscriptions
        let mut subscriptions = self.active_subscriptions.write().await;
        subscriptions.insert(registration_id, Arc::clone(&wrapper));

        Ok(wrapper)
    }

    /// Remove a subscription
    pub async fn remove_subscription(
        &self,
        registration_id: RegistrationId,
    ) -> SubscriptionResult<()> {
        let mut subscriptions = self.active_subscriptions.write().await;

        if let Some(wrapper) = subscriptions.remove(&registration_id) {
            // Unsubscribe from the UPnP service
            wrapper.unsubscribe().await?;
        } else {
            return Err(SubscriptionError::InvalidState);
        }

        Ok(())
    }

    /// Get a subscription by registration ID
    pub async fn get_subscription(
        &self,
        registration_id: RegistrationId,
    ) -> Option<Arc<ManagedSubscriptionWrapper>> {
        let subscriptions = self.active_subscriptions.read().await;
        subscriptions.get(&registration_id).cloned()
    }

    /// Get subscription by UPnP subscription ID (for event routing)
    pub async fn get_subscription_by_sid(
        &self,
        subscription_id: &str,
    ) -> Option<Arc<ManagedSubscriptionWrapper>> {
        let subscriptions = self.active_subscriptions.read().await;
        subscriptions
            .values()
            .find(|wrapper| wrapper.subscription_id() == subscription_id)
            .cloned()
    }

    /// List all active subscriptions
    pub async fn list_subscriptions(&self) -> Vec<Arc<ManagedSubscriptionWrapper>> {
        let subscriptions = self.active_subscriptions.read().await;
        subscriptions.values().cloned().collect()
    }

    /// Check for subscriptions that need renewal and renew them
    pub async fn check_renewals(&self) -> SubscriptionResult<usize> {
        let subscriptions = self.active_subscriptions.read().await;
        let mut renewed_count = 0;

        for wrapper in subscriptions.values() {
            if wrapper.needs_renewal() {
                match wrapper.renew().await {
                    Ok(()) => {
                        renewed_count += 1;
                        eprintln!(
                            "✅ Renewed subscription for {} {:?}",
                            wrapper.speaker_service_pair.speaker_ip,
                            wrapper.speaker_service_pair.service
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "❌ Failed to renew subscription for {} {:?}: {}",
                            wrapper.speaker_service_pair.speaker_ip,
                            wrapper.speaker_service_pair.service,
                            e
                        );
                        // Note: We continue processing other subscriptions even if one fails
                    }
                }
            }
        }

        Ok(renewed_count)
    }

    /// Record that an event was received for a subscription
    pub async fn record_event_received(&self, subscription_id: &str) {
        if let Some(wrapper) = self.get_subscription_by_sid(subscription_id).await {
            wrapper.record_event_received().await;
        }
    }

    /// Get statistics about managed subscriptions
    pub async fn stats(&self) -> SubscriptionStats {
        let subscriptions = self.active_subscriptions.read().await;
        let total_count = subscriptions.len();
        let firewall_status = *self.firewall_status.read().await;

        let mut service_counts = HashMap::new();
        let mut polling_count = 0;
        let mut renewal_count = 0;

        for wrapper in subscriptions.values() {
            *service_counts
                .entry(wrapper.speaker_service_pair.service)
                .or_insert(0) += 1;

            if wrapper.is_polling_active() {
                polling_count += 1;
            }

            renewal_count += wrapper.renewal_count().await;
        }

        SubscriptionStats {
            total_subscriptions: total_count,
            service_breakdown: service_counts,
            polling_active_count: polling_count,
            total_renewals: renewal_count,
            firewall_status,
        }
    }

    /// Shutdown all subscriptions
    pub async fn shutdown(&self) -> SubscriptionResult<()> {
        let mut subscriptions = self.active_subscriptions.write().await;

        for (registration_id, wrapper) in subscriptions.drain() {
            match wrapper.unsubscribe().await {
                Ok(()) => {
                    eprintln!("✅ Unsubscribed {}", registration_id);
                }
                Err(e) => {
                    eprintln!("❌ Failed to unsubscribe {}: {}", registration_id, e);
                }
            }
        }

        Ok(())
    }
}

/// Statistics about subscription manager state
#[derive(Debug)]
pub struct SubscriptionStats {
    pub total_subscriptions: usize,
    pub service_breakdown: HashMap<Service, usize>,
    pub polling_active_count: usize,
    pub total_renewals: u32,
    pub firewall_status: FirewallStatus,
}

impl std::fmt::Display for SubscriptionStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Subscription Manager Stats:")?;
        writeln!(f, "  Total subscriptions: {}", self.total_subscriptions)?;
        writeln!(f, "  Firewall status: {:?}", self.firewall_status)?;
        writeln!(f, "  Polling active: {}", self.polling_active_count)?;
        writeln!(f, "  Total renewals: {}", self.total_renewals)?;
        writeln!(f, "  Service breakdown:")?;
        for (service, count) in &self.service_breakdown {
            writeln!(f, "    {:?}: {}", service, count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_wrapper_creation() {
        // Note: We can't easily test ManagedSubscription creation without actual devices
        // So we'll test the basic wrapper functionality that doesn't require network calls
        let _reg_id = RegistrationId::new(1);
        let pair = SpeakerServicePair::new(
            "192.168.1.100".parse().unwrap(),
            Service::AVTransport,
        );

        // Basic tests for the pair functionality
        assert_eq!(pair.speaker_ip.to_string(), "192.168.1.100");
        assert_eq!(pair.service, Service::AVTransport);
    }

    #[tokio::test]
    async fn test_subscription_manager_creation() {
        let manager = SubscriptionManager::new(
            "http://192.168.1.50:3400/callback".to_string(),
        );

        // Test initial state
        assert_eq!(manager.firewall_status().await, FirewallStatus::Unknown);
        assert_eq!(manager.list_subscriptions().await.len(), 0);

        // Test firewall status updates
        manager.set_firewall_status(FirewallStatus::Accessible).await;
        assert_eq!(manager.firewall_status().await, FirewallStatus::Accessible);
    }

    #[tokio::test]
    async fn test_subscription_stats() {
        let manager = SubscriptionManager::new(
            "http://192.168.1.50:3400/callback".to_string(),
        );

        let stats = manager.stats().await;
        assert_eq!(stats.total_subscriptions, 0);
        assert_eq!(stats.polling_active_count, 0);
        assert_eq!(stats.firewall_status, FirewallStatus::Unknown);
    }
}