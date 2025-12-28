//! Event processor for routing and parsing UPnP events.
//!
//! The `EventProcessor` runs a background task that receives raw events from the callback
//! server, routes them to the appropriate strategy for parsing, and emits parsed events
//! or parse errors to the application.
//!
//! # Responsibilities
//!
//! - Receive raw events from the callback server via an unbounded channel
//! - Look up the strategy for each event's service type
//! - Call the strategy's `parse_event()` method to parse the raw XML
//! - Emit `ServiceEvent` for each successfully parsed event
//! - Emit `ParseError` for events that fail to parse
//! - Update subscription timestamps when events are received
//! - Handle graceful shutdown with a 2-second timeout
//!
//! # Architecture
//!
//! The event processor operates independently from other broker components. It receives
//! raw events via a channel from the callback server and emits parsed events via a
//! channel to the application. This decoupling allows event processing to continue
//! even if other broker operations are blocked.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::event::Event;
use crate::services::ServiceStrategy;
use crate::types::RawEvent;
use crate::types::{ServiceType, SubscriptionKey, ActiveSubscription};



/// Event processor that routes raw events to strategies for parsing.
///
/// The processor runs a background task that continuously processes events until
/// shutdown is requested. Events are processed sequentially to maintain ordering,
/// but processing is non-blocking to prevent one slow event from blocking others.
pub struct EventProcessor {
    /// Handle to the background processing task
    #[allow(dead_code)]
    processing_task: Option<JoinHandle<()>>,
}

impl EventProcessor {
    /// Start the event processor with a background task.
    ///
    /// This method spawns a background task that receives raw events from the callback
    /// server and processes them. The task runs until the raw event channel is closed
    /// (which happens during broker shutdown).
    ///
    /// # Parameters
    ///
    /// * `raw_event_rx` - Receiver for raw events from the callback server
    /// * `strategies` - Map of service type to strategy implementation
    /// * `subscriptions` - Shared subscription state for updating timestamps
    /// * `event_sender` - Channel for emitting parsed events and errors
    ///
    /// # Returns
    ///
    /// Returns an `EventProcessor` instance with a handle to the background task.
    pub fn start(
        raw_event_rx: mpsc::UnboundedReceiver<RawEvent>,
        strategies: Arc<HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>>>,
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
    ) -> Self {
        let processing_task = Self::start_processing_task(
            raw_event_rx,
            strategies,
            subscriptions,
            event_sender,
        );

        Self {
            processing_task: Some(processing_task),
        }
    }

