//! Sync iterator for consuming events from SonosEventManager
//!
//! Provides a blocking iterator interface for processing events
//! without requiring async/await.

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use sonos_stream::events::EnrichedEvent;

/// Blocking iterator over enriched events
///
/// This iterator blocks on `next()` until an event is available or the
/// channel is closed. Use `try_recv()` for non-blocking access.
pub struct EventManagerIterator {
    rx: Arc<Mutex<mpsc::Receiver<EnrichedEvent>>>,
}

impl EventManagerIterator {
    /// Create a new iterator from a shared receiver
    pub(crate) fn new(rx: Arc<Mutex<mpsc::Receiver<EnrichedEvent>>>) -> Self {
        Self { rx }
    }

    /// Block until an event is available
    ///
    /// Returns `None` if the channel is closed.
    pub fn recv(&self) -> Option<EnrichedEvent> {
        self.rx.lock().ok()?.recv().ok()
    }

    /// Try to receive an event without blocking
    ///
    /// Returns `None` if no event is currently available or channel is closed.
    pub fn try_recv(&self) -> Option<EnrichedEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// Block until an event is available or timeout expires
    ///
    /// Returns `None` if the timeout expires or channel is closed.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<EnrichedEvent> {
        self.rx.lock().ok()?.recv_timeout(timeout).ok()
    }

    /// Get a non-blocking iterator over currently available events
    ///
    /// Useful for batch processing without blocking.
    pub fn try_iter(&self) -> TryIterator<'_> {
        TryIterator { inner: self }
    }

    /// Get a blocking iterator with timeout
    ///
    /// Blocks for up to `timeout` on each call to `next()`.
    pub fn timeout_iter(&self, timeout: Duration) -> TimeoutIterator<'_> {
        TimeoutIterator {
            inner: self,
            timeout,
        }
    }
}

impl Iterator for EventManagerIterator {
    type Item = EnrichedEvent;

    /// Block until the next event is available
    fn next(&mut self) -> Option<Self::Item> {
        self.recv()
    }
}

impl Clone for EventManagerIterator {
    fn clone(&self) -> Self {
        Self {
            rx: Arc::clone(&self.rx),
        }
    }
}

/// Non-blocking iterator over currently available events
pub struct TryIterator<'a> {
    inner: &'a EventManagerIterator,
}

impl<'a> Iterator for TryIterator<'a> {
    type Item = EnrichedEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.try_recv()
    }
}

/// Blocking iterator with timeout
pub struct TimeoutIterator<'a> {
    inner: &'a EventManagerIterator,
    timeout: Duration,
}

impl<'a> Iterator for TimeoutIterator<'a> {
    type Item = EnrichedEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.recv_timeout(self.timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_recv_empty() {
        let (tx, rx) = mpsc::channel();
        let iter = EventManagerIterator::new(Arc::new(Mutex::new(rx)));

        // Should return None when empty
        assert!(iter.try_recv().is_none());

        // Prevent unused warning
        drop(tx);
    }

    #[test]
    fn test_recv_timeout_empty() {
        let (tx, rx) = mpsc::channel::<EnrichedEvent>();
        let iter = EventManagerIterator::new(Arc::new(Mutex::new(rx)));

        // Should timeout when empty
        let start = std::time::Instant::now();
        let result = iter.recv_timeout(Duration::from_millis(50));
        assert!(result.is_none());
        assert!(start.elapsed() >= Duration::from_millis(45));

        drop(tx);
    }

    #[test]
    fn test_try_iter_empty() {
        let (tx, rx) = mpsc::channel::<EnrichedEvent>();
        let iter = EventManagerIterator::new(Arc::new(Mutex::new(rx)));

        // Should return empty vec when no events
        let events: Vec<_> = iter.try_iter().collect();
        assert!(events.is_empty());

        drop(tx);
    }

    #[test]
    fn test_clone() {
        let (tx, rx) = mpsc::channel::<EnrichedEvent>();
        let iter1 = EventManagerIterator::new(Arc::new(Mutex::new(rx)));
        let iter2 = iter1.clone();

        // Both should see no events
        assert!(iter1.try_recv().is_none());
        assert!(iter2.try_recv().is_none());

        drop(tx);
    }
}
