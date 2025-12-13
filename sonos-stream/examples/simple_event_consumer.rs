//! Minimal example showing how to consume events from sonos-stream.
//!
//! This example demonstrates the complete event consumption pattern:
//! 1. Creates a broker with a simple mock strategy
//! 2. Subscribes to a mock speaker
//! 3. Simulates receiving events by sending HTTP requests to the callback server
//! 4. Parses and prints events to the terminal
//! 5. Cleans up and exits gracefully
//!
//! The mock strategy uses simple XML parsing (just looks for "PLAYING" or "PAUSED")
//! and the events are simulated by spawning a background task that POSTs to the
//! callback server, mimicking how real Sonos speakers would send notifications.
//!
//! Run with: cargo run --example simple_event_consumer

use sonos_stream::{
    EventBrokerBuilder, Event, ServiceType, Speaker, SpeakerId, 
    SubscriptionStrategy, Subscription, SubscriptionScope,
    StrategyError, SubscriptionError, ParsedEvent,
};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;
use async_trait::async_trait;

/// Simple mock strategy that returns hardcoded events
struct SimpleStrategy;

impl SubscriptionStrategy for SimpleStrategy {
    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }

    fn subscription_scope(&self) -> SubscriptionScope {
        SubscriptionScope::PerSpeaker
    }

    fn service_endpoint_path(&self) -> &'static str {
        "/MediaRenderer/AVTransport/Event"
    }

    fn parse_event(
        &self,
        speaker_id: &SpeakerId,
        event_xml: &str,
    ) -> Result<Vec<ParsedEvent>, StrategyError> {
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

        Ok(vec![ParsedEvent::custom(event_type, data)])
    }
}

/// Simple mock subscription
struct SimpleSubscription {
    id: String,
    speaker_id: SpeakerId,
}

#[async_trait]
impl Subscription for SimpleSubscription {
    fn subscription_id(&self) -> &str {
        &self.id
    }

    async fn renew(&mut self) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn unsubscribe(&mut self) -> Result<(), SubscriptionError> {
        Ok(())
    }

    fn is_active(&self) -> bool {
        true
    }

    fn time_until_renewal(&self) -> Option<Duration> {
        None
    }

    fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::AVTransport
    }
}

/// Simulate sending events to the callback server
async fn simulate_events(callback_url: String, subscription_id: String) {
    // Wait a bit for subscription to be established
    tokio::time::sleep(Duration::from_millis(200)).await;

    let client = reqwest::Client::new();
    
    // Simulate a series of events
    let events = vec![
        ("<event><PLAYING>true</PLAYING></event>", "Playing music"),
        ("<event><PAUSED>true</PAUSED></event>", "Paused"),
        ("<event><PLAYING>true</PLAYING></event>", "Resumed playing"),
    ];

    for (xml, description) in events {
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        println!("📤 Simulating event: {}", description);
        
        // The callback server expects /notify/{subscription_id}
        let url = format!("{}/notify/{}", callback_url, subscription_id);
        let result = client
            .post(&url)
            .header("SID", format!("uuid:{}", subscription_id))
            .header("NT", "upnp:event")
            .header("NTS", "upnp:propchange")
            .header("Content-Type", "text/xml")
            .body(xml.to_string())
            .send()
            .await;
            
        if let Err(e) = result {
            eprintln!("Failed to send event: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting simple event consumer example...\n");

    // Create broker with our simple strategy
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(SimpleStrategy))
        .with_port_range(3400, 3500)
        .build()
        .await?;

    // Get the callback URL from the broker
    let callback_url = format!("http://127.0.0.1:3400");

    // Create a mock speaker
    let speaker = Speaker::new(
        SpeakerId::new("RINCON_EXAMPLE123"),
        "192.168.1.100".parse::<IpAddr>()?,
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

    // Spawn task to simulate events
    let sim_url = callback_url.clone();
    let sim_sub_id = subscription_id.clone();
    tokio::spawn(async move {
        simulate_events(sim_url, sim_sub_id).await;
    });

    // Process events
    let mut event_count = 0;
    let max_events = 3; // Wait for 3 service events then exit

    loop {
        tokio::select! {
            Some(event) = event_stream.recv() => {
                match event {
                    Event::ServiceEvent { speaker_id, service_type, event } => {
                        println!("→ Event received:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}", service_type);
                        println!("  Type: {}", event.event_type());
                        println!("  Data: {:?}\n", event.data());
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
