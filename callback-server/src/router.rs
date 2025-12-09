//! Event routing for HTTP callback notifications.
//!
//! This module provides the `EventRouter` which maintains a set of active
//! subscription IDs and routes incoming UPnP event notifications to a channel.

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Generic notification payload for UPnP event notifications.
///
/// This represents an unparsed UPnP event notification that has been received
/// via HTTP callback. It contains only the subscription ID and raw XML body,
/// with no device-specific context.
#[derive(Debug, Clone)]
pub struct NotificationPayload {
    /// The subscription ID from the UPnP SID header
    pub subscription_id: String,
    /// The raw XML event body
    pub event_xml: String,
}

/// Routes events from HTTP callbacks to a channel.
///
/// The `EventRouter` maintains a set of active subscription IDs. When an event
/// is received via HTTP callback, the router checks if the subscription is
/// registered and sends the notification payload to the configured channel.
#[derive(Clone)]
pub struct EventRouter {
    /// Set of active subscription IDs
    subscriptions: Arc<RwLock<HashSet<String>>>,
    /// Channel for sending notification payloads
    event_sender: mpsc::UnboundedSender<NotificationPayload>,
}

impl EventRouter {
    /// Create a new event router.
    ///
    /// # Arguments
    ///
    /// * `event_sender` - Channel for sending notification payloads
    ///
    /// # Example
    ///
    /// ```
    /// use tokio::sync::mpsc;
    /// use callback_server::router::{EventRouter, NotificationPayload};
    ///
    /// let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// let router = EventRouter::new(tx);
    /// ```
    pub fn new(event_sender: mpsc::UnboundedSender<NotificationPayload>) -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashSet::new())),
            event_sender,
        }
    }

    /// Register a subscription ID for event routing.
    ///
    /// This adds the subscription ID to the set of active subscriptions,
    /// allowing incoming events for this subscription to be routed.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The UPnP subscription ID to register
    ///
    /// # Example
    ///
    /// ```
    /// # use tokio::sync::mpsc;
    /// # use callback_server::router::{EventRouter, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let router = EventRouter::new(tx);
    /// router.register("uuid:subscription-123".to_string()).await;
    /// # }
    /// ```
    pub async fn register(&self, subscription_id: String) {
        let mut subs = self.subscriptions.write().await;
        subs.insert(subscription_id);
    }

    /// Unregister a subscription ID.
    ///
    /// Removes the subscription ID from the set of active subscriptions,
    /// preventing future events for this subscription from being routed.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The subscription ID to unregister
    ///
    /// # Example
    ///
    /// ```
    /// # use tokio::sync::mpsc;
    /// # use callback_server::router::{EventRouter, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, _rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let router = EventRouter::new(tx);
    /// # router.register("uuid:subscription-123".to_string()).await;
    /// router.unregister("uuid:subscription-123").await;
    /// # }
    /// ```
    pub async fn unregister(&self, subscription_id: &str) {
        let mut subs = self.subscriptions.write().await;
        subs.remove(subscription_id);
    }

    /// Route an incoming event to the channel.
    ///
    /// Checks if the subscription ID is registered and sends a `NotificationPayload`
    /// to the configured channel. If the subscription ID is not found, the event
    /// is dropped and `false` is returned.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The subscription ID from the event notification
    /// * `event_xml` - The raw XML event body
    ///
    /// # Returns
    ///
    /// Returns `true` if the event was successfully routed, `false` if the
    /// subscription ID was not registered.
    ///
    /// # Example
    ///
    /// ```
    /// # use tokio::sync::mpsc;
    /// # use callback_server::router::{EventRouter, NotificationPayload};
    /// # #[tokio::main]
    /// # async fn main() {
    /// # let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
    /// # let router = EventRouter::new(tx);
    /// # router.register("uuid:subscription-123".to_string()).await;
    /// let routed = router.route_event(
    ///     "uuid:subscription-123".to_string(),
    ///     "<event>data</event>".to_string()
    /// ).await;
    /// assert!(routed);
    /// # }
    /// ```
    pub async fn route_event(&self, subscription_id: String, event_xml: String) -> bool {
        let subs = self.subscriptions.read().await;
        
        if subs.contains(&subscription_id) {
            let payload = NotificationPayload {
                subscription_id,
                event_xml,
            };
            
            // Send payload to channel (ignore errors if receiver is dropped)
            let _ = self.event_sender.send(payload);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_router_register_and_route() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "test-sub-123".to_string();

        // Register subscription
        router.register(sub_id.clone()).await;

        // Route an event
        let event_xml = "<event>test</event>".to_string();
        let routed = router.route_event(sub_id.clone(), event_xml.clone()).await;
        assert!(routed);

        // Verify payload was sent
        let payload = rx.recv().await.unwrap();
        assert_eq!(payload.subscription_id, sub_id);
        assert_eq!(payload.event_xml, event_xml);
    }

    #[tokio::test]
    async fn test_event_router_unregister() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "test-sub-123".to_string();

        // Register and then unregister
        router.register(sub_id.clone()).await;
        router.unregister(&sub_id).await;

        // Try to route an event - should fail
        let event_xml = "<event>test</event>".to_string();
        let routed = router.route_event(sub_id, event_xml).await;
        assert!(!routed);

        // No payload should be received
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_event_router_unknown_subscription() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        // Try to route event for unknown subscription
        let routed = router
            .route_event("unknown-sub".to_string(), "<event>test</event>".to_string())
            .await;
        assert!(!routed);

        // No payload should be received
        assert!(rx.try_recv().is_err());
    }
}
