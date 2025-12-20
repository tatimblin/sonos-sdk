//! Adapter for converting generic callback notifications to Sonos-specific events.
//!
//! This module provides the adapter layer between the generic `callback-server` crate
//! and the Sonos-specific event processing. It maintains a mapping from subscription IDs
//! to Sonos context (speaker ID and service type) and converts generic `NotificationPayload`
//! events into `RawEvent` instances with Sonos-specific context.
//!
//! # Unified Event Stream Processing
//!
//! The CallbackAdapter is a key component in the unified event stream processing pattern:
//! 
//! 1. **Generic Input**: Receives `NotificationPayload` from the unified callback server
//! 2. **Context Enrichment**: Adds Sonos-specific context (speaker ID, service type)
//! 3. **Unified Output**: Produces `RawEvent` instances for the event stream processor
//!
//! This design allows the callback server to remain generic while enabling Sonos-specific
//! event processing downstream.

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
/// # Unified Event Stream Processing
///
/// This adapter is essential for the unified event stream processing pattern:
/// - **Single Entry Point**: All UPnP notifications flow through one callback server
/// - **Context Mapping**: Maps subscription IDs to speaker and service context
/// - **Event Enrichment**: Converts generic notifications to Sonos-specific events
/// - **Stream Integration**: Feeds the unified event stream processor
///
/// The adapter task will automatically terminate when the notification channel is closed,
/// which happens when the callback server shuts down.
pub struct CallbackAdapter {
    /// Map of subscription ID to (speaker_id, service_type)
    subscription_map: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>>,
}

impl CallbackAdapter {
    /// Create and start a new callback adapter for unified event stream processing.
    ///
    /// This creates the adapter and spawns a background task that converts generic
    /// notifications into Sonos-specific raw events. The adapter is a critical
    /// component in the unified event stream processing architecture.
    ///
    /// # Unified Event Stream Processing Flow
    ///
    /// 1. **Unified Callback Server** receives HTTP notifications from all speakers/services
    /// 2. **Event Router** routes notifications by subscription ID to this adapter
    /// 3. **Callback Adapter** (this component) enriches with Sonos-specific context
    /// 4. **Event Stream Processor** processes the enriched events through strategies
    ///
    /// # Arguments
    ///
    /// * `notification_rx` - Channel for receiving generic notifications from callback-server
    /// * `raw_event_sender` - Channel for sending Sonos-specific raw events to the broker
    ///
    /// # Returns
    ///
    /// Returns the callback adapter instance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let (notification_tx, notification_rx) = mpsc::unbounded_channel();
    /// let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();
    /// 
    /// let adapter = CallbackAdapter::new(notification_rx, raw_event_tx);
    /// // Adapter now processes notifications in the background
    /// ```
    pub fn new(
        mut notification_rx: mpsc::UnboundedReceiver<NotificationPayload>,
        raw_event_sender: mpsc::UnboundedSender<RawEvent>,
    ) -> Self {
        // Create the subscription map
        let subscription_map: Arc<RwLock<HashMap<String, (SpeakerId, ServiceType)>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Spawn adapter task to convert NotificationPayload to RawEvent
        // This is the core of the unified event stream processing:
        // - Receives generic notifications from the unified callback server
        // - Looks up Sonos-specific context (speaker ID, service type)
        // - Produces enriched RawEvent instances for the event stream processor
        // The task will automatically terminate when notification_rx is closed
        {
            let subscription_map = subscription_map.clone();

            tokio::spawn(async move {
                println!("🔄 CallbackAdapter: Starting unified event processing task");
                
                while let Some(notification) = notification_rx.recv().await {
                    println!("📨 CallbackAdapter: Processing notification for subscription {}", 
                        notification.subscription_id);
                    
                    // Validate notification data before processing
                    if notification.subscription_id.is_empty() {
                        eprintln!("❌ CallbackAdapter: Received notification with empty subscription ID, dropping");
                        continue;
                    }
                    
                    if notification.event_xml.is_empty() {
                        eprintln!("❌ CallbackAdapter: Received notification with empty XML for subscription {}, dropping", 
                            notification.subscription_id);
                        continue;
                    }
                    
                    // Look up Sonos context for this subscription
                    let subs = subscription_map.read().await;
                    if let Some((speaker_id, service_type)) =
                        subs.get(&notification.subscription_id)
                    {
                        // Create RawEvent with Sonos-specific context for unified processing
                        let raw_event = RawEvent {
                            subscription_id: notification.subscription_id.clone(),
                            speaker_id: speaker_id.clone(),
                            service_type: *service_type,
                            event_xml: notification.event_xml,
                        };

                        println!("✅ CallbackAdapter: Enriched event for speaker {} service {:?}", 
                            speaker_id.as_str(), service_type);

                        // Send to unified event stream processor (ignore errors if receiver is dropped)
                        if let Err(_) = raw_event_sender.send(raw_event) {
                            println!("⚠️  CallbackAdapter: Event stream processor channel closed");
                            break;
                        }
                    } else {
                        eprintln!("❌ CallbackAdapter: Unknown subscription {}, dropping event. Registered subscriptions: {}", 
                            notification.subscription_id,
                            subs.len());
                    }
                    // If subscription not found, drop the event silently
                }
                
                println!("🔄 CallbackAdapter: Unified event processing task terminated");
            });
        }

        Self { subscription_map }
    }

