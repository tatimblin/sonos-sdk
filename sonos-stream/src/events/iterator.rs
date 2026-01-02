//! Event iterator interfaces for consuming events
//!
//! This module provides both sync and async iterator interfaces for consuming events,
//! with sync being the best practice for local state management and async for real-time processing.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use futures::Stream;
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;

use crate::error::{EventProcessingError, EventProcessingResult};
use crate::events::types::{EnrichedEvent, EventSource, ResyncReason};
use crate::registry::RegistrationId;
use crate::subscription::event_detector::ResyncDetector;

/// Main event iterator that provides both sync and async interfaces
pub struct EventIterator {
    /// Receiver for enriched events
    receiver: Option<mpsc::UnboundedReceiver<EnrichedEvent>>,

    /// Buffer for events when using sync iteration
    buffered_events: VecDeque<EnrichedEvent>,

    /// Resync detector for automatic state drift detection
    resync_detector: Option<Arc<ResyncDetector>>,

    /// Tokio runtime handle for sync iteration
    runtime_handle: tokio::runtime::Handle,

    /// Statistics tracking
    stats: EventIteratorStats,

    /// Whether the iterator has been consumed
    consumed: bool,
}

impl EventIterator {
    /// Create a new event iterator
    pub fn new(
        receiver: mpsc::UnboundedReceiver<EnrichedEvent>,
        resync_detector: Option<Arc<ResyncDetector>>,
    ) -> Self {
        let runtime_handle = tokio::runtime::Handle::try_current()
            .expect("EventIterator must be created within a Tokio runtime");

        Self {
            receiver: Some(receiver),
            buffered_events: VecDeque::new(),
            resync_detector,
            runtime_handle,
            stats: EventIteratorStats::new(),
            consumed: false,
        }
    }

    /// ASYNC INTERFACE - Get the next event asynchronously
    /// Best for real-time event processing where you want to handle events as they arrive
    pub async fn next_async(&mut self) -> Option<EnrichedEvent> {
        if self.consumed {
            return None;
        }

        // First check buffered events
        if let Some(event) = self.buffered_events.pop_front() {
            self.stats.events_delivered += 1;
            return Some(event);
        }

        // Check for automatic resync needs
        if let Some(resync_event) = self.check_and_emit_resync().await {
            self.stats.resync_events_emitted += 1;
            self.stats.events_delivered += 1;
            return Some(resync_event);
        }

        // Get next event from receiver
        if let Some(receiver) = &mut self.receiver {
            match receiver.recv().await {
                Some(event) => {
                    self.stats.events_received += 1;
                    self.stats.events_delivered += 1;
                    Some(event)
                }
                None => {
                    // Channel closed
                    self.consumed = true;
                    None
                }
            }
        } else {
            None
        }
    }

    /// ASYNC INTERFACE - Get next event with timeout
    pub async fn next_timeout(&mut self, timeout_duration: Duration) -> EventProcessingResult<Option<EnrichedEvent>> {
        match timeout(timeout_duration, self.next_async()).await {
            Ok(event) => Ok(event),
            Err(_) => {
                self.stats.timeouts += 1;
                Err(EventProcessingError::Timeout)
            }
        }
    }

