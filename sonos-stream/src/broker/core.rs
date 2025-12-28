//! Core EventBroker implementation.
//!
//! This module contains the main EventBroker struct and its public API methods.
//! The broker coordinates between manager components and exposes a clean public
//! interface for event streaming and processing.
//!
//! Note: Subscription management is now handled by the `sonos-api` crate's
//! `SonosClient` and `ManagedSubscription` types.

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::event::Event;

// Import manager types
use super::event_processor::EventProcessor;

/// Event broker for managing UPnP event streaming and processing.
///
/// The broker is the central component that:
/// - Routes incoming events to appropriate strategies for parsing
/// - Emits lifecycle and error events to the application
/// - Handles automatic renewal in a background task
/// - Manages the callback server for receiving UPnP notifications
///
/// Note: Subscription creation and management is now handled by the `sonos-api` crate's
/// `SonosClient` and `ManagedSubscription` types. This broker focuses on event processing.
///
/// # Thread Safety
///
/// The broker is designed to be used from multiple async tasks. All public methods
/// are async and use internal locking to ensure thread safety.
///
/// # Resource Management
///
/// The broker owns several resources that must be cleaned up:
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
/// use sonos_stream::EventBrokerBuilder;
///
/// let broker = EventBrokerBuilder::new()
///     .with_strategy(Box::new(AVTransportStrategy))
///     .with_port_range(3400, 3500)
///     .build()
///     .await?;
///
/// // Get callback URL for creating subscriptions via sonos-api
/// let callback_url = broker.callback_url();
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
    /// Callback server for receiving UPnP events
    callback_server: Arc<callback_server::CallbackServer>,
    /// Sender for emitting events to the application
    event_sender: mpsc::Sender<Event>,
    /// Receiver for the event stream (taken by event_stream())
    event_receiver: Option<mpsc::Receiver<Event>>,
    /// Event processor for routing and parsing
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
    /// * `callback_server` - Arc-wrapped callback server for receiving events
    /// * `event_sender` - Channel sender for emitting events
    /// * `event_receiver` - Channel receiver for the event stream
    /// * `event_processor` - Manager for event routing and parsing
    pub(crate) fn new(
        callback_server: Arc<callback_server::CallbackServer>,
        event_sender: mpsc::Sender<Event>,
        event_receiver: mpsc::Receiver<Event>,
        event_processor: EventProcessor,
    ) -> Self {
        Self {
            callback_server,
            event_sender,
            event_receiver: Some(event_receiver),
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

    /// Get the callback URL for creating subscriptions.
    ///
    /// This URL should be used when creating subscriptions via the `sonos-api` crate's
    /// `SonosClient.create_managed_subscription()` method.
    ///
    /// # Returns
    ///
    /// Returns the base callback URL that should be used for UPnP subscriptions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use sonos_api::SonosClient;
    ///
    /// let callback_url = broker.callback_url();
    /// let client = SonosClient::new();
    /// let subscription = client.create_managed_subscription(
    ///     "192.168.1.100",
    ///     Service::AVTransport,
    ///     &callback_url,
    ///     1800
    /// )?;
    /// ```
    pub fn callback_url(&self) -> String {
        self.callback_server.base_url().to_string()
    }

    /// Shutdown the broker and clean up all resources.
    ///
    /// This method performs a graceful shutdown of the broker by:
    /// 1. Shutting down the EventProcessor (stops event processing task)
    /// 2. Shutting down the callback server
    /// 3. Closing event channels
    ///
    /// Note: Subscription cleanup is now handled by the `sonos-api` crate's
    /// `ManagedSubscription` types, which should be dropped or explicitly
    /// unsubscribed before shutting down the broker.
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

        // 1. Shutdown EventProcessor
        if let Err(e) = self.event_processor.shutdown().await {
            return Err(BrokerError::ShutdownError(format!(
                "Failed to shutdown event processor: {e}"
            )));
        }

        // 2. Shutdown callback server
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

        // 3. Close event channels
        drop(self.event_sender);
        drop(self.event_receiver);

        Ok(())
    }
}