    /// Register a subscription for unified event routing with Sonos-specific context.
    ///
    /// This stores the Sonos-specific context (speaker ID and service type) for
    /// the given subscription ID, allowing future events to be enriched with
    /// this information as part of the unified event stream processing.
    ///
    /// # Unified Event Stream Processing
    ///
    /// This registration is essential for the unified pattern because:
    /// - The callback server receives generic HTTP notifications
    /// - This mapping provides the Sonos-specific context needed
    /// - Events can be properly routed to the correct service strategies
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The UPnP subscription ID
    /// * `speaker_id` - The ID of the speaker this subscription is for
    /// * `service_type` - The type of service being subscribed to
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// adapter.register_subscription(
    ///     "uuid:sub-123".to_string(),
    ///     SpeakerId::new("RINCON_123"),
    ///     ServiceType::AVTransport,
    /// ).await;
    /// ```
    pub async fn register_subscription(
        &self,
        subscription_id: String,
        speaker_id: SpeakerId,
        service_type: ServiceType,
    ) {
        let mut subs = self.subscription_map.write().await;
        subs.insert(subscription_id.clone(), (speaker_id.clone(), service_type));
        println!("📝 CallbackAdapter: Registered subscription {} for speaker {} service {:?}", 
            subscription_id, speaker_id.as_str(), service_type);
    }

    /// Unregister a subscription from the unified event stream.
    ///
    /// This removes the Sonos-specific context for the given subscription ID,
    /// ensuring that future events for this subscription will be dropped.
    ///
    /// # Arguments
    ///
    /// * `subscription_id` - The subscription ID to unregister
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// adapter.unregister_subscription("uuid:sub-123").await;
    /// ```
    pub async fn unregister_subscription(&self, subscription_id: &str) {
        let mut subs = self.subscription_map.write().await;
        if subs.remove(subscription_id).is_some() {
            println!("📝 CallbackAdapter: Unregistered subscription {}", subscription_id);
        }
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
    async fn test_adapter_validates_empty_subscription_id() {
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        let (raw_event_tx, mut raw_event_rx) = mpsc::unbounded_channel();

        let _adapter = CallbackAdapter::new(notification_rx, raw_event_tx);

        // Send notification with empty subscription ID
        let notification = NotificationPayload {
            subscription_id: "".to_string(),
            event_xml: "<event>test</event>".to_string(),
        };
        notification_tx.send(notification).unwrap();

        // Close notification channel to end adapter task
        drop(notification_tx);

        // Should not receive any raw events
        assert!(raw_event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_adapter_validates_empty_xml() {
        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        let (raw_event_tx, mut raw_event_rx) = mpsc::unbounded_channel();

        let adapter = CallbackAdapter::new(notification_rx, raw_event_tx);

        let speaker_id = SpeakerId::new("RINCON_123");
        let service_type = ServiceType::AVTransport;
        let subscription_id = "uuid:sub-123".to_string();

        // Register subscription
        adapter
            .register_subscription(subscription_id.clone(), speaker_id, service_type)
            .await;

        // Send notification with empty XML
        let notification = NotificationPayload {
            subscription_id: subscription_id.clone(),
            event_xml: "".to_string(),
        };
        notification_tx.send(notification).unwrap();

        // Close notification channel to end adapter task
        drop(notification_tx);

        // Should not receive any raw events due to empty XML validation
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