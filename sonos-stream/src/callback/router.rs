//! Event routing for HTTP callback notifications.
//!
//! This module provides the `EventRouter` which maintains mappings between
//! subscription IDs and their associated speaker/service information, and
//! routes incoming UPnP event notifications to the event processor.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::types::{ServiceType, SpeakerId};

/// Raw event received from the callback server.
///
/// This represents an unparsed UPnP event notification that has been received
/// via HTTP callback and needs to be processed by the event processor.
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

/// Routes events from HTTP callbacks to the appropriate handlers.
///
/// The `EventRouter` maintains a mapping of subscription IDs to their associated
/// speaker and service type information. When an event is received via HTTP callback,
/// the router looks up the subscription information and creates a `RawEvent` that
/// is sent to the event processor for parsing.
#[derive(Clone)]
pub struct EventRouter {
    /// Map of subscription ID to (speaker_id, service_type)
    subscriptions: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>>,
    /// Channel for sending raw events to the broker
    event_sender: mpsc::UnboundedSender<RawEvent>,
}

impl EventRouter {
    /// Create a new event router.
    ///
    /// # Arguments
    ///
    /// * `event_sender` - Channel for sending raw events to the event processor
    pub fn new(event_sender: mpsc::UnboundedSender<RawEvent>) -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
        }
    }

    /// Register a subscription for event routing.
    ///
    /// This creates a mapping from the subscription ID to the speaker and service
    /// type, allowing incoming events to be properly routed.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The UPnP subscription ID
    /// * `speaker_id` - The ID of the speaker this subscription is for
    /// * `service_type` - The type of service being subscribed to
    pub async fn register(
        &self,
        subscription_id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
    ) {
        let mut subs = self.subscriptions.write().await;
        subs.insert(subscription_id, (speaker_id, service_type));
    }

    /// Unregister a subscription.
    ///
    /// Removes the subscription mapping, preventing future events for this
    /// subscription ID from being routed.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The subscription ID to unregister
    pub async fn unregister(&self, subscription_id: &str) {
        let mut subs = self.subscriptions.write().await;
        subs.remove(subscription_id);
    }

    /// Route an incoming event to the broker.
    ///
    /// Looks up the subscription information and creates a `RawEvent` that is
    /// sent to the event processor. If the subscription ID is not found, the
    /// event is dropped and `false` is returned.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The subscription ID from the event notification
    /// * `event_xml` - The raw XML event body
    ///
    /// # Returns
    ///
    /// Returns `true` if the event was successfully routed, `false` if the
    /// subscription ID was not found.
    pub async fn route_event(&self, subscription_id: String, event_xml: String) -> bool {
        let subs = self.subscriptions.read().await;
        
        if let Some((speaker_id, service_type)) = subs.get(&subscription_id) {
            let event = RawEvent {
                subscription_id,
                speaker_id: speaker_id.clone(),
                service_type: *service_type,
                event_xml,
            };
            
            // Send event to broker (ignore errors if receiver is dropped)
            let _ = self.event_sender.send(event);
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
        let speaker_id = SpeakerId::new("speaker1");
        let service_type = ServiceType::AVTransport;

        // Register subscription
        router
            .register(sub_id.clone(), speaker_id.clone(), service_type)
            .await;

        // Route an event
        let event_xml = "<event>test</event>".to_string();
        let routed = router.route_event(sub_id.clone(), event_xml.clone()).await;
        assert!(routed);

        // Verify event was sent
        let event = rx.recv().await.unwrap();
        assert_eq!(event.subscription_id, sub_id);
        assert_eq!(event.speaker_id, speaker_id);
        assert_eq!(event.service_type, service_type);
        assert_eq!(event.event_xml, event_xml);
    }

    #[tokio::test]
    async fn test_event_router_unregister() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "test-sub-123".to_string();
        let speaker_id = SpeakerId::new("speaker1");
        let service_type = ServiceType::AVTransport;

        // Register and then unregister
        router
            .register(sub_id.clone(), speaker_id.clone(), service_type)
            .await;
        router.unregister(&sub_id).await;

        // Try to route an event - should fail
        let event_xml = "<event>test</event>".to_string();
        let routed = router.route_event(sub_id, event_xml).await;
        assert!(!routed);

        // No event should be received
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

        // No event should be received
        assert!(rx.try_recv().is_err());
    }
}
