//! Sync-first change iterator for property updates
//!
//! Provides a blocking iterator over property change events.
//! Only emits events for properties that have been watched.
//!
//! Every [`ChangeIterator`] is an **independent subscriber**: each one receives
//! every event emitted after it was created, rather than competing with its
//! siblings for a shared queue.
//!
//! # Example
//!
//! ```rust,ignore
//! use sonos_state::StateManager;
//!
//! let manager = StateManager::new()?;
//! // ... add devices and watch properties ...
//!
//! // Blocking iteration — the new value rides along on the event
//! for event in manager.iter() {
//!     println!("{} changed on {}: {:?}", event.property_key(), event.speaker_id, event.change);
//! }
//!
//! // Non-blocking check
//! for event in manager.iter().try_iter() {
//!     println!("{} changed", event.property_key());
//! }
//!
//! // With timeout
//! if let Some(event) = manager.iter().recv_timeout(Duration::from_secs(1)) {
//!     println!("Got event: {:?}", event);
//! }
//! ```

use std::sync::{mpsc, Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;

use crate::state::ChangeEvent;

// ============================================================================
// EventFanout - one queue per subscriber
// ============================================================================

/// Broadcasts each [`ChangeEvent`] to every live subscriber.
///
/// # Why this exists
///
/// `iter()` previously handed every caller a clone of one `Arc<Mutex<Receiver>>`.
/// Two `for event in system.iter()` loops therefore *split* the stream — each
/// event went to whichever loop happened to win the mutex — even though `iter()`
/// returning an independent iterator is the universal Rust idiom for "iterate
/// the whole thing". Nothing errored and nothing was logged; a dashboard that
/// added a second event loop simply started missing roughly half its updates.
/// This type makes the API mean what it says.
///
/// # Why not `tokio::sync::broadcast`
///
/// This crate is deliberately sync-first: `recv()` blocks and no runtime is
/// assumed. `broadcast::Receiver::blocking_recv()` *panics* when called from a
/// thread already inside a Tokio runtime ("Cannot block the current thread from
/// within a runtime") — the same runtime-within-runtime failure already tracked
/// against `sonos-stream` in `docs/STATUS.md`. It also has no `recv_timeout`,
/// which [`ChangeIterator::recv_timeout`] needs, and its fixed ring buffer drops
/// events for slow consumers. A registry of plain `std::sync::mpsc` senders needs
/// no runtime, keeps `recv_timeout`, and drops nothing.
///
/// # Delivery guarantees
///
/// - **Nothing is dropped.** Each subscriber owns an *unbounded* `mpsc` queue, so
///   a slow consumer never loses an event and never blocks a fast one. There is
///   no lag or overflow state for a consumer to detect, because there is no lag.
///   The cost is the flip side of the same coin: a subscriber that never drains
///   grows its own queue without bound (see `docs/specs/sonos-state.md` 14.1).
/// - **Order is preserved per subscriber.** Every event is pushed to all
///   subscribers under one lock in emit order, so each subscriber observes the
///   same sequence the emitter produced. This is what keeps the observation-time
///   write ordering of spec 4.1a meaningful on the consumer side.
/// - **Subscribe before you emit.** A subscriber only receives events emitted
///   after `subscribe()`. There is no replay buffer; the store holds current
///   state for that job.
pub(crate) struct EventFanout {
    inner: Mutex<FanoutInner>,
}

struct FanoutInner {
    /// Monotonic subscriber id source. Never reused, so a late `unsubscribe`
    /// from an already-reaped subscriber cannot evict a newer one.
    next_id: u64,
    subscribers: Vec<(u64, mpsc::Sender<ChangeEvent>)>,
}

impl EventFanout {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(FanoutInner {
                next_id: 0,
                subscribers: Vec::new(),
            }),
        }
    }

    /// Register a new subscriber, returning its id and its private queue.
    pub(crate) fn subscribe(&self) -> (u64, mpsc::Receiver<ChangeEvent>) {
        let (tx, rx) = mpsc::channel();
        let mut inner = self.inner.lock();
        let id = inner.next_id;
        inner.next_id += 1;
        inner.subscribers.push((id, tx));
        tracing::trace!(
            "EventFanout: subscriber {} registered ({} total)",
            id,
            inner.subscribers.len()
        );
        (id, rx)
    }

    /// Drop a subscriber by id. Called by `ChangeIterator::drop` so a departing
    /// consumer is released immediately rather than lingering until the next
    /// event happens to fail a send.
    pub(crate) fn unsubscribe(&self, id: u64) {
        let mut inner = self.inner.lock();
        inner.subscribers.retain(|(sub_id, _)| *sub_id != id);
        tracing::trace!(
            "EventFanout: subscriber {} removed ({} remain)",
            id,
            inner.subscribers.len()
        );
    }

    /// Deliver `event` to every live subscriber, returning how many got it.
    ///
    /// Subscribers whose receiver has gone away are reaped here. That is a
    /// safety net independent of [`Self::unsubscribe`]: it covers a receiver
    /// dropped without its `ChangeIterator` wrapper, and it guarantees the
    /// registry cannot accumulate dead senders forever.
    pub(crate) fn send(&self, event: ChangeEvent) -> usize {
        let mut inner = self.inner.lock();
        let mut delivered = 0usize;
        inner
            .subscribers
            .retain(|(_, tx)| match tx.send(event.clone()) {
                Ok(()) => {
                    delivered += 1;
                    true
                }
                Err(_) => false,
            });
        delivered
    }

    /// Number of live subscribers. Used by tests to assert that a dropped
    /// consumer is actually released.
    #[cfg(test)]
    pub(crate) fn subscriber_count(&self) -> usize {
        self.inner.lock().subscribers.len()
    }
}

