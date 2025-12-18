//! Subscription lifecycle management.
//!
//! This module contains the SubscriptionManager which handles:
//! - Creating new subscriptions via strategies
//! - Checking for duplicate subscriptions
//! - Registering subscriptions with the callback server
//! - Unsubscribing and cleanup
//! - Emitting subscription lifecycle events

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};

use super::CallbackAdapter;
use crate::error::{BrokerError, Result};
use crate::event::Event;
use crate::services::ServiceStrategy;
use crate::subscription::Subscription;
use crate::types::{BrokerConfig, ServiceType, Speaker, SubscriptionConfig, SubscriptionKey};

/// Active subscription state tracked by the broker.
///
/// This struct contains all the information needed to manage a subscription's lifecycle,
/// including the subscription instance itself and metadata about when it was created
/// and last received an event.
pub struct ActiveSubscription {
    /// The unique key identifying this subscription
    pub key: SubscriptionKey,
    /// The subscription instance that handles UPnP operations
    pub subscription: Box<dyn Subscription>,
    /// When this subscription was created
    pub created_at: SystemTime,
    /// When the last event was received (None if no events yet)
    pub last_event: Option<SystemTime>,
}

impl ActiveSubscription {
    /// Create a new active subscription.
    pub fn new(key: SubscriptionKey, subscription: Box<dyn Subscription>) -> Self {
        Self {
            key,
            subscription,
            created_at: SystemTime::now(),
            last_event: None,
        }
    }

    /// Update the last event timestamp to now.
    pub fn mark_event_received(&mut self) {
        self.last_event = Some(SystemTime::now());
    }
}

/// Manager for subscription lifecycle operations.
///
/// The SubscriptionManager handles:
/// - Creating subscriptions via strategies
/// - Duplicate detection
/// - Callback server registration
/// - Unsubscription and cleanup
/// - Emitting lifecycle events
pub struct SubscriptionManager {
    /// Shared subscription state
    subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
    /// Registered strategies by service type
    strategies: Arc<HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>>>,
    /// Callback server for HTTP routing
    callback_server: Arc<callback_server::CallbackServer>,
    /// Callback adapter for Sonos-specific event conversion
    callback_adapter: CallbackAdapter,
    /// Event sender for emitting lifecycle events
    event_sender: mpsc::Sender<Event>,
    /// Broker configuration
    config: BrokerConfig,
}

impl SubscriptionManager {
    /// Create a new subscription manager.
    ///
    /// # Arguments
    ///
    /// * `subscriptions` - Shared subscription state
    /// * `strategies` - Map of service type to strategy implementation
    /// * `callback_server` - The callback server for receiving events
    /// * `callback_adapter` - The adapter for converting notifications to raw events
    /// * `event_sender` - Channel sender for emitting events
    /// * `config` - Broker configuration
    pub fn new(
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        strategies: Arc<HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>>>,
        callback_server: Arc<callback_server::CallbackServer>,
        callback_adapter: CallbackAdapter,
        event_sender: mpsc::Sender<Event>,
        config: BrokerConfig,
    ) -> Self {
        Self {
            subscriptions,
            strategies,
            callback_server,
            callback_adapter,
            event_sender,
            config,
        }
    }

