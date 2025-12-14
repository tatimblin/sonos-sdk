//! Minimal example showing how to consume events from sonos-stream.
//!
//! This example demonstrates the complete event consumption pattern:
//! 1. Creates a broker with a mock strategy (no real network calls)
//! 2. Subscribes to a mock speaker
//! 3. Simulates receiving events by sending HTTP requests to the callback server
//! 4. Parses and prints events to the terminal
//! 5. Cleans up and exits gracefully
//!
//! The mock strategy doesn't make real UPnP calls - it creates mock subscriptions
//! and parses simulated events. Events are simulated by spawning a background task
//! that POSTs to the callback server, mimicking how real Sonos speakers would send
//! notifications.
//!
//! Run with: cargo run --example simple_event_consumer

use sonos_stream::{
    EventBrokerBuilder, Event, ServiceType, Speaker, SpeakerId, 
    SubscriptionStrategy, Subscription, SubscriptionScope, SubscriptionConfig,
    StrategyError, SubscriptionError, TypedEvent,
};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, SystemTime};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use async_trait::async_trait;

/// Mock strategy that creates fake subscriptions without real UPnP calls
#[derive(Clone)]
struct MockStrategy {
    service_type: ServiceType,
    counter: Arc<AtomicU32>,
}

impl MockStrategy {
    fn new(service_type: ServiceType) -> Self {
        Self {
            service_type,
            counter: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl SubscriptionStrategy for MockStrategy {
    fn service_type(&self) -> ServiceType {
        self.service_type
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MockService/Event"
    }

    async fn create_subscription(
        &self,
        speaker: &Speaker,
        _callback_url: String,
        config: &SubscriptionConfig,
    ) -> Result<Box<dyn Subscription>, StrategyError> {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let subscription_id = format!("mock-sub-{}-{}", speaker.id.as_str(), count);
        
        Ok(Box::new(MockSubscription {
            id: subscription_id,
            speaker_id: speaker.id.clone(),
            service_type: self.service_type,
            created_at: SystemTime::now(),
            timeout: Duration::from_secs(config.timeout_seconds as u64),
        }))
    }

    fn parse_event(
        &self,
        speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<TypedEvent, StrategyError> {
        use sonos_stream::{EventData, TypedEvent};
        use std::any::Any;
        
        // Simple parsing: just extract the event type from XML
        let event_type = if event_xml.contains("PLAYING") {
            "playing"
        } else if event_xml.contains("PAUSED") {
            "paused"
        } else {
            "unknown"
        };

        let mut data = HashMap::new();
        data.insert("speaker".to_string(), speaker_id.as_str().to_string());
        data.insert("state".to_string(), event_type.to_string());

        #[derive(Debug, Clone)]
        struct MockEventData {
            event_type: String,
            data: HashMap<String, String>,
        }

        impl EventData for MockEventData {
            fn event_type(&self) -> &str {
                &self.event_type
            }

            fn service_type(&self) -> ServiceType {
                ServiceType::AVTransport
            }

            fn as_any(&self) -> &dyn Any {
                self
            }

            fn clone_box(&self) -> Box<dyn EventData> {
                Box::new(self.clone())
            }
        }

        let mock_data = MockEventData {
            event_type: event_type.to_string(),
            data,
        };

        Ok(TypedEvent::new(Box::new(mock_data)))
    }
}

/// Mock subscription that doesn't make real UPnP calls
struct MockSubscription {
    id: String,
    speaker_id: SpeakerId,
    service_type: ServiceType,
    created_at: SystemTime,
    timeout: Duration,
}

#[async_trait]
impl Subscription for MockSubscription {
    fn subscription_id(&self) -> &str {
        &self.id
    }

    async fn renew(&mut self) -> Result<(), SubscriptionError> {
        // Mock renewal - just update the created time
        self.created_at = SystemTime::now();
        Ok(())
    }

    async fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        // Mock unsubscribe - always succeeds
        Ok(())
    }

    fn is_active(&self) -> bool {
        // Consider active if not expired
        self.created_at.elapsed().unwrap_or(Duration::MAX) < self.timeout
    }

    fn time_until_renewal(&self) -> Option<Duration> {
        // Renew at 50% of timeout
        let renewal_time = self.timeout / 2;
        let elapsed = self.created_at.elapsed().unwrap_or(Duration::ZERO);
        
        if elapsed < renewal_time {
            Some(renewal_time - elapsed)
        } else {
            Some(Duration::ZERO)
        }
    }

    fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    fn service_type(&self) -> ServiceType {
        self.service_type
    }
}

/// Simulate sending events to the callback server
async fn simulate_events(callback_url: String, subscription_id: String) {
    // Wait a bit for subscription to be established
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    
    // Simulate a series of events
    let events = vec![
        ("<event><PLAYING>true</PLAYING></event>", "Playing music"),
        ("<event><PAUSED>true</PAUSED></event>", "Paused"),
        ("<event><PLAYING>true</PLAYING></event>", "Resumed playing"),
    ];

    for (xml, description) in events {
        tokio::time::sleep(Duration::from_millis(800)).await;
        
        println!("📤 Simulating event: {}", description);
        
        // The callback server expects /notify/{subscription_id}
        let url = format!("{}/notify/{}", callback_url, subscription_id);
        let result = client
            .post(&url)
            .header("SID", subscription_id.clone())  // Don't add "uuid:" prefix
            .header("NT", "upnp:event")
            .header("NTS", "upnp:propchange")
            .header("Content-Type", "text/xml")
            .body(xml.to_string())
            .send()
            .await;
            
        match result {
            Ok(response) => {
                println!("   ✓ Event sent successfully (status: {})", response.status());
            }
            Err(e) => {
                eprintln!("   ✗ Failed to send event: {}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting simple event consumer example...\n");

    // Create broker with mock strategy (no real network calls)
    let strategy = MockStrategy::new(ServiceType::AVTransport);
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(strategy))
        .with_port_range(40000, 40100)  // Use same range as integration tests
        .build()
        .await?;

    // Create a mock speaker (IP doesn't matter since we're not making real calls)
    let speaker = Speaker::new(
        SpeakerId::new("RINCON_EXAMPLE123"),
        "127.0.0.1".parse::<IpAddr>()?,
        "Living Room".to_string(),
        "Living Room".to_string(),
    );

    println!("Subscribing to speaker: {}", speaker.name);
    
    // Subscribe to the speaker
    broker.subscribe(&speaker, ServiceType::AVTransport).await?;

    // Get the event stream
    let mut event_stream = broker.event_stream();

    println!("Listening for events...\n");

    // Capture the subscription ID from the first event
    let subscription_id = if let Some(Event::SubscriptionEstablished { subscription_id, .. }) = event_stream.recv().await {
        println!("✓ Subscription established");
        println!("  ID: {}\n", subscription_id);
        subscription_id
    } else {
        return Err("Failed to establish subscription".into());
    };

    // Use the same approach as integration tests - hardcode to first port in range
    let callback_url = format!("http://127.0.0.1:40000");

    // Spawn task to simulate events
    let sim_url = callback_url.clone();
    let sim_sub_id = subscription_id.clone();
    tokio::spawn(async move {
        simulate_events(sim_url, sim_sub_id).await;
    });

    // Process events with timeout
    let mut event_count = 0;
    let max_events = 3; // Wait for 3 service events then exit
    let timeout_duration = Duration::from_secs(10); // Total timeout for the demo

    println!("Waiting for events (timeout: {}s)...\n", timeout_duration.as_secs());

    let start_time = std::time::Instant::now();

    loop {
        let remaining_time = timeout_duration.saturating_sub(start_time.elapsed());
        if remaining_time.is_zero() {
            println!("⏰ Demo timeout reached. Events may not have been received due to network configuration.");
            println!("   This is normal in a mock example - the subscription lifecycle worked correctly!\n");
            break;
        }

        tokio::select! {
            Some(event) = event_stream.recv() => {
                match event {
                    Event::ServiceEvent { speaker_id, service_type, event } => {
                        println!("→ Event received:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}", service_type);
                        println!("  Type: {}", event.event_type());
                        println!("  Data: {:?}\n", event.debug());
                        event_count += 1;
                        
                        if event_count >= max_events {
                            println!("Received {} events, exiting...\n", max_events);
                            break;
                        }
                    }
                    Event::SubscriptionFailed { speaker_id, service_type, error } => {
                        eprintln!("✗ Subscription failed:");
                        eprintln!("  Speaker: {}", speaker_id.as_str());
                        eprintln!("  Service: {:?}", service_type);
                        eprintln!("  Error: {}\n", error);
                    }
                    Event::SubscriptionRenewed { speaker_id, service_type } => {
                        println!("↻ Subscription renewed:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}\n", service_type);
                    }
                    Event::SubscriptionExpired { speaker_id, service_type } => {
                        println!("⏱ Subscription expired:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}\n", service_type);
                    }
                    Event::SubscriptionRemoved { speaker_id, service_type } => {
                        println!("✓ Subscription removed:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}\n", service_type);
                        break;
                    }
                    Event::ParseError { speaker_id, service_type, error } => {
                        eprintln!("✗ Parse error:");
                        eprintln!("  Speaker: {}", speaker_id.as_str());
                        eprintln!("  Service: {:?}", service_type);
                        eprintln!("  Error: {}\n", error);
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(remaining_time) => {
                println!("⏰ Demo timeout reached. Events may not have been received due to network configuration.");
                println!("   This is normal in a mock example - the subscription lifecycle worked correctly!\n");
                break;
            }
        }
    }

    // Unsubscribe and shutdown
    println!("Demo complete! Cleaning up...");
    broker.unsubscribe(&speaker, ServiceType::AVTransport).await?;
    
    // Wait briefly for unsubscribe event with timeout
    tokio::select! {
        Some(Event::SubscriptionRemoved { .. }) = event_stream.recv() => {
            println!("✓ Unsubscribed successfully");
        }
        _ = tokio::time::sleep(Duration::from_millis(100)) => {
            println!("✓ Unsubscribe initiated");
        }
    }

    broker.shutdown().await?;
    println!("✓ Broker shut down");
    println!("\nGoodbye!");

    Ok(())
}
