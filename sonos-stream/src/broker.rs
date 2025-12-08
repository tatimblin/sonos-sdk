//! Event broker for managing UPnP event subscriptions.
//!
//! The `EventBroker` is the central component that manages subscription lifecycle,
//! routes events to strategies for parsing, and emits lifecycle events to the application.
//!
//! # Architecture
//!
//! The broker uses a strategy pattern to delegate service-specific operations:
//! - Subscription creation is delegated to `SubscriptionStrategy` implementations
//! - Event parsing is delegated to the strategy for the service type
//! - The broker handles lifecycle, renewal, error handling, and event routing
//!
//! # Usage
//!
//! The broker is created using `EventBrokerBuilder`:
//!
//! ```rust,ignore
//! use sonos_stream::{EventBrokerBuilder, ServiceType};
//!
//! let broker = EventBrokerBuilder::new()
//!     .with_strategy(Box::new(AVTransportStrategy))
//!     .with_port_range(3400, 3500)
//!     .build()
//!     .await?;
//!
//! // Subscribe to a service
//! broker.subscribe(&speaker, ServiceType::AVTransport).await?;
//!
//! // Receive events
//! let mut events = broker.event_stream();
//! while let Some(event) = events.recv().await {
//!     // Handle event
//! }
//!
//! // Cleanup
//! broker.shutdown().await?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::callback_server::{CallbackServer, RawEvent};
use crate::strategy::SubscriptionStrategy;
use crate::subscription::Subscription;
use crate::types::{BrokerConfig, ServiceType, SubscriptionKey};

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

/// Event broker for managing UPnP event subscriptions.
///
/// The broker is the central component that:
/// - Manages subscription lifecycle (subscribe, unsubscribe, renewal)
/// - Routes incoming events to appropriate strategies for parsing
/// - Emits lifecycle and error events to the application
/// - Handles automatic renewal in a background task
/// - Manages the callback server for receiving UPnP notifications
///
/// # Thread Safety
///
/// The broker is designed to be used from multiple async tasks. All public methods
/// are async and use internal locking to ensure thread safety.
///
/// # Resource Management
///
/// The broker owns several resources that must be cleaned up:
/// - Active subscriptions (unsubscribed on shutdown)
/// - Callback server (stopped on shutdown)
/// - Background renewal task (terminated on shutdown)
/// - Event channels (closed on shutdown)
///
/// Call `shutdown()` to ensure proper cleanup, or the `Drop` implementation will
/// attempt cleanup (though async cleanup in Drop is limited).
pub struct EventBroker {
    /// Map of active subscriptions by (speaker_id, service_type)
    subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
    /// Map of registered strategies by service type
    strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
    /// Callback server for receiving UPnP events
    callback_server: Arc<CallbackServer>,
    /// Sender for emitting events to the application
    event_sender: mpsc::Sender<Event>,
    /// Receiver for the event stream (taken by event_stream())
    event_receiver: Option<mpsc::Receiver<Event>>,
    /// Broker configuration
    config: BrokerConfig,
    /// Background task handle for subscription renewal
    background_task: Option<JoinHandle<()>>,
    /// Shutdown signal sender for background task
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Event processing task handle
    event_processing_task: Option<JoinHandle<()>>,
}

impl EventBroker {
    /// Create a new event broker (private, use EventBrokerBuilder).
    ///
    /// This constructor is intentionally private to enforce use of the builder pattern,
    /// which ensures proper validation and initialization.
    ///
    /// # Arguments
    ///
    /// * `strategies` - Map of service type to strategy implementation
    /// * `callback_server` - The callback server for receiving events
    /// * `config` - Broker configuration
    /// * `event_sender` - Channel sender for emitting events
    /// * `event_receiver` - Channel receiver for the event stream
    /// * `background_task` - Handle to the background renewal task
    /// * `shutdown_tx` - Sender for signaling background task shutdown
    /// * `raw_event_rx` - Receiver for raw events from callback server
    pub(crate) fn new(
        strategies: HashMap<ServiceType, Box<dyn SubscriptionStrategy>>,
        callback_server: CallbackServer,
        config: BrokerConfig,
        event_sender: mpsc::Sender<Event>,
        event_receiver: mpsc::Receiver<Event>,
        background_task: JoinHandle<()>,
        shutdown_tx: mpsc::Sender<()>,
        raw_event_rx: mpsc::UnboundedReceiver<RawEvent>,
    ) -> Self {
        let subscriptions = Arc::new(RwLock::new(HashMap::new()));
        let strategies = Arc::new(strategies);
        
        // Start event processing task
        let event_processing_task = Self::start_event_processing_task(
            raw_event_rx,
            strategies.clone(),
            subscriptions.clone(),
            event_sender.clone(),
        );
        
        Self {
            subscriptions,
            strategies,
            callback_server: Arc::new(callback_server),
            event_sender,
            event_receiver: Some(event_receiver),
            config,
            background_task: Some(background_task),
            shutdown_tx: Some(shutdown_tx),
            event_processing_task: Some(event_processing_task),
        }
    }