    /// Subscribe to a specific service on a speaker.
    ///
    /// This method creates a new subscription for the given speaker-service combination.
    /// If a subscription already exists for this combination, an error is returned.
    ///
    /// # Process
    ///
    /// 1. Check for duplicate subscriptions
    /// 2. Look up the strategy for the service type
    /// 3. Generate a callback URL using the callback server
    /// 4. Call the strategy to create the subscription
    /// 5. Register the subscription with the callback server
    /// 6. Store the subscription in the internal map
    /// 7. Emit a `SubscriptionEstablished` event on success
    ///
    /// # Parameters
    ///
    /// * `speaker` - The speaker to subscribe to
    /// * `service_type` - The service type to subscribe to
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the subscription was created successfully.
    ///
    /// # Errors
    ///
    /// * `BrokerError::SubscriptionAlreadyExists` - A subscription already exists
    /// * `BrokerError::NoStrategyForService` - No strategy is registered
    /// * `BrokerError::StrategyError` - The strategy failed to create the subscription
    pub async fn subscribe(&self, speaker: &Speaker, service_type: ServiceType) -> Result<()> {
        let key = SubscriptionKey::new(speaker.id.clone(), service_type);

        // Check for duplicate subscriptions
        {
            let subs = self.subscriptions.read().await;
            if subs.contains_key(&key) {
                return Err(BrokerError::SubscriptionAlreadyExists {
                    speaker_id: speaker.id.clone(),
                    service_type,
                });
            }
        }

        // Look up strategy for service type
        let strategy = self
            .strategies
            .get(&service_type)
            .ok_or(BrokerError::NoStrategyForService(service_type))?;

        // Generate callback URL using callback server base URL
        let callback_url = self.callback_server.base_url().to_string();
        println!("🔗 Using callback URL: {}", callback_url);

        // Create subscription config
        let config = SubscriptionConfig::new(
            self.config.subscription_timeout.as_secs() as u32,
            callback_url.clone(),
        );

        // Call strategy to create subscription
        let subscription_result = strategy.create_subscription(speaker, callback_url, &config).await;

        match subscription_result {
            Ok(subscription) => {
                let subscription_id = subscription.subscription_id().to_string();

                // Register subscription with callback server and adapter
                self.callback_server
                    .router()
                    .register(subscription_id.clone())
                    .await;
                self.callback_adapter
                    .register_subscription(
                        subscription_id.clone(),
                        speaker.id.clone(),
                        service_type,
                    )
                    .await;

                // Store subscription in internal map
                let active_sub = ActiveSubscription::new(key.clone(), subscription);
                {
                    let mut subs = self.subscriptions.write().await;
                    subs.insert(key, active_sub);
                }

                // Emit SubscriptionEstablished event on success
                let _ = self
                    .event_sender
                    .send(Event::SubscriptionEstablished {
                        speaker_id: speaker.id.clone(),
                        service_type,
                        subscription_id,
                    })
                    .await;

                Ok(())
            }
            Err(e) => {
                // Emit SubscriptionFailed event on error
                let error_msg = e.to_string();
                let _ = self
                    .event_sender
                    .send(Event::SubscriptionFailed {
                        speaker_id: speaker.id.clone(),
                        service_type,
                        error: error_msg.clone(),
                    })
                    .await;

                Err(BrokerError::StrategyError(e))
            }
        }
    }

    /// Unsubscribe from a specific service on a speaker.
    ///
    /// This method removes an existing subscription for the given speaker-service combination.
    /// If no subscription exists for this combination, the operation completes gracefully.
    ///
    /// # Process
    ///
    /// 1. Look up the subscription by key
    /// 2. Call `unsubscribe()` on the subscription instance
    /// 3. Unregister from the callback server
    /// 4. Remove from the internal map
    /// 5. Emit a `SubscriptionRemoved` event
    ///
    /// # Parameters
    ///
    /// * `speaker` - The speaker to unsubscribe from
    /// * `service_type` - The service type to unsubscribe from
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the unsubscribe operation completed successfully.
    /// Returns `Ok(())` even if no subscription existed (graceful handling).
    pub async fn unsubscribe(&self, speaker: &Speaker, service_type: ServiceType) -> Result<()> {
        let key = SubscriptionKey::new(speaker.id.clone(), service_type);

        // Look up subscription by key
        let subscription_opt = {
            let mut subs = self.subscriptions.write().await;
            subs.remove(&key)
        };

        // If subscription exists, clean it up
        if let Some(mut active_sub) = subscription_opt {
            let subscription_id = active_sub.subscription.subscription_id().to_string();

            // Call unsubscribe() on subscription instance
            // Log errors but don't propagate them - we still want to clean up
            if let Err(e) = active_sub.subscription.unsubscribe().await {
                eprintln!(
                    "Warning: Failed to unsubscribe {}/{:?}: {}",
                    speaker.id.as_str(),
                    service_type,
                    e
                );
            }

            // Unregister from callback server and adapter
            self.callback_server
                .router()
                .unregister(&subscription_id)
                .await;
            self.callback_adapter
                .unregister_subscription(&subscription_id)
                .await;

            // Emit SubscriptionRemoved event
            let _ = self
                .event_sender
                .send(Event::SubscriptionRemoved {
                    speaker_id: speaker.id.clone(),
                    service_type,
                })
                .await;
        }

        // Handle non-existent subscriptions gracefully - just return Ok
        Ok(())
    }