// ============================================================================
// ChangeIterator
// ============================================================================

/// Blocking iterator over property change events
///
/// Receives change events for watched properties via `std::sync::mpsc`.
/// All methods are synchronous - no async/await required.
///
/// Each `ChangeIterator` is an **independent subscriber**: two iterators taken
/// from the same `StateManager` both receive every event, rather than competing
/// for them. An iterator only sees events emitted *after* it was created, so
/// take it before the writes you care about.
pub struct ChangeIterator {
    /// Kept so `Drop` can deregister this subscriber.
    ///
    /// Deliberately `Weak`: a strong reference would keep the fan-out — and
    /// therefore this subscriber's own `Sender` — alive for exactly as long as
    /// the iterator, so `recv()` could never observe a closed channel and would
    /// block forever after the `StateManager` was dropped. `Weak` preserves the
    /// pre-fan-out behaviour, where dropping the manager dropped the sender and
    /// `recv()` returned `None`.
    fanout: Weak<EventFanout>,
    id: u64,
    /// This subscriber's private queue.
    ///
    /// Behind a `Mutex` only to keep `ChangeIterator: Sync`, so `&self` still
    /// crosses threads as it did before. Sharing one iterator between threads
    /// makes them compete for *this* iterator's events — which is now an
    /// explicit choice rather than the silent default.
    rx: Mutex<mpsc::Receiver<ChangeEvent>>,
}

impl ChangeIterator {
    /// Subscribe to a fan-out, creating an independent event queue.
    pub(crate) fn new(fanout: &Arc<EventFanout>) -> Self {
        let (id, rx) = fanout.subscribe();
        Self {
            fanout: Arc::downgrade(fanout),
            id,
            rx: Mutex::new(rx),
        }
    }

    /// Block until the next event is available
    ///
    /// Returns `None` if the channel is closed.
    pub fn recv(&self) -> Option<ChangeEvent> {
        let event = self.rx.lock().recv().ok();
        if let Some(ref e) = event {
            tracing::trace!(
                "ChangeIterator::recv yielded {} for {}",
                e.property_key(),
                e.speaker_id.as_str()
            );
        }
        event
    }

    /// Block until the next event or timeout expires
    ///
    /// Returns `None` if the timeout expires or channel is closed.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<ChangeEvent> {
        let event = self.rx.lock().recv_timeout(timeout).ok();
        if let Some(ref e) = event {
            tracing::trace!(
                "ChangeIterator::recv_timeout yielded {} for {}",
                e.property_key(),
                e.speaker_id.as_str()
            );
        }
        event
    }

    /// Try to receive an event without blocking
    ///
    /// Returns `None` if no event is currently available.
    pub fn try_recv(&self) -> Option<ChangeEvent> {
        let event = self.rx.lock().try_recv().ok();
        if let Some(ref e) = event {
            tracing::trace!(
                "ChangeIterator::try_recv yielded {} for {}",
                e.property_key(),
                e.speaker_id.as_str()
            );
        }
        event
    }

    /// Get a non-blocking iterator over currently available events
    ///
    /// Returns an iterator that yields all events currently in the queue
    /// without blocking. Useful for batch processing.
    ///
    /// This is a *view* of this iterator's own queue, not a new subscriber, so
    /// it consumes the same events `recv()` would. Call `iter()` again for an
    /// independent stream.
    pub fn try_iter(&self) -> TryIter<'_> {
        TryIter { inner: self }
    }

    /// Get a blocking iterator with timeout
    ///
    /// Returns an iterator that blocks for up to `timeout` on each call
    /// to `next()`. Stops when timeout expires without events.
    ///
    /// Like [`Self::try_iter`], a view of this iterator's queue rather than a
    /// new subscriber.
    pub fn timeout_iter(&self, timeout: Duration) -> TimeoutIter<'_> {
        TimeoutIter {
            inner: self,
            timeout,
        }
    }
}

