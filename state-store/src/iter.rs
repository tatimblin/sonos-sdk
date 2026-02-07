//! Blocking iterator over property change events
//!
//! Provides various iteration patterns for consuming change events:
//! - Blocking: `recv()`, `for event in iter`
//! - Non-blocking: `try_recv()`, `try_iter()`
//! - Timeout: `recv_timeout()`, `timeout_iter()`

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::event::ChangeEvent;

/// Blocking iterator over property change events
///
/// Receives change events for watched properties via `std::sync::mpsc`.
/// All methods are synchronous - no async/await required.
///
/// # Example
///
/// ```rust,ignore
/// // Blocking iteration
/// for event in store.iter() {
///     println!("{} changed on {:?}", event.property_key, event.entity_id);
/// }
///
/// // Non-blocking check
/// for event in store.iter().try_iter() {
///     println!("{} changed", event.property_key);
/// }
///
/// // With timeout
/// if let Some(event) = store.iter().recv_timeout(Duration::from_secs(1)) {
///     println!("Got event: {:?}", event);
/// }
/// ```
pub struct ChangeIterator<Id> {
    rx: Arc<Mutex<mpsc::Receiver<ChangeEvent<Id>>>>,
}

impl<Id> ChangeIterator<Id> {
    /// Create a new ChangeIterator from a shared receiver
    pub(crate) fn new(rx: Arc<Mutex<mpsc::Receiver<ChangeEvent<Id>>>>) -> Self {
        Self { rx }
    }

    /// Block until the next event is available
    ///
    /// Returns `None` if the channel is closed.
    pub fn recv(&self) -> Option<ChangeEvent<Id>> {
        self.rx.lock().ok()?.recv().ok()
    }

    /// Block until the next event or timeout expires
    ///
    /// Returns `None` if the timeout expires or channel is closed.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<ChangeEvent<Id>> {
        self.rx.lock().ok()?.recv_timeout(timeout).ok()
    }

    /// Try to receive an event without blocking
    ///
    /// Returns `None` if no event is currently available.
    pub fn try_recv(&self) -> Option<ChangeEvent<Id>> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// Get a non-blocking iterator over currently available events
    ///
    /// Returns an iterator that yields all events currently in the queue
    /// without blocking. Useful for batch processing.
    pub fn try_iter(&self) -> TryIter<'_, Id> {
        TryIter { inner: self }
    }

    /// Get a blocking iterator with timeout
    ///
    /// Returns an iterator that blocks for up to `timeout` on each call
    /// to `next()`. Stops when timeout expires without events.
    pub fn timeout_iter(&self, timeout: Duration) -> TimeoutIter<'_, Id> {
        TimeoutIter {
            inner: self,
            timeout,
        }
    }
}

impl<Id> Iterator for ChangeIterator<Id> {
    type Item = ChangeEvent<Id>;

    /// Block until the next change event
    ///
    /// Returns `None` if the channel is closed.
    fn next(&mut self) -> Option<Self::Item> {
        self.recv()
    }
}

/// Non-blocking iterator over currently available events
pub struct TryIter<'a, Id> {
    inner: &'a ChangeIterator<Id>,
}

impl<'a, Id> Iterator for TryIter<'a, Id> {
    type Item = ChangeEvent<Id>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.try_recv()
    }
}

/// Blocking iterator with timeout
pub struct TimeoutIter<'a, Id> {
    inner: &'a ChangeIterator<Id>,
    timeout: Duration,
}

impl<'a, Id> Iterator for TimeoutIter<'a, Id> {
    type Item = ChangeEvent<Id>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.recv_timeout(self.timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;

    fn create_test_event() -> ChangeEvent<String> {
        ChangeEvent::new("test-entity".to_string(), "test-property")
    }

    #[test]
    fn test_try_recv_empty() {
        let (tx, rx) = mpsc::channel::<ChangeEvent<String>>();
        let iter = ChangeIterator::new(Arc::new(Mutex::new(rx)));

        // Should return None when empty
        assert!(iter.try_recv().is_none());

        // Prevent unused warning
        drop(tx);
    }

    #[test]
    fn test_try_recv_with_event() {
        let (tx, rx) = mpsc::channel();
        let iter = ChangeIterator::new(Arc::new(Mutex::new(rx)));

        // Send an event
        tx.send(create_test_event()).unwrap();

        // Should receive the event
        let event = iter.try_recv().unwrap();
        assert_eq!(event.property_key, "test-property");
        assert_eq!(event.entity_id, "test-entity");

        // Should return None now
        assert!(iter.try_recv().is_none());
    }

    #[test]
    fn test_recv_timeout() {
        let (tx, rx) = mpsc::channel::<ChangeEvent<String>>();
        let iter = ChangeIterator::new(Arc::new(Mutex::new(rx)));

        // Should timeout when empty
        let start = Instant::now();
        let result = iter.recv_timeout(Duration::from_millis(50));
        assert!(result.is_none());
        assert!(start.elapsed() >= Duration::from_millis(45));

        // Prevent unused warning
        drop(tx);
    }

    #[test]
    fn test_recv_timeout_with_event() {
        let (tx, rx) = mpsc::channel();
        let iter = ChangeIterator::new(Arc::new(Mutex::new(rx)));

        // Send event after a short delay
        let tx_clone = tx.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            tx_clone.send(create_test_event()).unwrap();
        });

        // Should receive within timeout
        let result = iter.recv_timeout(Duration::from_millis(100));
        assert!(result.is_some());

        drop(tx);
    }

    #[test]
    fn test_try_iter() {
        let (tx, rx) = mpsc::channel();
        let iter = ChangeIterator::new(Arc::new(Mutex::new(rx)));

        // Send multiple events
        for _ in 0..3 {
            tx.send(create_test_event()).unwrap();
        }

        // Should get all events via try_iter
        let events: Vec<_> = iter.try_iter().collect();
        assert_eq!(events.len(), 3);

        // Should be empty now
        assert!(iter.try_recv().is_none());

        drop(tx);
    }

    #[test]
    fn test_blocking_recv() {
        let (tx, rx) = mpsc::channel();
        let iter = ChangeIterator::new(Arc::new(Mutex::new(rx)));

        // Send event from another thread
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            tx.send(create_test_event()).unwrap();
        });

        // Should block and receive
        let event = iter.recv().unwrap();
        assert_eq!(event.property_key, "test-property");
    }

    #[test]
    fn test_channel_closed() {
        let (tx, rx) = mpsc::channel::<ChangeEvent<String>>();
        let iter = ChangeIterator::new(Arc::new(Mutex::new(rx)));

        // Close the channel
        drop(tx);

        // Should return None
        assert!(iter.recv().is_none());
    }
}