    /// ASYNC INTERFACE - Try to get next event without blocking
    pub fn try_next(&mut self) -> EventProcessingResult<Option<EnrichedEvent>> {
        if self.consumed {
            return Ok(None);
        }

        // Check buffered events first
        if let Some(event) = self.buffered_events.pop_front() {
            self.stats.events_delivered += 1;
            return Ok(Some(event));
        }

        // Try to receive from channel without blocking
        if let Some(receiver) = &mut self.receiver {
            match receiver.try_recv() {
                Ok(event) => {
                    self.stats.events_received += 1;
                    self.stats.events_delivered += 1;
                    Ok(Some(event))
                }
                Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.consumed = true;
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    /// SYNC INTERFACE - Get iterator for simple loop patterns
    /// **BEST PRACTICE for local state management**
    ///
    /// This is the recommended interface for maintaining local state from events.
    /// Use like: `for event in events.iter() { /* handle event */ }`
    pub fn iter(&mut self) -> SyncEventIterator {
        SyncEventIterator::new(self)
    }

    /// Check for automatic resync needs
    async fn check_and_emit_resync(&mut self) -> Option<EnrichedEvent> {
        // This is a placeholder implementation
        // In a real implementation, this would coordinate with ResyncDetector
        // to check if any registrations need resync events
        None
    }

    /// Buffer multiple events for batch processing
    pub async fn next_batch(&mut self, max_count: usize, max_wait: Duration) -> Vec<EnrichedEvent> {
        let mut events = Vec::new();
        let start = tokio::time::Instant::now();

        // Get first event (wait for it)
        if let Some(first_event) = self.next_async().await {
            events.push(first_event);
        } else {
            return events; // No events available
        }

        // Try to get additional events without blocking
        while events.len() < max_count && start.elapsed() < max_wait {
            match self.try_next() {
                Ok(Some(event)) => events.push(event),
                Ok(None) => break, // No more events available
                Err(_) => break,   // Error occurred
            }
        }

        events
    }

    /// Get iterator statistics
    pub fn stats(&self) -> &EventIteratorStats {
        &self.stats
    }

    /// Check if the iterator has been consumed (channel closed)
    pub fn is_consumed(&self) -> bool {
        self.consumed
    }

    /// Peek at the next event without consuming it
    pub async fn peek(&mut self) -> Option<&EnrichedEvent> {
        // If we don't have buffered events, try to get one
        if self.buffered_events.is_empty() {
            if let Some(event) = self.next_async().await {
                self.buffered_events.push_back(event);
                self.stats.events_delivered -= 1; // Don't count peeked events as delivered
            }
        }

        self.buffered_events.front()
    }

    /// Filter events by registration ID
    pub fn filter_by_registration(self, registration_id: RegistrationId) -> FilteredEventIterator {
        FilteredEventIterator::new(self, move |event| event.registration_id == registration_id)
    }

    /// Filter events by service type
    pub fn filter_by_service(self, service: sonos_api::Service) -> FilteredEventIterator {
        FilteredEventIterator::new(self, move |event| event.service == service)
    }

    /// Filter events by source type (UPnP, polling, resync)
    pub fn filter_by_source_type(self, source_type: EventSourceType) -> FilteredEventIterator {
        FilteredEventIterator::new(self, move |event| {
            match (&event.event_source, source_type) {
                (EventSource::UPnPNotification { .. }, EventSourceType::UPnP) => true,
                (EventSource::PollingDetection { .. }, EventSourceType::Polling) => true,
                (EventSource::ResyncDetection { .. }, EventSourceType::Resync) => true,
                _ => false,
            }
        })
    }
}

/// Synchronous event iterator for simple loop patterns
/// **This is the best practice for local state management**
pub struct SyncEventIterator<'a> {
    inner: &'a mut EventIterator,
}

impl<'a> SyncEventIterator<'a> {
    fn new(inner: &'a mut EventIterator) -> Self {
        Self { inner }
    }
}

impl<'a> Iterator for SyncEventIterator<'a> {
    type Item = EnrichedEvent;

    fn next(&mut self) -> Option<Self::Item> {
        // Block on async next() for sync interface
        let runtime_handle = self.inner.runtime_handle.clone();
        runtime_handle.block_on(self.inner.next_async())
    }
}

/// Implement Stream trait for EventIterator for advanced async usage
impl Stream for EventIterator {
    type Item = EnrichedEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.consumed {
            return Poll::Ready(None);
        }

        // Check buffered events first
        if let Some(event) = self.buffered_events.pop_front() {
            self.stats.events_delivered += 1;
            return Poll::Ready(Some(event));
        }

        // Poll the receiver
        if let Some(receiver) = &mut self.receiver {
            match receiver.poll_recv(cx) {
                Poll::Ready(Some(event)) => {
                    self.stats.events_received += 1;
                    self.stats.events_delivered += 1;
                    Poll::Ready(Some(event))
                }
                Poll::Ready(None) => {
                    self.consumed = true;
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(None)
        }
    }
}

/// Filter criteria for event source types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSourceType {
    UPnP,
    Polling,
    Resync,
}

/// Filtered event iterator that applies a predicate to events
pub struct FilteredEventIterator {
    inner: EventIterator,
    predicate: Box<dyn Fn(&EnrichedEvent) -> bool + Send>,
}

impl FilteredEventIterator {
    fn new<F>(inner: EventIterator, predicate: F) -> Self
    where
        F: Fn(&EnrichedEvent) -> bool + Send + 'static,
    {
        Self {
            inner,
            predicate: Box::new(predicate),
        }
    }

    /// Get the next filtered event asynchronously
    pub async fn next_async(&mut self) -> Option<EnrichedEvent> {
        loop {
            match self.inner.next_async().await {
                Some(event) => {
                    if (self.predicate)(&event) {
                        return Some(event);
                    }
                    // Event doesn't match filter, continue
                }
                None => return None,
            }
        }
    }

    /// Get sync iterator for the filtered events
    pub fn iter(&mut self) -> FilteredSyncIterator {
        FilteredSyncIterator::new(self)
    }
}

/// Sync iterator for filtered events
pub struct FilteredSyncIterator<'a> {
    inner: &'a mut FilteredEventIterator,
}

impl<'a> FilteredSyncIterator<'a> {
    fn new(inner: &'a mut FilteredEventIterator) -> Self {
        Self { inner }
    }
}

impl<'a> Iterator for FilteredSyncIterator<'a> {
    type Item = EnrichedEvent;