impl Drop for ChangeIterator {
    fn drop(&mut self) {
        // No upgrade means the fan-out is already gone, so there is no registry
        // left to clean up.
        if let Some(fanout) = self.fanout.upgrade() {
            fanout.unsubscribe(self.id);
        }
    }
}

impl Iterator for ChangeIterator {
    type Item = ChangeEvent;

    /// Block until the next change event
    ///
    /// Returns `None` if the channel is closed.
    fn next(&mut self) -> Option<Self::Item> {
        self.recv()
    }
}

/// Non-blocking iterator over currently available events
pub struct TryIter<'a> {
    inner: &'a ChangeIterator,
}

impl<'a> Iterator for TryIter<'a> {
    type Item = ChangeEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.try_recv()
    }
}

/// Blocking iterator with timeout
pub struct TimeoutIter<'a> {
    inner: &'a ChangeIterator,
    timeout: Duration,
}

impl<'a> Iterator for TimeoutIter<'a> {
    type Item = ChangeEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.recv_timeout(self.timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::PropertyChange;
    use crate::model::SpeakerId;
    use crate::property::Volume;
    use crate::state::{ChangeSource, WriteStamp};
    use std::thread;
    use std::time::Instant;

    fn create_test_event() -> ChangeEvent {
        event_with_volume(42)
    }

    fn event_with_volume(v: u8) -> ChangeEvent {
        ChangeEvent::new(
            SpeakerId::new("test-speaker"),
            PropertyChange::Volume(Volume::new(v)),
            WriteStamp::now(ChangeSource::Event),
        )
    }

    fn volume_of(event: &ChangeEvent) -> u8 {
        match &event.change {
            PropertyChange::Volume(v) => v.value(),
            other => panic!("expected a Volume change, got {other:?}"),
        }
    }

    /// A fan-out plus one iterator, the shape every test below starts from.
    fn fanout_with_iter() -> (Arc<EventFanout>, ChangeIterator) {
        let fanout = Arc::new(EventFanout::new());
        let iter = ChangeIterator::new(&fanout);
        (fanout, iter)
    }

    #[test]
    fn test_try_recv_empty() {
        let (_fanout, iter) = fanout_with_iter();

        // Should return None when empty
        assert!(iter.try_recv().is_none());
    }

    #[test]
    fn test_try_recv_with_event() {
        let (fanout, iter) = fanout_with_iter();

        fanout.send(create_test_event());

        // Should receive the event
        let event = iter.try_recv().unwrap();
        assert_eq!(event.property_key(), "volume");
        assert_eq!(event.speaker_id.as_str(), "test-speaker");

        // Should return None now
        assert!(iter.try_recv().is_none());
    }

    #[test]
    fn test_recv_timeout() {
        let (_fanout, iter) = fanout_with_iter();

        // Should timeout when empty
        let start = Instant::now();
        let result = iter.recv_timeout(Duration::from_millis(50));
        assert!(result.is_none());
        assert!(start.elapsed() >= Duration::from_millis(45));
    }

    #[test]
    fn test_recv_timeout_with_event() {
        let (fanout, iter) = fanout_with_iter();

        // Send event after a short delay
        let sender = Arc::clone(&fanout);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            sender.send(create_test_event());
        });

