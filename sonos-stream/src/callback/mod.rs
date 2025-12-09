//! HTTP callback server for receiving UPnP event notifications.
//!
//! This module provides the infrastructure for receiving UPnP event notifications
//! from Sonos devices via HTTP callbacks. It wraps the generic `callback-server`
//! crate and adds Sonos-specific context (speaker ID and service type) to events.
//!
//! # Architecture
//!
//! This module acts as an adapter layer between the generic `callback-server` crate
//! and the Sonos-specific event processing:
//!
//! - **CallbackServer**: Wraps the generic callback server and maintains a mapping
//!   from subscription IDs to Sonos context (speaker ID and service type)
//! - **RawEvent**: Sonos-specific event structure that includes speaker and service
//!   information in addition to the raw XML payload
//!
//! ## Adapter Pattern
//!
//! The adapter pattern is implemented as follows:
//!
//! 1. **Generic Layer** (`callback-server` crate):
//!    - Receives HTTP POST requests with UPnP NOTIFY events
//!    - Validates UPnP headers (SID, NT, NTS)
//!    - Extracts subscription ID and raw XML body
//!    - Sends `NotificationPayload` (generic) to a channel
//!
//! 2. **Adapter Task** (this module):
//!    - Receives `NotificationPayload` from callback-server
//!    - Looks up Sonos context in `subscription_map`
//!    - Creates `RawEvent` with speaker ID and service type
//!    - Sends `RawEvent` to the event broker
//!
//! 3. **Subscription Management**:
//!    - `register_subscription()` updates both the generic router and the Sonos-specific map
//!    - `unregister_subscription()` removes from both locations
//!    - Unknown subscriptions are silently dropped by the adapter
//!
//! This design keeps the callback-server generic and reusable while allowing
//! sonos-stream to add domain-specific context without modifying the HTTP layer.
//!
//! ## Data Flow
//!
//! ```text
//! UPnP Device
//!     │
//!     │ HTTP POST /notify/{sid}
//!     ▼
//! callback-server (generic)
//!     │
//!     │ NotificationPayload { subscription_id, event_xml }
//!     ▼
//! Adapter Task
//!     │
//!     │ Lookup: subscription_id -> (SpeakerId, ServiceType)
//!     ▼
//! RawEvent { subscription_id, speaker_id, service_type, event_xml }
//!     │
//!     ▼
//! Event Broker
//! ```
//!
//! # Usage
//!
//! The callback server is typically created by the `EventBrokerBuilder` and is
//! not intended to be used directly by end users. It automatically:
//!
//! 1. Finds an available port in the configured range (default 3400-3500)
//! 2. Detects the local IP address for callback URLs
//! 3. Starts an HTTP server to receive UPnP NOTIFY requests
//! 4. Routes events to the event processor with Sonos-specific context
//!
//! # Example
//!
//! ```no_run
//! use tokio::sync::mpsc;
//! use sonos_stream::{CallbackServer, RawEvent};
//! use sonos_stream::types::{SpeakerId, ServiceType};
//!
//! # async fn example() -> Result<(), String> {
//! // Create channel for receiving Sonos-specific events
//! let (event_tx, mut event_rx) = mpsc::unbounded_channel();
//! 
//! // Create callback server (wraps generic callback-server)
//! let server = CallbackServer::new((3400, 3500), event_tx).await?;
//!
//! println!("Callback server running at: {}", server.base_url());
//!
//! // Register a subscription with Sonos context
//! let speaker_id = SpeakerId::from("RINCON_000XXX1400");
//! server.register_subscription(
//!     "uuid:subscription-123".to_string(),
//!     speaker_id,
//!     ServiceType::AVTransport,
//! ).await;
//!
//! // Process Sonos-specific events
//! tokio::spawn(async move {
//!     while let Some(event) = event_rx.recv().await {
//!         println!("Event from speaker {} service {:?}", 
//!                  event.speaker_id, event.service_type);
//!     }
//! });
//!
//! // Cleanup
//! server.shutdown().await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::types::{ServiceType, SpeakerId};

/// Raw event received from the callback server with Sonos-specific context.
///
/// This represents an unparsed UPnP event notification that has been received
/// via HTTP callback and enriched with Sonos-specific information (speaker ID
/// and service type). It needs to be processed by the event processor.
#[derive(Debug, Clone)]
pub struct RawEvent {
    /// The subscription ID this event is for
    pub subscription_id: String,
    /// The speaker ID
    pub speaker_id: SpeakerId,
    /// The service type
    pub service_type: ServiceType,
    /// The raw XML event body
    pub event_xml: String,
}

