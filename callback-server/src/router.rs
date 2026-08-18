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

/// Hard cap on buffered pending events.
///
/// The buffer exists only to bridge the microsecond race between a SUBSCRIBE
/// response and the `register()` call that follows it, so legitimate occupancy is
/// 0-5 entries. But `route_event` buffers events for *any* unrecognised SID, and
/// the endpoint is unauthenticated — a hostile or buggy sender spraying random
/// SIDs would grow this Vec without bound (each entry holds a full event body).
///
/// 256 is ~50x the legitimate high-water mark, so it cannot be reached by normal
/// operation even on a large household bringing every subscription up at once,
/// while bounding worst-case retention to 256 bodies. On overflow the oldest
/// entry is dropped: these are pending state snapshots, and the newest is both
/// the most current and the most likely to still have a `register()` coming.
const MAX_PENDING_EVENTS: usize = 256;

/// Generic notification payload for UPnP event notifications.
///
/// This represents an unparsed UPnP event notification that has been received
/// via HTTP callback. It contains only the subscription ID, raw XML body, and
/// when the notification arrived — no device-specific context.
#[derive(Debug, Clone)]
pub struct NotificationPayload {
    /// The subscription ID from the UPnP SID header
    pub subscription_id: String,
    /// The raw XML event body
    pub event_xml: String,
    /// When this notification reached the router, on the monotonic clock.
    ///
    /// This is the earliest instant at which the process can be said to have
    /// *observed* the values in `event_xml`, and downstream consumers order
    /// writes by it. It is deliberately `Instant` and not `SystemTime`: an
    /// ordering built on the wall clock would reverse itself whenever NTP
    /// stepped the clock backwards.
    ///
    /// For an event that arrived before its SID was registered, this is the
    /// original arrival instant, *not* the replay instant — a notification held
    /// in the pending buffer for seconds must not be treated as freshly
    /// observed when it is finally replayed.
    pub received_at: Instant,
}

/// Internal state protected by a single lock to eliminate TOCTOU gaps.
struct RouterState {
    subscriptions: HashSet<String>,
    /// Flat buffer of (subscription_id, event_xml, buffered_at).
    /// Expected size: 0-5 entries for well-behaved senders — only populated
    /// during the microsecond race window between SUBSCRIBE response and
    /// register(). Hard-capped at `MAX_PENDING_EVENTS` because the HTTP endpoint
    /// is unauthenticated and cannot rely on senders being well-behaved.
    pending: Vec<(String, String, Instant)>,
}

impl RouterState {
    /// Drop buffered entries older than `BUFFER_TTL`.
    fn sweep_stale(&mut self) {
        let now = Instant::now();
        self.pending
            .retain(|(_, _, buffered_at)| now.duration_since(*buffered_at) <= BUFFER_TTL);
    }

