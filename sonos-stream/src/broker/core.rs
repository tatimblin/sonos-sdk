//! Core EventBroker implementation.
//!
//! This module contains the main EventBroker struct and its public API methods.
//! The broker coordinates between manager components and exposes a clean public
//! interface for subscription management and event streaming.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::callback::CallbackServer;
use crate::event::Event;
use crate::strategy::SubscriptionStrategy;
use crate::types::{BrokerConfig, ServiceType, SubscriptionKey};

// Import manager types
use super::event_processor::EventProcessor;
use super::renewal_manager::RenewalManager;
use super::subscription_manager::{ActiveSubscription, SubscriptionManager};

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
///
/// # Example
///
/// ```rust,ignore
/// use sonos_stream::{EventBrokerBuilder, ServiceType};
///
/// let broker = EventBrokerBuilder::new()
///     .with_strategy(Box::new(AVTransportStrategy))
///     .with_port_range(3400, 3500)
///     .build()
///     .await?;
///
/// // Subscribe to a service
/// broker.subscribe(&speaker, ServiceType::AVTransport).await?;
///
/// // Receive events
/// let mut events = broker.event_stream();
/// while let Some(event) = events.recv().await {
///     // Handle event
/// }
///
/// // Cleanup
/// broker.shutdown().await?;
/// ```
pub struct EventBroker {
    /// Map of active subscriptions by (speaker_id, service_type)
    #[allow(dead_code)]
    subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
    /// Map of registered strategies by service type
    #[allow(dead_code)]
    strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
    /// Callback server for receiving UPnP events
    callback_server: Arc<CallbackServer>,
    /// Sender for emitting events to the application
    event_sender: mpsc::Sender<Event>,
    /// Receiver for the event stream (taken by event_stream())
    event_receiver: Option<mpsc::Receiver<Event>>,
    /// Broker configuration
    #[allow(dead_code)]
    config: BrokerConfig,
    /// Subscription manager for lifecycle operations
    subscription_manager: SubscriptionManager,
    /// Renewal manager for automatic renewal
    renewal_manager: RenewalManager,
    /// Event processor for routing and parsing
    #[allow(dead_code)]
    event_processor: EventProcessor,
}

impl EventBroker {
    /// Create a new event broker (internal, use EventBrokerBuilder).
    ///
    /// This constructor is intentionally private to enforce use of the builder pattern,
    /// which ensures proper validation and initialization.
    ///
    /// # Arguments
    ///
    /// * `strategies` - Arc-wrapped map of service type to strategy implementation
    /// * `callback_server` - Arc-wrapped callback server for receiving events
    /// * `config` - Broker configuration
    /// * `event_sender` - Channel sender for emitting events
    /// * `event_receiver` - Channel receiver for the event stream
    /// * `subscription_manager` - Manager for subscription lifecycle operations
    /// * `renewal_manager` - Manager for automatic renewal
    /// * `event_processor` - Manager for event routing and parsing
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
        callback_server: Arc<CallbackServer>,
        config: BrokerConfig,
        event_sender: mpsc::Sender<Event>,
        event_receiver: mpsc::Receiver<Event>,
        subscription_manager: SubscriptionManager,
        renewal_manager: RenewalManager,
        event_processor: EventProcessor,
    ) -> Self {
        let subscriptions = Arc::new(RwLock::new(HashMap::new()));

        Self {
            subscriptions,
            strategies,
            callback_server,
            event_sender,
            event_receiver: Some(event_receiver),
            config,
            subscription_manager,
            renewal_manager,
            event_processor,
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
    /// The actual subscription creation is delegated to the SubscriptionManager.
    ///
    /// # Process
    ///
    /// 1. Delegate to SubscriptionManager.subscribe()
    /// 2. SubscriptionManager checks for duplicates
    /// 3. SubscriptionManager looks up the strategy
    /// 4. SubscriptionManager creates the subscription via strategy
    /// 5. SubscriptionManager registers with callback server
    /// 6. SubscriptionManager stores in subscription map
    /// 7. SubscriptionManager emits SubscriptionEstablished event
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
    ///
    /// # Events
    ///
    /// * `Event::SubscriptionEstablished` - Emitted on success
    /// * `Event::SubscriptionFailed` - Emitted on failure
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
        // Delegate to SubscriptionManager
        self.subscription_manager.subscribe(speaker, service_type).await
    }

    /// Unsubscribe from a specific service on a speaker.
    ///
    /// This method removes an existing subscription for the given speaker-service combination.
    /// If no subscription exists for this combination, the operation completes gracefully.
    ///
    /// The actual unsubscription is delegated to the SubscriptionManager.
    ///
    /// # Process
    ///
    /// 1. Delegate to SubscriptionManager.unsubscribe()
    /// 2. SubscriptionManager looks up the subscription
    /// 3. SubscriptionManager calls unsubscribe() on the subscription
    /// 4. SubscriptionManager unregisters from callback server
    /// 5. SubscriptionManager removes from subscription map
    /// 6. SubscriptionManager emits SubscriptionRemoved event
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
    /// * `Event::SubscriptionRemoved` - Emitted when the subscription is removed
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
        // Delegate to SubscriptionManager
        self.subscription_manager.unsubscribe(speaker, service_type).await
    }

    /// Shutdown the broker and clean up all resources.
    ///
    /// This method performs a graceful shutdown of the broker by:
    /// 1. Shutting down the RenewalManager (stops background renewal task)
    /// 2. Shutting down the EventProcessor (stops event processing task)
    /// 3. Delegating to SubscriptionManager to unsubscribe from all subscriptions
    /// 4. Shutting down the callback server
    /// 5. Closing event channels
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if shutdown completed successfully.
    ///
    /// # Errors
    ///
    /// * `BrokerError::ShutdownError` - If shutdown fails or times out
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_stream::EventBroker;
    ///
    /// // Use the broker...
    ///
    /// // Clean shutdown
    /// broker.shutdown().await?;
    /// ```
    pub async fn shutdown(self) -> crate::error::Result<()> {
        use crate::error::BrokerError;

        // TODO: Implement shutdown sequence once manager components are complete
        // 1. Shutdown RenewalManager (task 4) - DONE
        // 2. Shutdown EventProcessor (task 5)
        // 3. Shutdown SubscriptionManager - unsubscribe all (task 3) - DONE
        // 4. Shutdown callback server
        // 5. Close event channels

        // 1. Shutdown RenewalManager
        self.renewal_manager.shutdown().await?;

        // 2. TODO: Shutdown EventProcessor (task 5)

        // 3. Shutdown SubscriptionManager - unsubscribe all
        self.subscription_manager.shutdown_all().await?;

        // 4. Shutdown callback server
        // We need to take ownership of the callback server from the Arc
        // The SubscriptionManager also holds a reference, so we need to drop it first
        drop(self.subscription_manager);
        
        match Arc::try_unwrap(self.callback_server) {
            Ok(callback_server) => {
                if let Err(e) = callback_server.shutdown().await {
                    return Err(BrokerError::ShutdownError(format!(
                        "Failed to shutdown callback server: {e}"
                    )));
                }
            }
            Err(_) => {
                // Arc still has other references, which shouldn't happen in normal usage
                eprintln!("Warning: Callback server has multiple references during shutdown");
            }
        }

        // 5. Close event channels
        drop(self.event_sender);
        drop(self.event_receiver);

        Ok(())
    }
}