    /// Shutdown all active subscriptions.
    ///
    /// This method unsubscribes from all active subscriptions and cleans up resources.
    /// It is called during broker shutdown to ensure proper cleanup.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if all subscriptions were cleaned up successfully.
    /// Errors from individual unsubscribe operations are logged but not propagated.
    pub async fn shutdown_all(&self) -> Result<()> {
        // Get list of all subscription keys
        let subscription_keys: Vec<_> = {
            let subs = self.subscriptions.read().await;
            subs.keys().cloned().collect()
        };

        // Unsubscribe from each subscription
        for key in subscription_keys {
            // Get the subscription and unsubscribe
            let subscription_opt = {
                let mut subs = self.subscriptions.write().await;
                subs.remove(&key)
            };

            if let Some(mut active_sub) = subscription_opt {
                let subscription_id = active_sub.subscription.subscription_id().to_string();

                // Call unsubscribe() on subscription instance
                // Log errors but don't fail shutdown
                if let Err(e) = active_sub.subscription.unsubscribe().await {
                    eprintln!(
                        "Warning: Failed to unsubscribe {}/{:?} during shutdown: {}",
                        key.speaker_id.as_str(),
                        key.service_type,
                        e
                    );
                }

                // Unregister from callback server and adapter
                self.callback_server
                    .router()
                    .unregister(&subscription_id)
                    .await;
                self.callback_adapter
                    .unregister_subscription(&subscription_id)
                    .await;
            }
        }

        // Clear the subscription map
        {
            let mut subs = self.subscriptions.write().await;
            subs.clear();
        }

        Ok(())
    }

    /// Get a reference to the callback server.
    ///
    /// This method is used by other components that need access to the callback server.
    #[allow(dead_code)]
    pub fn callback_server(&self) -> &Arc<callback_server::CallbackServer> {
        &self.callback_server
    }

    /// Get a reference to the subscriptions map.
    ///
    /// This method is used by other components that need access to subscription state.
    #[allow(dead_code)]
    pub fn subscriptions(&self) -> &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>> {
        &self.subscriptions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SpeakerId;

    // Mock subscription for testing
    struct MockSub {
        id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
    }

    #[async_trait::async_trait]
    impl crate::subscription::Subscription for MockSub {
        fn subscription_id(&self) -> &str {
            &self.id
        }
        async fn renew(&mut self) -> std::result::Result<(), crate::error::SubscriptionError> {
            Ok(())
        }
        async fn unsubscribe(&mut self) -> std::result::Result<(), crate::error::SubscriptionError> {
            Ok(())
        }
        fn is_active(&self) -> bool {
            true
        }
        fn time_until_renewal(&self) -> Option<std::time::Duration> {
            None
        }
        fn speaker_id(&self) -> &SpeakerId {
            &self.speaker_id
        }
        fn service_type(&self) -> ServiceType {
            self.service_type
        }
    }

    #[test]
    fn test_active_subscription_creation() {
        let speaker_id = SpeakerId::new("speaker1");
        let service_type = ServiceType::AVTransport;
        let key = SubscriptionKey::new(speaker_id.clone(), service_type);

        let mock_sub = MockSub {
            id: "test-sub-123".to_string(),
            speaker_id: speaker_id.clone(),
            service_type,
        };

        let active_sub = ActiveSubscription::new(key.clone(), Box::new(mock_sub));

        assert_eq!(active_sub.key, key);
        assert_eq!(active_sub.subscription.subscription_id(), "test-sub-123");
        assert!(active_sub.last_event.is_none());
        assert!(active_sub.created_at <= SystemTime::now());
    }

    #[test]
    fn test_active_subscription_mark_event_received() {
        let speaker_id = SpeakerId::new("speaker1");
        let key = SubscriptionKey::new(speaker_id.clone(), ServiceType::AVTransport);

        let mock_sub = MockSub {
            id: "test-sub-123".to_string(),
            speaker_id,
            service_type: ServiceType::AVTransport,
        };

        let mut active_sub = ActiveSubscription::new(key, Box::new(mock_sub));

        assert!(active_sub.last_event.is_none());

        active_sub.mark_event_received();

        assert!(active_sub.last_event.is_some());
        assert!(active_sub.last_event.unwrap() <= SystemTime::now());
    }
}