    /// Buffer an event, enforcing `MAX_PENDING_EVENTS` by dropping the oldest.
    ///
    /// `received_at` is the caller's arrival instant, carried through so that a
    /// replayed event is stamped with when it arrived rather than when it was
    /// replayed. It doubles as the TTL/eviction key, which is what it already
    /// was.
    fn push_pending(&mut self, subscription_id: String, event_xml: String, received_at: Instant) {
        self.sweep_stale();

        while self.pending.len() >= MAX_PENDING_EVENTS {
            // Evict by timestamp rather than position: swap_remove elsewhere
            // means index order does not track insertion order.
            let oldest = self
                .pending
                .iter()
                .enumerate()
                .min_by_key(|(_, (_, _, at))| *at)
                .map(|(i, _)| i);
            match oldest {
                Some(i) => {
                    let (sid, _, _) = self.pending.remove(i);
                    debug!(
                        sid = %sid,
                        cap = MAX_PENDING_EVENTS,
                        "Pending event buffer full; dropped oldest entry"
                    );
                }
                None => break,
            }
        }

        self.pending.push((subscription_id, event_xml, received_at));
    }
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
                let (_, xml, arrived_at) = state.pending.swap_remove(i);
                debug!(sid = %subscription_id, "Replayed buffered event");
                let payload = NotificationPayload {
                    subscription_id: subscription_id.clone(),
                    event_xml: xml,
                    // The buffer entry's instant *is* the arrival instant, so a
                    // replayed event keeps its original observation time rather
                    // than looking as though it just arrived.
                    received_at: arrived_at,
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
    ///
    /// Buffering sweeps entries older than `BUFFER_TTL` and enforces a hard cap
    /// of `MAX_PENDING_EVENTS`, dropping the oldest entry on overflow. Without
    /// this, unrecognised SIDs could grow the buffer without bound.
    pub async fn route_event(&self, subscription_id: String, event_xml: String) {
        // Taken before acquiring the lock: waiting on a contended write lock is
        // not part of the observation, and stamping afterwards would push the
        // event's apparent observation time later than it really was.
        let received_at = Instant::now();

        let mut state = self.state.write().await;
        if state.subscriptions.contains(&subscription_id) {
            let payload = NotificationPayload {
                subscription_id,
                event_xml,
                received_at,
            };
            let _ = self.event_sender.send(payload);
        } else {
            debug!(sid = %subscription_id, "Buffered event for pending SID");
            // Sweeps stale entries and enforces MAX_PENDING_EVENTS. Doing this
            // here (not only in register()) is what bounds the buffer: register()
            // is called on genuine new subscriptions only, so events for random
            // SIDs would otherwise never trigger cleanup.
            state.push_pending(subscription_id, event_xml, received_at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `Instant` the given duration in the past.
    ///
    /// `Instant::now() - d` can panic where the monotonic clock's zero point is
    /// near process start, so subtract fallibly and fail with a clear message
    /// instead of an opaque arithmetic panic.
    fn stale_instant(ago: Duration) -> Instant {
        Instant::now()
            .checked_sub(ago)
            .expect("clock has enough history to backdate a test instant")
    }

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
    async fn test_replayed_event_keeps_original_arrival_instant() {
        // An event that arrives before its SID is registered is buffered and
        // replayed later. Downstream orders writes by `received_at`, so the
        // replayed payload must carry when it *arrived*, not when it was
        // replayed — otherwise a notification held in the buffer would come out
        // looking freshly observed and could displace a newer local write.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        let sub_id = "test-sub-replay".to_string();

        // Arrives before registration, so it is buffered.
        let before = Instant::now();
        router
            .route_event(sub_id.clone(), "<event>buffered</event>".to_string())
            .await;
        assert!(rx.try_recv().is_err(), "precondition: event was buffered");

        // Time passes, then registration replays it.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let after_delay = Instant::now();
        router.register(sub_id.clone()).await;

        let payload = rx.try_recv().expect("buffered event should be replayed");
        assert_eq!(payload.subscription_id, sub_id);
        assert!(
            payload.received_at >= before && payload.received_at < after_delay,
            "replayed payload must keep its original arrival instant, not the replay instant"
        );
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
        let event_xml =
            "<e:propertyset><CurrentPlayMode>NORMAL</CurrentPlayMode></e:propertyset>".to_string();

        // 1. Event arrives BEFORE register (the race condition)
        router.route_event(sub_id.clone(), event_xml.clone()).await;

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
                stale_instant(Duration::from_secs(10)), // 10s ago, well past TTL
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

    /// The pending buffer is hard-capped so unknown-SID traffic cannot grow it
    /// without bound. Only `register()` used to sweep the buffer, so a sender
    /// spraying random SIDs would never trigger cleanup.
    #[tokio::test]
    async fn test_pending_buffer_is_bounded() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        for i in 0..(MAX_PENDING_EVENTS + 50) {
            router
                .route_event(
                    format!("uuid:unknown-{i}"),
                    "<event>spam</event>".to_string(),
                )
                .await;
        }

        let state = router.state.read().await;
        assert!(
            state.pending.len() <= MAX_PENDING_EVENTS,
            "pending buffer grew to {} entries (cap is {MAX_PENDING_EVENTS})",
            state.pending.len()
        );
    }

    /// Dropping the oldest entry keeps the newest events, which are the ones a
    /// pending `register()` is most likely to still care about.
    #[tokio::test]
    async fn test_pending_buffer_evicts_oldest_first() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        // First event is the oldest and should be evicted once the cap is hit.
        router
            .route_event(
                "uuid:oldest".to_string(),
                "<event>oldest</event>".to_string(),
            )
            .await;
        for i in 0..MAX_PENDING_EVENTS {
            router
                .route_event(
                    format!("uuid:filler-{i}"),
                    "<event>filler</event>".to_string(),
                )
                .await;
        }

        // The evicted SID has nothing to replay.
        router.register("uuid:oldest".to_string()).await;
        assert!(rx.try_recv().is_err(), "oldest entry should be evicted");

        // The most recent SID is still buffered and replays fine.
        let newest = format!("uuid:filler-{}", MAX_PENDING_EVENTS - 1);
        router.register(newest.clone()).await;
        let payload = rx
            .try_recv()
            .expect("newest entry should still be buffered");
        assert_eq!(payload.subscription_id, newest);
    }

    /// Stale entries are swept by `route_event`, not only by `register()`.
    #[tokio::test]
    async fn test_stale_entries_swept_on_route() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let router = EventRouter::new(tx);

        {
            let mut state = router.state.write().await;
            state.pending.push((
                "uuid:stale".to_string(),
                "<event>stale</event>".to_string(),
                stale_instant(Duration::from_secs(10)),
            ));
        }

        router
            .route_event("uuid:fresh".to_string(), "<event>fresh</event>".to_string())
            .await;

        let state = router.state.read().await;
        assert_eq!(state.pending.len(), 1, "stale entry should have been swept");
        assert_eq!(state.pending[0].0, "uuid:fresh");
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