        // Should receive within timeout
        let result = iter.recv_timeout(Duration::from_millis(500));
        assert!(result.is_some());
    }

    #[test]
    fn test_try_iter() {
        let (fanout, iter) = fanout_with_iter();

        for _ in 0..3 {
            fanout.send(create_test_event());
        }

        // Should get all events via try_iter
        let events: Vec<_> = iter.try_iter().collect();
        assert_eq!(events.len(), 3);

        // Should be empty now
        assert!(iter.try_recv().is_none());
    }

    #[test]
    fn test_blocking_recv() {
        let (fanout, iter) = fanout_with_iter();

        let sender = Arc::clone(&fanout);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            sender.send(create_test_event());
        });

        // Should block and receive
        let event = iter.recv().unwrap();
        assert_eq!(event.property_key(), "volume");
    }

    /// Dropping the fan-out must close every subscriber's queue, so a blocked
    /// `recv()` wakes up and returns `None` instead of hanging forever.
    ///
    /// This is why `ChangeIterator::fanout` is `Weak`. With a strong `Arc` the
    /// iterator keeps the fan-out — and therefore its own `Sender` — alive for as
    /// long as it lives, so the channel can never close and `recv()` blocks
    /// forever. That is a deadlock, not a wrong value, so the assertion runs on a
    /// worker thread with a bounded join: the test *fails* rather than wedging
    /// the whole suite.
    #[test]
    fn test_channel_closed() {
        let (fanout, iter) = fanout_with_iter();

        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let got = iter.recv();
            let _ = done_tx.send(got);
        });

        // Dropping the fan-out closes every subscriber's queue.
        drop(fanout);

        match done_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(None) => {} // correct: closed channel reported as end of stream
            Ok(Some(e)) => panic!("expected no event from a closed fan-out, got {e:?}"),
            Err(_) => panic!(
                "recv() did not return after the fan-out was dropped — the \
                 iterator is holding its own sender alive (should be `Weak`)"
            ),
        }
    }

    /// A slow subscriber loses nothing while a fast one races ahead, and neither
    /// blocks the other. This is the failure mode of the design we rejected: a
    /// bounded broadcast ring would have overwritten the slow subscriber's
    /// backlog and it would have had no way to know.
    #[test]
    fn test_slow_subscriber_loses_no_events() {
        let fanout = Arc::new(EventFanout::new());
        let fast = ChangeIterator::new(&fanout);
        let slow = ChangeIterator::new(&fanout);

        // Far more events than any plausible ring buffer would hold. Capped at
        // 100 because `Volume::new` clamps there, and the assertion below needs
        // each event to carry a distinguishable value.
        for v in 0..100u8 {
            assert_eq!(
                fanout.send(event_with_volume(v)),
                2,
                "both subscribers must be delivered to"
            );
        }

        // The fast subscriber drains everything...
        let fast_seen: Vec<u8> = fast.try_iter().map(|e| volume_of(&e)).collect();
        assert_eq!(fast_seen, (0..100u8).collect::<Vec<_>>());

        // ...and the slow one, which never ran until now, still has all 100 in
        // order. Nothing was dropped and nothing overwrote its backlog.
        let slow_seen: Vec<u8> = slow.try_iter().map(|e| volume_of(&e)).collect();
        assert_eq!(slow_seen, (0..100u8).collect::<Vec<_>>());
    }

    /// Dropping an iterator releases its slot immediately, without waiting for a
    /// send to notice. Guards `ChangeIterator::drop`.
    #[test]
    fn test_dropped_iterator_deregisters_immediately() {
        let fanout = Arc::new(EventFanout::new());
        let keep = ChangeIterator::new(&fanout);
        let discard = ChangeIterator::new(&fanout);
        assert_eq!(fanout.subscriber_count(), 2);

        drop(discard);

        // No event has been sent in between: the slot is gone because `Drop`
        // removed it, not because a failed send reaped it.
        assert_eq!(
            fanout.subscriber_count(),
            1,
            "a dropped ChangeIterator must deregister itself on drop"
        );

        // The survivor keeps working and is not stalled by the departure.
        assert_eq!(fanout.send(event_with_volume(9)), 1);
        assert_eq!(volume_of(&keep.recv().unwrap()), 9);
    }

    /// A subscriber whose receiver vanished without a `ChangeIterator` wrapper is
    /// reaped on the next send, so the registry cannot grow dead senders forever.
    /// Guards the `retain` in `EventFanout::send`, independently of `Drop`.
    #[test]
    fn test_send_reaps_dead_subscriber() {
        let fanout = Arc::new(EventFanout::new());
        let keep = ChangeIterator::new(&fanout);

        // A raw subscription, so no `Drop` impl is involved in its removal.
        let (_id, raw_rx) = fanout.subscribe();
        assert_eq!(fanout.subscriber_count(), 2);
        drop(raw_rx);

        // Still registered — nothing has tried to send to it yet.
        assert_eq!(fanout.subscriber_count(), 2);

        // The send delivers to the live subscriber only, and reaps the dead one.
        assert_eq!(fanout.send(event_with_volume(5)), 1);
        assert_eq!(
            fanout.subscriber_count(),
            1,
            "a send to a dead subscriber must remove it"
        );
        assert_eq!(volume_of(&keep.recv().unwrap()), 5);
    }
}
