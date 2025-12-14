//! Adapter for converting generic callback notifications to Sonos-specific events.
//!
//! This module provides the adapter layer between the generic `callback-server` crate
//! and the Sonos-specific event processing. It maintains a mapping from subscription IDs
//! to Sonos context (speaker ID and service type) and converts generic `NotificationPayload`
//! events into `RawEvent` instances with Sonos-specific context.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use callback_server::NotificationPayload;
use crate::types::{RawEvent, ServiceType, SpeakerId};

/// Adapter that converts generic callback notifications to Sonos-specific raw events.
///
/// The adapter maintains a mapping from subscription IDs to Sonos context and spawns
/// a background task that listens for generic notifications and converts them to
/// Sonos-specific raw events.
///
/// The adapter task will automatically terminate when the notification channel is closed,
/// which happens when the callback server shuts down.
pub struct CallbackAdapter {
    /// Map of subscription ID to (speaker_id, service_type)
    subscription_map: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>>,
}

impl CallbackAdapter {
    /// Create and start a new callback adapter.
    ///
    /// This creates the adapter and spawns a background task that converts generic
    /// notifications into Sonos-specific raw events.
    ///
    /// # Arguments
    ///
    /// * `notification_rx` - Channel for receiving generic notifications from callback-server
    /// * `raw_event_sender` - Channel for sending Sonos-specific raw events to the broker
    ///
    /// # Returns
    ///
    /// Returns the callback adapter instance.
    pub fn new(
        mut notification_rx: mpsc::UnboundedReceiver<NotificationPayload>,
        raw_event_sender: mpsc::UnboundedSender<RawEvent>,
    ) -> Self {
        // Create the subscription map
        let subscription_map: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Spawn adapter task to convert NotificationPayload to RawEvent
        // The task will automatically terminate when notification_rx is closed
        {
            let subscription_map = subscription_map.clone();

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
            });
        }

        Self { subscription_map }
    }

    /// Register a subscription for event routing with Sonos-specific context.
    ///
    /// This stores the Sonos-specific context (speaker ID and service type) for
    /// the given subscription ID, allowing future events to be enriched with
    /// this information.
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
        let mut subs = self.subscription_map.write().await;
        subs.insert(subscription_id, (speaker_id, service_type));
    }

    /// Unregister a subscription.
    ///
    /// This removes the Sonos-specific context for the given subscription ID.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The subscription ID to unregister
    pub async fn unregister_subscription(&self, subscription_id: &str) {
        let mut subs = self.subscription_map.write().await;
        subs.remove(subscription_id);
    }


}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_adapter_converts_notifications() {
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        let (raw_event_tx, mut raw_event_rx) = mpsc::unbounded_channel();

        let adapter = CallbackAdapter::new(notification_rx, raw_event_tx);

        let speaker_id = SpeakerId::new("RINCON_123");
        let service_type = ServiceType::AVTransport;
        let subscription_id = "uuid:sub-123".to_string();

        // Register subscription
        adapter
            .register_subscription(subscription_id.clone(), speaker_id.clone(), service_type)
            .await;

        // Send notification
        let notification = NotificationPayload {
            subscription_id: subscription_id.clone(),
            event_xml: "<event>test</event>".to_string(),
        };
        notification_tx.send(notification).unwrap();

        // Receive raw event
        let raw_event = raw_event_rx.recv().await.unwrap();
        assert_eq!(raw_event.subscription_id, subscription_id);
        assert_eq!(raw_event.speaker_id, speaker_id);
        assert_eq!(raw_event.service_type, service_type);
        assert_eq!(raw_event.event_xml, "<event>test</event>");

        // Close notification channel to signal adapter task to complete
        drop(notification_tx);
    }

    #[tokio::test]
    async fn test_adapter_ignores_unknown_subscriptions() {
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        let (raw_event_tx, mut raw_event_rx) = mpsc::unbounded_channel();

        let _adapter = CallbackAdapter::new(notification_rx, raw_event_tx);

        // Send notification for unknown subscription
        let notification = NotificationPayload {
            subscription_id: "unknown-sub".to_string(),
            event_xml: "<event>test</event>".to_string(),
        };
        notification_tx.send(notification).unwrap();

        // Close notification channel to end adapter task
        drop(notification_tx);

        // Should not receive any raw events
        assert!(raw_event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_adapter_unregister_subscription() {
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        let (raw_event_tx, mut raw_event_rx) = mpsc::unbounded_channel();

        let adapter = CallbackAdapter::new(notification_rx, raw_event_tx);

        let speaker_id = SpeakerId::new("RINCON_123");
        let service_type = ServiceType::AVTransport;
        let subscription_id = "uuid:sub-123".to_string();

        // Register and then unregister subscription
        adapter
            .register_subscription(subscription_id.clone(), speaker_id, service_type)
            .await;
        adapter.unregister_subscription(&subscription_id).await;

        // Send notification
        let notification = NotificationPayload {
            subscription_id: subscription_id.clone(),
            event_xml: "<event>test</event>".to_string(),
        };
        notification_tx.send(notification).unwrap();

        // Close notification channel to end adapter task
        drop(notification_tx);

        // Should not receive any raw events
        assert!(raw_event_rx.try_recv().is_err());
    }
}