/// HTTP callback server for receiving UPnP event notifications from Sonos devices.
///
/// This wraps the generic `callback-server` and adds Sonos-specific context to
/// incoming events. It maintains a mapping from subscription IDs to speaker and
/// service information, allowing events to be enriched before being sent to the
/// event processor.
pub struct CallbackServer {
    /// The inner generic callback server
    inner: callback_server::CallbackServer,
    /// Map of subscription ID to (speaker_id, service_type)
    subscription_map: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>>,
    /// Channel for sending Sonos-specific raw events (used in adapter task)
    #[allow(dead_code)]
    raw_event_sender: mpsc::UnboundedSender<RawEvent>,
    /// Handle for the adapter task
    adapter_task: Option<tokio::task::JoinHandle<()>>,
}

impl CallbackServer {
    /// Create and start a new callback server.
    ///
    /// This creates the underlying generic callback server and spawns an adapter
    /// task that converts generic notifications into Sonos-specific raw events.
    ///
    /// # Arguments
    ///
    /// * `port_range` - Range of ports to try binding to (start, end)
    /// * `raw_event_sender` - Channel for sending Sonos-specific raw events to the broker
    ///
    /// # Returns
    ///
    /// Returns the callback server instance or an error if no port could be bound.
    pub async fn new(
        port_range: (u16, u16),
        raw_event_sender: mpsc::UnboundedSender<RawEvent>,
    ) -> Result<Self, String> {
        // Create a channel for receiving generic notifications from callback-server
        let (notification_tx, mut notification_rx) =
            mpsc::unbounded_channel::<callback_server::NotificationPayload>();

        // Create the inner generic callback server
        let inner = callback_server::CallbackServer::new(port_range, notification_tx).await?;

        // Create the subscription map
        let subscription_map: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Spawn adapter task to convert NotificationPayload to RawEvent
        let adapter_task = {
            let subscription_map = subscription_map.clone();
            let raw_event_sender = raw_event_sender.clone();

            tokio::spawn(async move {
                while let Some(notification) = notification_rx.recv().await {
                    // Look up Sonos context for this subscription
                    let subs = subscription_map.read().await;
                    if let Some((speaker_id, service_type)) =
                        subs.get(&notification.subscription_id)
                    {
                        // Create RawEvent with Sonos-specific context
                        let raw_event = RawEvent {
                            subscription_id: notification.subscription_id,
                            speaker_id: speaker_id.clone(),
                            service_type: *service_type,
                            event_xml: notification.event_xml,
                        };

                        // Send to broker (ignore errors if receiver is dropped)
                        let _ = raw_event_sender.send(raw_event);
                    }
                    // If subscription not found, drop the event silently
                }
            })
        };

        Ok(Self {
            inner,
            subscription_map,
            raw_event_sender,
            adapter_task: Some(adapter_task),
        })
    }

    /// Get the base URL for callback registration.
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Get the port the server is bound to.
    pub fn port(&self) -> u16 {
        self.inner.port()
    }

    /// Register a subscription for event routing with Sonos-specific context.
    ///
    /// This registers the subscription in both the inner router (for HTTP routing)
    /// and the subscription map (for adding Sonos context to events).
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The UPnP subscription ID
    /// * `speaker_id` - The ID of the speaker this subscription is for
    /// * `service_type` - The type of service being subscribed to
    pub async fn register_subscription(
        &self,
        subscription_id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
    ) {
        // Register in the inner router for HTTP routing
        self.inner.router().register(subscription_id.clone()).await;

        // Store Sonos-specific context in subscription map
        let mut subs = self.subscription_map.write().await;
        subs.insert(subscription_id, (speaker_id, service_type));
    }

    /// Unregister a subscription.
    ///
    /// This removes the subscription from both the inner router and the
    /// subscription map.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The subscription ID to unregister
    pub async fn unregister_subscription(&self, subscription_id: &str) {
        // Unregister from the inner router
        self.inner.router().unregister(subscription_id).await;

        // Remove from subscription map
        let mut subs = self.subscription_map.write().await;
        subs.remove(subscription_id);
    }

    /// Shutdown the callback server gracefully.
    ///
    /// This shuts down the inner server and waits for the adapter task to complete.
    pub async fn shutdown(mut self) -> Result<(), String> {
        // Shutdown the inner server (this will close the notification channel)
        self.inner.shutdown().await?;

        // Wait for adapter task to complete
        if let Some(handle) = self.adapter_task.take() {
            let _ = handle.await;
        }

        Ok(())
    }
}

// Re-export EventRouter from callback-server for convenience
pub use callback_server::EventRouter;