    fn next(&mut self) -> Option<Self::Item> {
        let runtime_handle = self.inner.inner.runtime_handle.clone();
        runtime_handle.block_on(self.inner.next_async())
    }
}

/// Statistics for event iterator usage
#[derive(Debug, Clone)]
pub struct EventIteratorStats {
    /// Events received from the channel
    pub events_received: u64,

    /// Events delivered to the consumer
    pub events_delivered: u64,

    /// Resync events generated
    pub resync_events_emitted: u64,

    /// Timeouts occurred
    pub timeouts: u64,
}

impl EventIteratorStats {
    fn new() -> Self {
        Self {
            events_received: 0,
            events_delivered: 0,
            resync_events_emitted: 0,
            timeouts: 0,
        }
    }

    /// Get the delivery rate (events delivered / events received)
    pub fn delivery_rate(&self) -> f64 {
        if self.events_received == 0 {
            1.0
        } else {
            self.events_delivered as f64 / self.events_received as f64
        }
    }
}

impl std::fmt::Display for EventIteratorStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Event Iterator Stats:")?;
        writeln!(f, "  Events received: {}", self.events_received)?;
        writeln!(f, "  Events delivered: {}", self.events_delivered)?;
        writeln!(f, "  Resync events: {}", self.resync_events_emitted)?;
        writeln!(f, "  Timeouts: {}", self.timeouts)?;
        writeln!(f, "  Delivery rate: {:.1}%", self.delivery_rate() * 100.0)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::time::SystemTime;
    use crate::events::types::{EventData, AVTransportDelta, EventSource};

    fn create_test_event(registration_id: RegistrationId) -> EnrichedEvent {
        EnrichedEvent {
            registration_id,
            speaker_ip: "192.168.1.100".parse().unwrap(),
            service: sonos_api::ServiceType::AVTransport,
            event_source: EventSource::UPnPNotification {
                subscription_id: "test-sid".to_string(),
            },
            timestamp: SystemTime::now(),
            event_data: EventData::AVTransportChange(AVTransportDelta {
                transport_state: Some("PLAYING".to_string()),
                current_track_uri: None,
                track_duration: None,
                rel_time: None,
                play_mode: None,
                track_metadata: None,
            }),
        }
    }

    #[tokio::test]
    async fn test_event_iterator_creation() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let iterator = EventIterator::new(receiver, None);