    /// Start the background renewal task.
    ///
    /// This task runs on an interval and:
    /// 1. Checks all subscriptions for renewal needs using `time_until_renewal()`
    /// 2. Calls `renew()` on subscriptions that need renewal
    /// 3. Emits `SubscriptionRenewed` event on success
    /// 4. Implements retry logic with exponential backoff for renewal failures
    /// 5. Emits `SubscriptionExpired` event after all retries exhausted
    /// 6. Handles shutdown signal to stop task gracefully
    pub(crate) fn start_renewal_task(
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
        mut shutdown_rx: mpsc::Receiver<()>,
        config: BrokerConfig,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            // Check for renewals every 60 seconds
            let mut renewal_interval = tokio::time::interval(std::time::Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = renewal_interval.tick() => {
                        Self::check_and_renew_subscriptions(
                            &subscriptions,
                            &event_sender,
                            &config,
                        ).await;
                    }
                    _ = shutdown_rx.recv() => {
                        // Shutdown signal received, exit gracefully
                        break;
                    }
                }
            }
        })
    }

    /// Check all subscriptions and renew those that need renewal.
    ///
    /// This method iterates through all active subscriptions and checks if they need
    /// renewal using `time_until_renewal()`. For subscriptions that need renewal,
    /// it attempts to renew them with retry logic and exponential backoff.
    async fn check_and_renew_subscriptions(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
        config: &BrokerConfig,
    ) {
        // Get list of subscriptions that need renewal
        let subscriptions_to_renew: Vec<SubscriptionKey> = {
            let subs = subscriptions.read().await;
            subs.iter()
                .filter_map(|(key, active_sub)| {
                    if active_sub.subscription.is_active() {
                        if let Some(time_until) = active_sub.subscription.time_until_renewal() {
                            // Need renewal if within threshold
                            if time_until <= config.renewal_threshold {
                                return Some(key.clone());
                            }
                        }
                    }
                    None
                })
                .collect()
        };

        // Renew each subscription that needs it
        for key in subscriptions_to_renew {
            Self::renew_subscription_with_retry(
                subscriptions,
                &key,
                event_sender,
                config,
            ).await;
        }
    }

    /// Renew a single subscription with retry logic and exponential backoff.
    ///
    /// This method attempts to renew a subscription up to `max_retry_attempts` times,
    /// using exponential backoff between attempts. On success, it emits a
    /// `SubscriptionRenewed` event. On failure after all retries, it emits a
    /// `SubscriptionExpired` event and removes the subscription.
    async fn renew_subscription_with_retry(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        key: &SubscriptionKey,
        event_sender: &mpsc::Sender<Event>,
        config: &BrokerConfig,
    ) {
        let mut attempt = 0;
        let max_attempts = config.max_retry_attempts;
        let base_backoff = config.retry_backoff_base;

        loop {
            // Try to renew the subscription
            let renewal_result = {
                let mut subs = subscriptions.write().await;
                if let Some(active_sub) = subs.get_mut(key) {
                    active_sub.subscription.renew()
                } else {
                    // Subscription no longer exists, nothing to do
                    return;
                }
            };

            match renewal_result {
                Ok(()) => {
                    // Renewal succeeded, emit event
                    let _ = event_sender
                        .send(Event::SubscriptionRenewed {
                            speaker_id: key.speaker_id.clone(),
                            service_type: key.service_type,
                        })
                        .await;
                    return;
                }
                Err(e) => {
                    attempt += 1;

                    // Check if this is a non-retryable error
                    if matches!(e, crate::error::SubscriptionError::Expired) {
                        // Subscription already expired, don't retry
                        Self::handle_subscription_expiration(
                            subscriptions,
                            key,
                            event_sender,
                        ).await;
                        return;
                    }

                    // Check if we've exhausted all retry attempts
                    if attempt >= max_attempts {
                        // All retries exhausted, emit expiration event and remove subscription
                        Self::handle_subscription_expiration(
                            subscriptions,
                            key,
                            event_sender,
                        ).await;
                        return;
                    }

                    // Calculate backoff duration with exponential backoff
                    let backoff = base_backoff * 2_u32.pow(attempt - 1);
                    
                    // Log the retry attempt (in production, use proper logging)
                    eprintln!(
                        "Renewal failed for {}/{:?} (attempt {}/{}): {}. Retrying in {:?}...",
                        key.speaker_id.as_str(),
                        key.service_type,
                        attempt,
                        max_attempts,
                        e,
                        backoff
                    );

                    // Wait before retrying
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    /// Handle subscription expiration by removing it and emitting an event.
    ///
    /// This method is called when a subscription fails to renew after all retry
    /// attempts have been exhausted. It removes the subscription from the internal
    /// map and emits a `SubscriptionExpired` event.
    async fn handle_subscription_expiration(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        key: &SubscriptionKey,
        event_sender: &mpsc::Sender<Event>,
    ) {
        // Remove the subscription
        let mut subs = subscriptions.write().await;
        subs.remove(key);
        drop(subs);

        // Emit expiration event
        let _ = event_sender
            .send(Event::SubscriptionExpired {
                speaker_id: key.speaker_id.clone(),
                service_type: key.service_type,
            })
            .await;
    }

    /// Start the event processing task that receives raw events and routes them to strategies.
    ///
    /// This task runs in the background and:
    /// 1. Receives raw events from the callback server
    /// 2. Routes events to the appropriate strategy for parsing
    /// 3. Emits ServiceEvent with parsed data on success
    /// 4. Emits ParseError event on parse failure
    /// 5. Ensures errors don't stop event processing
    fn start_event_processing_task(
        mut raw_event_rx: mpsc::UnboundedReceiver<RawEvent>,
        strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(raw_event) = raw_event_rx.recv().await {
                // Process the event
                Self::process_raw_event(
                    raw_event,
                    &strategies,
                    &subscriptions,
                    &event_sender,
                )
                .await;
            }
        })
    }

    /// Process a single raw event from the callback server.
    ///
    /// This method:
    /// 1. Looks up the strategy for the service type
    /// 2. Calls the strategy to parse the event
    /// 3. Emits ServiceEvent for each parsed event
    /// 4. Emits ParseError if parsing fails
    /// 5. Updates the subscription's last event timestamp
    async fn process_raw_event(
        raw_event: RawEvent,
        strategies: &Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
    ) {
        let speaker_id = raw_event.speaker_id.clone();
        let service_type = raw_event.service_type;
        let event_xml = raw_event.event_xml;

        // Look up strategy for service type
        let strategy = match strategies.get(&service_type) {
            Some(s) => s,
            None => {
                // No strategy registered - emit parse error
                let _ = event_sender
                    .send(Event::ParseError {
                        speaker_id,
                        service_type,
                        error: format!("No strategy registered for service type: {:?}", service_type),
                    })
                    .await;
                return;
            }
        };

        // Parse the event using the strategy
        match strategy.parse_event(&speaker_id, &event_xml) {
            Ok(parsed_events) => {
                // Emit ServiceEvent for each parsed event
                for parsed_event in parsed_events {
                    let _ = event_sender
                        .send(Event::ServiceEvent {
                            speaker_id: speaker_id.clone(),
                            service_type,
                            event: parsed_event,
                        })
                        .await;
                }

                // Update subscription's last event timestamp
                let key = SubscriptionKey::new(speaker_id, service_type);
                let mut subs = subscriptions.write().await;
                if let Some(active_sub) = subs.get_mut(&key) {
                    active_sub.mark_event_received();
                }
            }
            Err(e) => {
                // Emit ParseError event
                let _ = event_sender
                    .send(Event::ParseError {
                        speaker_id,
                        service_type,
                        error: e.to_string(),
                    })
                    .await;
            }
        }
    }

    /// Get the event stream receiver.
    ///
    /// This method returns the receiver for the event stream. It can only be called once,
    /// as the receiver is moved out of the broker. Subsequent calls will panic.
    ///
    /// # Returns
    ///
    /// Returns the receiver for the event stream.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut event_rx = broker.event_stream();
    /// while let Some(event) = event_rx.recv().await {
    ///     match event {
    ///         Event::SubscriptionEstablished { .. } => { /* handle */ }
    ///         Event::ServiceEvent { .. } => { /* handle */ }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub fn event_stream(&mut self) -> mpsc::Receiver<Event> {
        self.event_receiver
            .take()
            .expect("event_stream() can only be called once")
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
    /// * `BrokerError::SubscriptionAlreadyExists` - A subscription already exists for this speaker-service combination
    /// * `BrokerError::NoStrategyForService` - No strategy is registered for the service type
    /// * `BrokerError::StrategyError` - The strategy failed to create the subscription
    ///
    /// # Events
    ///
    /// * `Event::SubscriptionEstablished` - Emitted when the subscription is successfully created
    /// * `Event::SubscriptionFailed` - Emitted when the subscription fails to create
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_stream::{EventBroker, Speaker, ServiceType, SpeakerId};
    /// use std::net::IpAddr;
    ///
    /// let speaker = Speaker::new(
    ///     SpeakerId::new("RINCON_123"),
    ///     "192.168.1.100".parse::<IpAddr>().unwrap(),
    ///     "Living Room".to_string(),
    ///     "Living Room".to_string(),
    /// );
    ///
    /// broker.subscribe(&speaker, ServiceType::AVTransport).await?;
    /// ```
    pub async fn subscribe(
        &self,
        speaker: &crate::types::Speaker,
        service_type: ServiceType,
    ) -> crate::error::Result<()> {
        use crate::error::BrokerError;
        use crate::types::{SubscriptionConfig, SubscriptionKey};

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

        // Create subscription config
        let config = SubscriptionConfig::new(
            self.config.subscription_timeout.as_secs() as u32,
            callback_url.clone(),
        );

        // Call strategy to create subscription
        let subscription_result = strategy.create_subscription(speaker, callback_url, &config);

        match subscription_result {
            Ok(subscription) => {
                let subscription_id = subscription.subscription_id().to_string();

                // Register subscription with callback server
                self.callback_server
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
    /// If no subscription exists for this combination, the operation completes gracefully without error.
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
    ///
    /// # Errors
    ///
    /// This method does not return errors for non-existent subscriptions.
    /// Errors from the subscription's unsubscribe operation are logged but not propagated.
    ///
    /// # Events
    ///
    /// * `Event::SubscriptionRemoved` - Emitted when the subscription is successfully removed
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_stream::{EventBroker, Speaker, ServiceType, SpeakerId};
    /// use std::net::IpAddr;
    ///
    /// let speaker = Speaker::new(
    ///     SpeakerId::new("RINCON_123"),
    ///     "192.168.1.100".parse::<IpAddr>().unwrap(),
    ///     "Living Room".to_string(),
    ///     "Living Room".to_string(),
    /// );
    ///
    /// broker.unsubscribe(&speaker, ServiceType::AVTransport).await?;
    /// ```
    pub async fn unsubscribe(
        &self,
        speaker: &crate::types::Speaker,
        service_type: ServiceType,
    ) -> crate::error::Result<()> {
        use crate::types::SubscriptionKey;

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
            if let Err(e) = active_sub.subscription.unsubscribe() {
                eprintln!(
                    "Warning: Failed to unsubscribe {}/{:?}: {}",
                    speaker.id.as_str(),
                    service_type,
                    e
                );
            }

            // Unregister from callback server
            self.callback_server
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
}

/// Events emitted by the broker.
///
/// These events represent the lifecycle of subscriptions and the parsed events
/// from services. Applications should handle these events to track subscription
/// state and process service events.
#[derive(Debug, Clone)]
pub enum Event {
    /// A subscription was successfully established.
    SubscriptionEstablished {
        /// The speaker ID
        speaker_id: crate::types::SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// The UPnP subscription ID
        subscription_id: String,
    },

    /// A subscription failed to establish.
    SubscriptionFailed {
        /// The speaker ID
        speaker_id: crate::types::SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// Error message describing the failure
        error: String,
    },

    /// A subscription was successfully renewed.
    SubscriptionRenewed {
        /// The speaker ID
        speaker_id: crate::types::SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// A subscription expired after all renewal attempts failed.
    SubscriptionExpired {
        /// The speaker ID
        speaker_id: crate::types::SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// A subscription was removed (unsubscribed).
    SubscriptionRemoved {
        /// The speaker ID
        speaker_id: crate::types::SpeakerId,
        /// The service type
        service_type: ServiceType,
    },

    /// A parsed event from a service.
    ServiceEvent {
        /// The speaker ID
        speaker_id: crate::types::SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// The parsed event data
        event: crate::strategy::ParsedEvent,
    },

    /// An error occurred parsing an event.
    ParseError {
        /// The speaker ID
        speaker_id: crate::types::SpeakerId,
        /// The service type
        service_type: ServiceType,
        /// Error message describing the parse failure
        error: String,
    },
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

    impl crate::subscription::Subscription for MockSub {
        fn subscription_id(&self) -> &str {
            &self.id
        }
        fn renew(&mut self) -> std::result::Result<(), crate::error::SubscriptionError> {
            Ok(())
        }
        fn unsubscribe(&mut self) -> std::result::Result<(), crate::error::SubscriptionError> {
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
        use crate::subscription::Subscription;
        use crate::error::SubscriptionError;
        use std::time::Duration;

        // Mock subscription for testing
        struct MockSub {
            id: String,
            speaker_id: SpeakerId,
            service_type: ServiceType,
        }

        impl Subscription for MockSub {
            fn subscription_id(&self) -> &str {
                &self.id
            }
            fn renew(&mut self) -> std::result::Result<(), SubscriptionError> {
                Ok(())
            }
            fn unsubscribe(&mut self) -> std::result::Result<(), SubscriptionError> {
                Ok(())
            }
            fn is_active(&self) -> bool {
                true
            }
            fn time_until_renewal(&self) -> Option<Duration> {
                None
            }
            fn speaker_id(&self) -> &SpeakerId {
                &self.speaker_id
            }
            fn service_type(&self) -> ServiceType {
                self.service_type
            }
        }

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
        use crate::subscription::Subscription;
        use crate::error::SubscriptionError;
        use std::time::Duration;

        struct MockSub {
            id: String,
            speaker_id: SpeakerId,
            service_type: ServiceType,
        }

        impl Subscription for MockSub {
            fn subscription_id(&self) -> &str {
                &self.id
            }
            fn renew(&mut self) -> std::result::Result<(), SubscriptionError> {
                Ok(())
            }
            fn unsubscribe(&mut self) -> std::result::Result<(), SubscriptionError> {
                Ok(())
            }
            fn is_active(&self) -> bool {
                true
            }
            fn time_until_renewal(&self) -> Option<Duration> {
                None
            }
            fn speaker_id(&self) -> &SpeakerId {
                &self.speaker_id
            }
            fn service_type(&self) -> ServiceType {
                self.service_type
            }
        }

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

    #[test]
    fn test_event_debug() {
        let event = Event::SubscriptionEstablished {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::AVTransport,
            subscription_id: "test-sub-123".to_string(),
        };

        let debug_str = format!("{:?}", event);
        assert!(debug_str.contains("SubscriptionEstablished"));
        assert!(debug_str.contains("speaker1"));
        assert!(debug_str.contains("AVTransport"));
        assert!(debug_str.contains("test-sub-123"));
    }

    #[test]
    fn test_event_clone() {
        let event = Event::SubscriptionFailed {
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::RenderingControl,
            error: "connection failed".to_string(),
        };

        let cloned = event.clone();

        match (event, cloned) {
            (
                Event::SubscriptionFailed {
                    speaker_id: s1,
                    service_type: st1,
                    error: e1,
                },
                Event::SubscriptionFailed {
                    speaker_id: s2,
                    service_type: st2,
                    error: e2,
                },
            ) => {
                assert_eq!(s1, s2);
                assert_eq!(st1, st2);
                assert_eq!(e1, e2);
            }
            _ => panic!("Event type mismatch after clone"),
        }
    }

    // Mock strategy for testing
    struct MockStrategy {
        service_type: ServiceType,
        should_fail: bool,
    }

    impl MockStrategy {
        fn new(service_type: ServiceType) -> Self {
            Self {
                service_type,
                should_fail: false,
            }
        }

        fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }

    impl crate::strategy::SubscriptionStrategy for MockStrategy {
        fn service_type(&self) -> ServiceType {
            self.service_type
        }

        fn subscription_scope(&self) -> crate::types::SubscriptionScope {
            crate::types::SubscriptionScope::PerSpeaker
        }

        fn create_subscription(
            &self,
            speaker: &crate::types::Speaker,
            _callback_url: String,
            _config: &crate::types::SubscriptionConfig,
        ) -> Result<Box<dyn crate::subscription::Subscription>, crate::error::StrategyError> {
            if self.should_fail {
                return Err(crate::error::StrategyError::SubscriptionCreationFailed(
                    "Mock failure".to_string(),
                ));
            }

            Ok(Box::new(MockSub {
                id: format!("mock-sub-{}", speaker.id.as_str()),
                speaker_id: speaker.id.clone(),
                service_type: self.service_type,
            }))
        }

        fn parse_event(
            &self,
            _speaker_id: &SpeakerId,
            _event_xml: &str,
        ) -> Result<Vec<crate::strategy::ParsedEvent>, crate::error::StrategyError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_subscribe_success() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx,
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(MockStrategy::new(ServiceType::AVTransport))
                as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let mut broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe
        let result = broker.subscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_ok());

        // Verify subscription was stored
        let subs = broker.subscriptions.read().await;
        let key = crate::types::SubscriptionKey::new(speaker.id.clone(), ServiceType::AVTransport);
        assert!(subs.contains_key(&key));
        drop(subs); // Release lock before getting event stream

        // Get event stream and verify event was emitted
        let mut event_rx = broker.event_stream();
        let event = event_rx.try_recv().unwrap();
        match event {
            Event::SubscriptionEstablished {
                speaker_id,
                service_type,
                subscription_id,
            } => {
                assert_eq!(speaker_id, speaker.id);
                assert_eq!(service_type, ServiceType::AVTransport);
                assert_eq!(subscription_id, "mock-sub-RINCON_123");
            }
            _ => panic!("Expected SubscriptionEstablished event"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_duplicate() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx,
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(MockStrategy::new(ServiceType::AVTransport))
                as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe first time
        let result = broker.subscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_ok());

        // Subscribe second time - should fail
        let result = broker.subscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_err());

        match result {
            Err(crate::error::BrokerError::SubscriptionAlreadyExists {
                speaker_id,
                service_type,
            }) => {
                assert_eq!(speaker_id, speaker.id);
                assert_eq!(service_type, ServiceType::AVTransport);
            }
            _ => panic!("Expected SubscriptionAlreadyExists error"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_no_strategy() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx,
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies (empty - no strategies registered)
        let strategies = HashMap::new();

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe - should fail with no strategy
        let result = broker.subscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_err());

        match result {
            Err(crate::error::BrokerError::NoStrategyForService(service_type)) => {
                assert_eq!(service_type, ServiceType::AVTransport);
            }
            _ => panic!("Expected NoStrategyForService error"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_strategy_failure() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx,
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies with failing strategy
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(MockStrategy::new(ServiceType::AVTransport).with_failure())
                as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let mut broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe - should fail
        let result = broker.subscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_err());

        match result {
            Err(crate::error::BrokerError::StrategyError(_)) => {
                // Expected
            }
            _ => panic!("Expected StrategyError"),
        }

        // Verify SubscriptionFailed event was emitted
        let mut event_rx = broker.event_stream();
        let event = event_rx.try_recv().unwrap();
        match event {
            Event::SubscriptionFailed {
                speaker_id,
                service_type,
                error,
            } => {
                assert_eq!(speaker_id, speaker.id);
                assert_eq!(service_type, ServiceType::AVTransport);
                assert!(error.contains("Mock failure"));
            }
            _ => panic!("Expected SubscriptionFailed event"),
        }

        // Verify subscription was NOT stored
        let subs = broker.subscriptions.read().await;
        let key = crate::types::SubscriptionKey::new(speaker.id.clone(), ServiceType::AVTransport);
        assert!(!subs.contains_key(&key));
    }

    #[tokio::test]
    async fn test_unsubscribe_success() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx,
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(MockStrategy::new(ServiceType::AVTransport))
                as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let mut broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe first
        let result = broker.subscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_ok());

        // Verify subscription exists
        {
            let subs = broker.subscriptions.read().await;
            let key = crate::types::SubscriptionKey::new(speaker.id.clone(), ServiceType::AVTransport);
            assert!(subs.contains_key(&key));
        }

        // Unsubscribe
        let result = broker.unsubscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_ok());

        // Verify subscription was removed
        {
            let subs = broker.subscriptions.read().await;
            let key = crate::types::SubscriptionKey::new(speaker.id.clone(), ServiceType::AVTransport);
            assert!(!subs.contains_key(&key));
        }

        // Get event stream and verify events
        let mut event_rx = broker.event_stream();
        
        // First event should be SubscriptionEstablished
        let event = event_rx.try_recv().unwrap();
        match event {
            Event::SubscriptionEstablished { .. } => {
                // Expected
            }
            _ => panic!("Expected SubscriptionEstablished event"),
        }

        // Second event should be SubscriptionRemoved
        let event = event_rx.try_recv().unwrap();
        match event {
            Event::SubscriptionRemoved {
                speaker_id,
                service_type,
            } => {
                assert_eq!(speaker_id, speaker.id);
                assert_eq!(service_type, ServiceType::AVTransport);
            }
            _ => panic!("Expected SubscriptionRemoved event"),
        }
    }

    #[tokio::test]
    async fn test_unsubscribe_non_existent() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx,
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(MockStrategy::new(ServiceType::AVTransport))
                as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let mut broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Unsubscribe without subscribing first - should succeed gracefully
        let result = broker.unsubscribe(&speaker, ServiceType::AVTransport).await;
        assert!(result.is_ok());

        // Get event stream and verify no events were emitted
        let mut event_rx = broker.event_stream();
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_unsubscribe_multiple_services() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx,
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies for multiple services
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(MockStrategy::new(ServiceType::AVTransport))
                as Box<dyn crate::strategy::SubscriptionStrategy>,
        );
        strategies.insert(
            ServiceType::RenderingControl,
            Box::new(MockStrategy::new(ServiceType::RenderingControl))
                as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe to both services
        broker.subscribe(&speaker, ServiceType::AVTransport).await.unwrap();
        broker.subscribe(&speaker, ServiceType::RenderingControl).await.unwrap();

        // Verify both subscriptions exist
        {
            let subs = broker.subscriptions.read().await;
            assert_eq!(subs.len(), 2);
        }

        // Unsubscribe from one service
        broker.unsubscribe(&speaker, ServiceType::AVTransport).await.unwrap();

        // Verify only one subscription remains
        {
            let subs = broker.subscriptions.read().await;
            assert_eq!(subs.len(), 1);
            
            let key = crate::types::SubscriptionKey::new(
                speaker.id.clone(),
                ServiceType::RenderingControl,
            );
            assert!(subs.contains_key(&key));
        }

        // Unsubscribe from the other service
        broker.unsubscribe(&speaker, ServiceType::RenderingControl).await.unwrap();

        // Verify no subscriptions remain
        {
            let subs = broker.subscriptions.read().await;
            assert_eq!(subs.len(), 0);
        }
    }

    #[tokio::test]
    async fn test_event_routing_and_parsing() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;
        use std::collections::HashMap;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx.clone(),
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create mock strategy that returns parsed events
        struct TestStrategy;
        impl crate::strategy::SubscriptionStrategy for TestStrategy {
            fn service_type(&self) -> ServiceType {
                ServiceType::AVTransport
            }

            fn subscription_scope(&self) -> crate::types::SubscriptionScope {
                crate::types::SubscriptionScope::PerSpeaker
            }

            fn create_subscription(
                &self,
                speaker: &crate::types::Speaker,
                _callback_url: String,
                _config: &crate::types::SubscriptionConfig,
            ) -> Result<Box<dyn crate::subscription::Subscription>, crate::error::StrategyError> {
                Ok(Box::new(MockSub {
                    id: format!("test-sub-{}", speaker.id.as_str()),
                    speaker_id: speaker.id.clone(),
                    service_type: ServiceType::AVTransport,
                }))
            }

            fn parse_event(
                &self,
                _speaker_id: &crate::types::SpeakerId,
                event_xml: &str,
            ) -> Result<Vec<crate::strategy::ParsedEvent>, crate::error::StrategyError> {
                // Parse the test XML and return a custom event
                Ok(vec![crate::strategy::ParsedEvent::custom(
                    "test_event",
                    HashMap::from([("xml_content".to_string(), event_xml.to_string())]),
                )])
            }
        }

        // Create strategies
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(TestStrategy) as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let mut broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe
        broker.subscribe(&speaker, ServiceType::AVTransport).await.unwrap();

        // Get event stream
        let mut event_rx = broker.event_stream();

        // Consume the SubscriptionEstablished event
        let event = event_rx.recv().await.unwrap();
        match event {
            Event::SubscriptionEstablished { .. } => {
                // Expected
            }
            _ => panic!("Expected SubscriptionEstablished event"),
        }

        // Simulate a raw event from the callback server
        let raw_event = crate::callback_server::RawEvent {
            subscription_id: "test-sub-RINCON_123".to_string(),
            speaker_id: speaker.id.clone(),
            service_type: ServiceType::AVTransport,
            event_xml: "<test>event data</test>".to_string(),
        };

        raw_event_tx.send(raw_event).unwrap();

        // Wait for the ServiceEvent to be emitted
        let event = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            event_rx.recv()
        ).await.unwrap().unwrap();

        match event {
            Event::ServiceEvent {
                speaker_id: evt_speaker_id,
                service_type: evt_service_type,
                event: parsed_event,
            } => {
                assert_eq!(evt_speaker_id, speaker.id);
                assert_eq!(evt_service_type, ServiceType::AVTransport);
                assert_eq!(parsed_event.event_type(), "test_event");
                assert_eq!(
                    parsed_event.data().get("xml_content").map(|s| s.as_str()),
                    Some("<test>event data</test>")
                );
            }
            _ => panic!("Expected ServiceEvent, got {:?}", event),
        }

        // Verify subscription's last_event was updated
        let subs = broker.subscriptions.read().await;
        let key = crate::types::SubscriptionKey::new(speaker.id.clone(), ServiceType::AVTransport);
        let active_sub = subs.get(&key).unwrap();
        assert!(active_sub.last_event.is_some());
    }

    #[tokio::test]
    async fn test_event_parsing_error() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx.clone(),
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create mock strategy that fails to parse
        struct FailingStrategy;
        impl crate::strategy::SubscriptionStrategy for FailingStrategy {
            fn service_type(&self) -> ServiceType {
                ServiceType::AVTransport
            }

            fn subscription_scope(&self) -> crate::types::SubscriptionScope {
                crate::types::SubscriptionScope::PerSpeaker
            }

            fn create_subscription(
                &self,
                speaker: &crate::types::Speaker,
                _callback_url: String,
                _config: &crate::types::SubscriptionConfig,
            ) -> Result<Box<dyn crate::subscription::Subscription>, crate::error::StrategyError> {
                Ok(Box::new(MockSub {
                    id: format!("test-sub-{}", speaker.id.as_str()),
                    speaker_id: speaker.id.clone(),
                    service_type: ServiceType::AVTransport,
                }))
            }

            fn parse_event(
                &self,
                _speaker_id: &crate::types::SpeakerId,
                _event_xml: &str,
            ) -> Result<Vec<crate::strategy::ParsedEvent>, crate::error::StrategyError> {
                Err(crate::error::StrategyError::EventParseFailed(
                    "Invalid XML format".to_string(),
                ))
            }
        }

        // Create strategies
        let mut strategies = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(FailingStrategy) as Box<dyn crate::strategy::SubscriptionStrategy>,
        );

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let mut broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Subscribe
        broker.subscribe(&speaker, ServiceType::AVTransport).await.unwrap();

        // Get event stream
        let mut event_rx = broker.event_stream();

        // Consume the SubscriptionEstablished event
        let event = event_rx.recv().await.unwrap();
        match event {
            Event::SubscriptionEstablished { .. } => {
                // Expected
            }
            _ => panic!("Expected SubscriptionEstablished event"),
        }

        // Simulate a raw event from the callback server
        let raw_event = crate::callback_server::RawEvent {
            subscription_id: "test-sub-RINCON_123".to_string(),
            speaker_id: speaker.id.clone(),
            service_type: ServiceType::AVTransport,
            event_xml: "<invalid>malformed xml".to_string(),
        };

        raw_event_tx.send(raw_event).unwrap();

        // Wait for the ParseError event to be emitted
        let event = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            event_rx.recv()
        ).await.unwrap().unwrap();

        match event {
            Event::ParseError {
                speaker_id: evt_speaker_id,
                service_type: evt_service_type,
                error,
            } => {
                assert_eq!(evt_speaker_id, speaker.id);
                assert_eq!(evt_service_type, ServiceType::AVTransport);
                assert!(error.contains("Invalid XML format"));
            }
            _ => panic!("Expected ParseError event, got {:?}", event),
        }
    }

    #[tokio::test]
    async fn test_event_routing_no_strategy() {
        use crate::types::{BrokerConfig, Speaker};
        use std::net::IpAddr;

        // Create callback server
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
        let callback_server = crate::callback_server::CallbackServer::new(
            (50000, 50100),
            raw_event_tx.clone(),
        )
        .await
        .unwrap();

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(10);

        // Create strategies (empty - no strategies registered)
        let strategies = HashMap::new();

        // Create background task (dummy for testing)
        let (_shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let background_task = tokio::spawn(async move {
            shutdown_rx.recv().await;
        });

        let (shutdown_tx, _) = mpsc::channel::<()>(1);

        // Create broker
        let mut broker = EventBroker::new(
            strategies,
            callback_server,
            BrokerConfig::default(),
            event_tx,
            event_rx,
            background_task,
            shutdown_tx,
            raw_event_rx,
        );

        // Get event stream
        let mut event_rx = broker.event_stream();

        // Create speaker
        let speaker = Speaker::new(
            SpeakerId::new("RINCON_123"),
            "192.168.1.100".parse::<IpAddr>().unwrap(),
            "Living Room".to_string(),
            "Living Room".to_string(),
        );

        // Simulate a raw event for a service with no registered strategy
        let raw_event = crate::callback_server::RawEvent {
            subscription_id: "test-sub-123".to_string(),
            speaker_id: speaker.id.clone(),
            service_type: ServiceType::AVTransport,
            event_xml: "<test>event data</test>".to_string(),
        };

        raw_event_tx.send(raw_event).unwrap();

        // Wait for the ParseError event to be emitted
        let event = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            event_rx.recv()
        ).await.unwrap().unwrap();

        match event {
            Event::ParseError {
                speaker_id: evt_speaker_id,
                service_type: evt_service_type,
                error,
            } => {
                assert_eq!(evt_speaker_id, speaker.id);
                assert_eq!(evt_service_type, ServiceType::AVTransport);
                assert!(error.contains("No strategy registered"));
            }
            _ => panic!("Expected ParseError event, got {:?}", event),
        }
    }
}