    /// Shutdown the event processor gracefully.
    ///
    /// This method aborts the background processing task and waits for it to complete
    /// with a 2-second timeout. The task is aborted because it waits on a channel that
    /// may not close immediately.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if shutdown completed successfully within the timeout.
    ///
    /// # Errors
    ///
    /// Returns an error message if the shutdown times out or the task panicked.
    #[allow(dead_code)]
    pub async fn shutdown(mut self) -> Result<(), String> {
        if let Some(processing_task) = self.processing_task.take() {
            // Abort the processing task since it waits on channel
            processing_task.abort();

            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                processing_task,
            )
            .await
            {
                Ok(Ok(())) => {
                    // Task completed successfully
                    Ok(())
                }
                Ok(Err(e)) if e.is_cancelled() => {
                    // Task was cancelled, which is expected
                    Ok(())
                }
                Ok(Err(e)) => {
                    // Task panicked
                    Err(format!("Event processing task panicked during shutdown: {e}"))
                }
                Err(_) => {
                    // Timeout
                    Err("Event processing task shutdown timed out".to_string())
                }
            }
        } else {
            Ok(())
        }
    }

    /// Start the background task that processes raw events.
    ///
    /// This task runs in a loop, receiving raw events from the callback server and
    /// processing them one at a time. The task exits when the raw event channel is
    /// closed (which happens during broker shutdown).
    fn start_processing_task(
        mut raw_event_rx: mpsc::UnboundedReceiver<RawEvent>,
        strategies: Arc<HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>>>,
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(raw_event) = raw_event_rx.recv().await {
                // Process the event
                Self::process_raw_event(
                    raw_event,
                    &strategies,
                    &subscriptions,
                    &event_sender,
                )
                .await;
            }
        })
    }

    /// Process a single raw event from the callback server.
    ///
    /// This method:
    /// 1. Looks up the strategy for the service type
    /// 2. Calls the strategy to parse the event
    /// 3. Emits `ServiceEvent` for each parsed event
    /// 4. Emits `ParseError` if parsing fails
    /// 5. Updates the subscription's last event timestamp
    ///
    /// # Parameters
    ///
    /// * `raw_event` - The raw event from the callback server
    /// * `strategies` - Map of service type to strategy implementation
    /// * `subscriptions` - Shared subscription state for updating timestamps
    /// * `event_sender` - Channel for emitting parsed events and errors
    async fn process_raw_event(
        raw_event: RawEvent,
        strategies: &Arc<HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>>>,
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
    ) {
        let speaker_id = raw_event.speaker_id.clone();
        let service_type = raw_event.service_type;
        let event_xml = raw_event.event_xml;

        // Look up strategy for service type
        let strategy = match strategies.get(&service_type) {
            Some(s) => s,
            None => {
                // No strategy registered - emit parse error with detailed context
                eprintln!(
                    "⚠️  EventProcessor: No strategy registered for service type {:?} from speaker {}. Available strategies: {:?}",
                    service_type,
                    speaker_id.as_str(),
                    strategies.keys().collect::<Vec<_>>()
                );
                
                let _ = event_sender
                    .send(Event::ParseError {
                        speaker_id,
                        service_type,
                        error: format!(
                            "No strategy registered for service type: {service_type:?}. Available strategies: {:?}",
                            strategies.keys().collect::<Vec<_>>()
                        ),
                        timestamp: SystemTime::now(),
                    })
                    .await;
                return;
            }
        };

        // Parse the event using the strategy
        match strategy.parse_event(&speaker_id, &event_xml) {
            Ok(typed_event) => {
                // Emit ServiceEvent with TypedEvent directly
                let _ = event_sender
                    .send(Event::ServiceEvent {
                        speaker_id: speaker_id.clone(),
                        service_type,
                        event: typed_event,
                        timestamp: SystemTime::now(),
                    })
                    .await;

                // Update subscription's last event timestamp
                let key = SubscriptionKey::new(speaker_id, service_type);
                let mut subs = subscriptions.write().await;
                if let Some(active_sub) = subs.get_mut(&key) {
                    active_sub.last_event = Some(SystemTime::now());
                }
            }
            Err(e) => {
                // Log parse error with context for debugging
                eprintln!(
                    "❌ EventProcessor: Parse error for speaker {} service {:?}: {}. XML length: {} bytes",
                    speaker_id.as_str(),
                    service_type,
                    e,
                    event_xml.len()
                );
                
                // Emit ParseError event with enhanced error information
                let enhanced_error = format!(
                    "Parse failed for {} bytes of XML: {}. Original error: {}",
                    event_xml.len(),
                    if event_xml.len() > 100 {
                        format!("{}...", &event_xml[..100])
                    } else {
                        event_xml.clone()
                    },
                    e
                );
                
                let _ = event_sender
                    .send(Event::ParseError {
                        speaker_id,
                        service_type,
                        error: enhanced_error,
                        timestamp: SystemTime::now(),
                    })
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::types::{ServiceType, SpeakerId};
    use std::time::Duration;



    #[tokio::test]
    async fn test_process_raw_event_success() {
        let (event_tx, mut event_rx) = mpsc::channel(10);
        let (_raw_event_tx, _raw_event_rx) = mpsc::unbounded_channel::<RawEvent>();

        let mut strategies: HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>> = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(crate::services::AVTransportProvider::new()),
        );
        let strategies = Arc::new(strategies);

        let subscriptions = Arc::new(RwLock::new(HashMap::new()));
        let speaker_id = SpeakerId::new("speaker1");
        let key = SubscriptionKey::new(speaker_id.clone(), ServiceType::AVTransport);

        // Add a subscription to update
        {
            let mut subs = subscriptions.write().await;
            subs.insert(
                key.clone(),
                ActiveSubscription::new(
                    key.clone(),
                    "sub-123".to_string(),
                    SystemTime::now() + Duration::from_secs(1800), // Expires in 30 minutes
                ),
            );
        }

        // Create raw event with valid AVTransport XML
        let event_xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0"><e:property><LastChange>&lt;Event xmlns="urn:schemas-upnp-org:metadata-1-0/AVT/"&gt;&lt;InstanceID val="0"&gt;&lt;TransportState val="PLAYING"/&gt;&lt;/InstanceID&gt;&lt;/Event&gt;</LastChange></e:property></e:propertyset>"#;
        
        let raw_event = RawEvent {
            subscription_id: "sub-123".to_string(),
            speaker_id: speaker_id.clone(),
            service_type: ServiceType::AVTransport,
            event_xml: event_xml.to_string(),
        };

        // Process the event
        EventProcessor::process_raw_event(
            raw_event,
            &strategies,
            &subscriptions,
            &event_tx,
        )
        .await;

        // Verify ServiceEvent was emitted
        let event = event_rx.recv().await.unwrap();
        match event {
            Event::ServiceEvent {
                speaker_id: sid,
                service_type: st,
                event: typed_event,
                ..
            } => {
                assert_eq!(sid, speaker_id);
                assert_eq!(st, ServiceType::AVTransport);
                assert_eq!(typed_event.event_type(), "av_transport_event");
            }
            _ => panic!("Expected ServiceEvent"),
        }

        // Verify timestamp was updated
        let subs = subscriptions.read().await;
        let active_sub = subs.get(&key).unwrap();
        assert!(active_sub.last_event.is_some());
    }

    #[tokio::test]
    async fn test_process_raw_event_parse_error() {
        let (event_tx, mut event_rx) = mpsc::channel(10);

        let mut strategies: HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>> = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(crate::services::AVTransportProvider::new()),
        );
        let strategies = Arc::new(strategies);

        let subscriptions = Arc::new(RwLock::new(HashMap::new()));
        let speaker_id = SpeakerId::new("speaker1");

        // Create raw event with invalid XML
        let raw_event = RawEvent {
            subscription_id: "sub-123".to_string(),
            speaker_id: speaker_id.clone(),
            service_type: ServiceType::AVTransport,
            event_xml: "<invalid>xml</invalid>".to_string(),
        };

        // Process the event
        EventProcessor::process_raw_event(
            raw_event,
            &strategies,
            &subscriptions,
            &event_tx,
        )
        .await;

        // Verify ParseError was emitted
        let event = event_rx.recv().await.unwrap();
        match event {
            Event::ParseError {
                speaker_id: sid,
                service_type: st,
                error,
                ..
            } => {
                assert_eq!(sid, speaker_id);
                assert_eq!(st, ServiceType::AVTransport);
                assert!(error.contains("Failed to parse event"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[tokio::test]
    async fn test_process_raw_event_no_strategy() {
        let (event_tx, mut event_rx) = mpsc::channel(10);

        let strategies: HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>> = HashMap::new();
        let strategies = Arc::new(strategies);

        let subscriptions = Arc::new(RwLock::new(HashMap::new()));
        let speaker_id = SpeakerId::new("speaker1");

        // Create raw event for service with no strategy
        let raw_event = RawEvent {
            subscription_id: "sub-123".to_string(),
            speaker_id: speaker_id.clone(),
            service_type: ServiceType::AVTransport,
            event_xml: "<event>test</event>".to_string(),
        };

        // Process the event
        EventProcessor::process_raw_event(
            raw_event,
            &strategies,
            &subscriptions,
            &event_tx,
        )
        .await;

        // Verify ParseError was emitted with enhanced error message
        let event = event_rx.recv().await.unwrap();
        match event {
            Event::ParseError {
                speaker_id: sid,
                service_type: st,
                error,
                ..
            } => {
                assert_eq!(sid, speaker_id);
                assert_eq!(st, ServiceType::AVTransport);
                assert!(error.contains("No strategy registered"));
                assert!(error.contains("Available strategies"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[tokio::test]
    async fn test_process_raw_event_enhanced_parse_error() {
        let (event_tx, mut event_rx) = mpsc::channel(10);

        let mut strategies: HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>> = HashMap::new();
        strategies.insert(
            ServiceType::AVTransport,
            Box::new(crate::services::AVTransportProvider::new()),
        );
        let strategies = Arc::new(strategies);

        let subscriptions = Arc::new(RwLock::new(HashMap::new()));
        let speaker_id = SpeakerId::new("speaker1");

        // Create raw event with invalid XML
        let raw_event = RawEvent {
            subscription_id: "sub-123".to_string(),
            speaker_id: speaker_id.clone(),
            service_type: ServiceType::AVTransport,
            event_xml: "<invalid>xml</invalid>".to_string(),
        };

        // Process the event
        EventProcessor::process_raw_event(
            raw_event,
            &strategies,
            &subscriptions,
            &event_tx,
        )
        .await;

        // Verify ParseError was emitted with enhanced error information
        let event = event_rx.recv().await.unwrap();
        match event {
            Event::ParseError {
                speaker_id: sid,
                service_type: st,
                error,
                ..
            } => {
                assert_eq!(sid, speaker_id);
                assert_eq!(st, ServiceType::AVTransport);
                assert!(error.contains("Parse failed for"));
                assert!(error.contains("bytes of XML"));
                assert!(error.contains("Original error"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[tokio::test]
    async fn test_event_processor_start_and_shutdown() {
        let (event_tx, _event_rx) = mpsc::channel(10);
        let (raw_event_tx, raw_event_rx) = mpsc::unbounded_channel();

        let strategies: HashMap<ServiceType, Box<dyn ServiceStrategy + Send + Sync>> = HashMap::new();
        let strategies = Arc::new(strategies);

        let subscriptions = Arc::new(RwLock::new(HashMap::new()));

        // Start processor
        let processor = EventProcessor::start(
            raw_event_rx,
            strategies,
            subscriptions,
            event_tx,
        );

        // Send a raw event
        let raw_event = RawEvent {
            subscription_id: "sub-123".to_string(),
            speaker_id: SpeakerId::new("speaker1"),
            service_type: ServiceType::AVTransport,
            event_xml: "<event>test</event>".to_string(),
        };
        raw_event_tx.send(raw_event).unwrap();

        // Give it a moment to process
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Shutdown
        let result = processor.shutdown().await;
        assert!(result.is_ok());
    }

}
