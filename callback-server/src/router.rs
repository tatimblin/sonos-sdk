//! Event routing for HTTP callback notifications.
//!
//! This module provides the `EventRouter` which maintains a set of active
//! subscription IDs and routes incoming UPnP event notifications to a channel.
//! Events for not-yet-registered SIDs are buffered and replayed when
//! registration completes, preventing the race between SUBSCRIBE response
//! and initial NOTIFY delivery.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::debug;

/// Maximum time a buffered event is kept before being discarded.
/// The race window is typically microseconds; 5 seconds handles any
/// pathological scheduling delay.
const BUFFER_TTL: Duration = Duration::from_secs(5);

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

/// Internal state protected by a single lock to eliminate TOCTOU gaps.
struct RouterState {
    subscriptions: HashSet<String>,
    /// Flat buffer of (subscription_id, event_xml, buffered_at).
    /// Expected size: 0-5 entries. Only populated during the microsecond
    /// race window between SUBSCRIBE response and register() call.
    pending: Vec<(String, String, Instant)>,
}

/// Routes events from HTTP callbacks to a channel.
///
/// The `EventRouter` maintains a set of active subscription IDs. When an event
/// is received via HTTP callback, the router checks if the subscription is
/// registered and sends the notification payload to the configured channel.
///
/// Events for unregistered SIDs are buffered briefly and replayed when
/// `register()` is called, preventing the race between SUBSCRIBE response
/// and initial UPnP NOTIFY delivery.
#[derive(Clone)]
pub struct EventRouter {
    state: Arc<RwLock<RouterState>>,
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
            state: Arc::new(RwLock::new(RouterState {
                subscriptions: HashSet::new(),
                pending: Vec::new(),
            })),
            event_sender,
        }
    }

    /// Register a subscription ID for event routing.
    ///
    /// Adds the SID to the active set and replays any buffered events that
    /// arrived before registration (the SUBSCRIBE/NOTIFY race window).
    /// Also cleans up stale buffer entries older than `BUFFER_TTL`.
    pub async fn register(&self, subscription_id: String) {
        let mut state = self.state.write().await;
        state.subscriptions.insert(subscription_id.clone());

        // Replay buffered events for this SID and remove stale entries.
        let now = Instant::now();
        let mut i = 0;
        while i < state.pending.len() {
            let (ref sid, _, buffered_at) = state.pending[i];
            if sid == &subscription_id {
                let (_, xml, _) = state.pending.swap_remove(i);
                debug!(sid = %subscription_id, "Replayed buffered event");
                let payload = NotificationPayload {
                    subscription_id: subscription_id.clone(),
                    event_xml: xml,
                };
                let _ = self.event_sender.send(payload);
                // Don't increment i — swap_remove moved the last element here
            } else if now.duration_since(buffered_at) > BUFFER_TTL {
                state.pending.swap_remove(i);
                // Don't increment i
            } else {
                i += 1;
            }
        }
    }

    /// Unregister a subscription ID.
    ///
    /// Removes the SID from the active set and drains any buffered events
    /// for it, preventing stale replays on future re-registration.
    pub async fn unregister(&self, subscription_id: &str) {
        let mut state = self.state.write().await;
        state.subscriptions.remove(subscription_id);
        state.pending.retain(|(sid, _, _)| sid != subscription_id);
    }

    /// Route an incoming event to the unified event stream.
    ///
    /// If the subscription is registered, the event is sent immediately.
    /// If not, the event is buffered for replay when `register()` is called.
    /// The caller should always return HTTP 200 OK — buffered events are
    /// accepted for processing, not rejected.
    pub async fn route_event(&self, subscription_id: String, event_xml: String) {
        let mut state = self.state.write().await;
        if state.subscriptions.contains(&subscription_id) {
            let payload = NotificationPayload {
                subscription_id,
                event_xml,
            };
            let _ = self.event_sender.send(payload);
        } else {
            debug!(sid = %subscription_id, "Buffered event for pending SID");
            state
                .pending
                .push((subscription_id, event_xml, Instant::now()));
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
        router.route_event(sub_id.clone(), event_xml.clone()).await;

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

        // Route an event — should be buffered (not delivered), since SID is unregistered
        let event_xml = "<event>test</event>".to_string();
        router.route_event(sub_id, event_xml).await;

        // No immediate payload — event was buffered, not routed
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_event_router_unknown_subscription_buffers() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        // Route event for unknown subscription — should be buffered, not dropped
        router
            .route_event("unknown-sub".to_string(), "<event>test</event>".to_string())
            .await;

        // No immediate payload — event was buffered
        assert!(rx.try_recv().is_err());
    }

    /// Proves the registration race condition: an event arriving before register()
    /// should be buffered and replayed when register() is called.
    #[tokio::test]
    async fn test_event_buffered_and_replayed_on_late_register() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "uuid:late-register".to_string();
        let event_xml = "<e:propertyset><CurrentPlayMode>NORMAL</CurrentPlayMode></e:propertyset>"
            .to_string();

        // 1. Event arrives BEFORE register (the race condition)
        router
            .route_event(sub_id.clone(), event_xml.clone())
            .await;

        // 2. Register happens moments later
        router.register(sub_id.clone()).await;

        // 3. The buffered event should have been replayed on register
        let payload = rx.try_recv().expect("expected replayed event");
        assert_eq!(payload.subscription_id, sub_id);
        assert_eq!(payload.event_xml, event_xml);
    }

    /// Stale buffered events (older than BUFFER_TTL) are cleaned up during register().
    #[tokio::test]
    async fn test_stale_buffer_entries_cleaned_on_register() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        // Manually insert a stale entry by writing to state directly
        {
            let mut state = router.state.write().await;
            state.pending.push((
                "uuid:stale-sid".to_string(),
                "<event>stale</event>".to_string(),
                Instant::now() - Duration::from_secs(10), // 10s ago, well past TTL
            ));
        }

        // Register a different SID — should clean up the stale entry
        router.register("uuid:fresh-sid".to_string()).await;

        // No events replayed (the stale entry was for a different SID and expired)
        assert!(rx.try_recv().is_err());

        // Verify the stale entry was cleaned up
        let state = router.state.read().await;
        assert!(state.pending.is_empty(), "stale entry should be cleaned up");
    }

    /// unregister() drains buffered events for the removed SID.
    #[tokio::test]
    async fn test_unregister_drains_buffer() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "uuid:drain-test".to_string();

        // Buffer an event
        router
            .route_event(sub_id.clone(), "<event>buffered</event>".to_string())
            .await;

        // Unregister — should drain the buffered event
        router.unregister(&sub_id).await;

        // Re-register — should NOT replay the drained event
        router.register(sub_id.clone()).await;

        // No events replayed (buffer was drained by unregister)
        assert!(rx.try_recv().is_err());
    }

    /// Multiple buffered events for the same SID are all replayed.
    #[tokio::test]
    async fn test_multiple_buffered_events_replayed() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "uuid:multi".to_string();

        // Buffer two events before registering
        router
            .route_event(sub_id.clone(), "<event>first</event>".to_string())
            .await;
        router
            .route_event(sub_id.clone(), "<event>second</event>".to_string())
            .await;

        // Register — both events should be replayed
        router.register(sub_id.clone()).await;

        let p1 = rx.try_recv().expect("expected first replayed event");
        assert!(p1.event_xml.contains("first"));

        let p2 = rx.try_recv().expect("expected second replayed event");
        assert!(p2.event_xml.contains("second"));

        // No more events
        assert!(rx.try_recv().is_err());
    }

    /// Buffered events for different SIDs don't interfere.
    #[tokio::test]
    async fn test_buffer_isolates_different_sids() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        // Buffer events for two different SIDs
        router
            .route_event("uuid:sid-a".to_string(), "<event>a</event>".to_string())
            .await;
        router
            .route_event("uuid:sid-b".to_string(), "<event>b</event>".to_string())
            .await;

        // Register only SID-A
        router.register("uuid:sid-a".to_string()).await;

        // Only SID-A's event should be replayed
        let p = rx.try_recv().expect("expected replayed event for sid-a");
        assert_eq!(p.subscription_id, "uuid:sid-a");
        assert!(p.event_xml.contains("a"));

        // SID-B's event is still in the buffer
        assert!(rx.try_recv().is_err());

        // Now register SID-B
        router.register("uuid:sid-b".to_string()).await;

        let p2 = rx.try_recv().expect("expected replayed event for sid-b");
        assert_eq!(p2.subscription_id, "uuid:sid-b");
        assert!(p2.event_xml.contains("b"));
    }
}