        assert!(!iterator.is_consumed());
        assert_eq!(iterator.stats().events_received, 0);
        assert_eq!(iterator.stats().events_delivered, 0);
    }

    #[tokio::test]
    async fn test_async_iteration() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut iterator = EventIterator::new(receiver, None);

        // Send test event
        let test_event = create_test_event(RegistrationId::new(1));
        sender.send(test_event.clone()).unwrap();

        // Receive event
        let received = iterator.next_async().await;
        assert!(received.is_some());
        let event = received.unwrap();
        assert_eq!(event.registration_id, test_event.registration_id);
        assert_eq!(iterator.stats().events_received, 1);
        assert_eq!(iterator.stats().events_delivered, 1);
    }

    #[tokio::test]
    async fn test_try_next() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut iterator = EventIterator::new(receiver, None);

        // Try without any events
        let result = iterator.try_next().unwrap();
        assert!(result.is_none());

        // Send event and try again
        let test_event = create_test_event(RegistrationId::new(1));
        sender.send(test_event.clone()).unwrap();

        let result = iterator.try_next().unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().registration_id, test_event.registration_id);
    }

    #[tokio::test]
    async fn test_next_timeout() {
        let (_sender, receiver) = mpsc::unbounded_channel();
        let mut iterator = EventIterator::new(receiver, None);

        // Should timeout since no events are sent
        let result = iterator.next_timeout(Duration::from_millis(100)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EventProcessingError::Timeout));
        assert_eq!(iterator.stats().timeouts, 1);
    }

    #[tokio::test]
    async fn test_next_batch() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut iterator = EventIterator::new(receiver, None);

        // Send multiple events
        for i in 1..=5 {
            let event = create_test_event(RegistrationId::new(i));
            sender.send(event).unwrap();
        }

        // Get batch of 3 events
        let batch = iterator.next_batch(3, Duration::from_millis(100)).await;
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].registration_id.as_u64(), 1);
        assert_eq!(batch[1].registration_id.as_u64(), 2);
        assert_eq!(batch[2].registration_id.as_u64(), 3);
    }

    #[test]
    fn test_sync_iteration() {
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let (sender, receiver) = mpsc::unbounded_channel();
            let mut iterator = EventIterator::new(receiver, None);

            // Send test events
            for i in 1..=3 {
                let event = create_test_event(RegistrationId::new(i));
                sender.send(event).unwrap();
            }
            drop(sender); // Close channel after sending

            // Use sync iterator
            let events: Vec<_> = iterator.iter().collect();
            assert_eq!(events.len(), 3);
            assert_eq!(events[0].registration_id.as_u64(), 1);
            assert_eq!(events[1].registration_id.as_u64(), 2);
            assert_eq!(events[2].registration_id.as_u64(), 3);
        });
    }

    #[tokio::test]
    async fn test_filtered_iterator() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let iterator = EventIterator::new(receiver, None);

        // Send events with different registration IDs
        let event1 = create_test_event(RegistrationId::new(1));
        let event2 = create_test_event(RegistrationId::new(2));
        let event3 = create_test_event(RegistrationId::new(1));

        sender.send(event1).unwrap();
        sender.send(event2).unwrap();
        sender.send(event3).unwrap();
        drop(sender);

        // Filter for registration ID 1
        let mut filtered = iterator.filter_by_registration(RegistrationId::new(1));

        let events: Vec<_> = filtered.iter().collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].registration_id.as_u64(), 1);
        assert_eq!(events[1].registration_id.as_u64(), 1);
    }

    #[tokio::test]
    async fn test_peek() {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut iterator = EventIterator::new(receiver, None);

        let test_event = create_test_event(RegistrationId::new(1));
        sender.send(test_event.clone()).unwrap();

        // Peek at the event
        let peeked = iterator.peek().await;
        assert!(peeked.is_some());
        assert_eq!(peeked.unwrap().registration_id, test_event.registration_id);

        // Event should still be available for next()
        let next = iterator.next_async().await;
        assert!(next.is_some());
        assert_eq!(next.unwrap().registration_id, test_event.registration_id);
    }

    #[test]
    fn test_stats() {
        let stats = EventIteratorStats::new();
        assert_eq!(stats.delivery_rate(), 1.0);

        let mut stats_with_data = EventIteratorStats {
            events_received: 10,
            events_delivered: 8,
            resync_events_emitted: 1,
            timeouts: 2,
        };
        assert_eq!(stats_with_data.delivery_rate(), 0.8);
    }
}