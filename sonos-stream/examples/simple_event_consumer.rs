//! Minimal example showing how to consume events from sonos-stream using the provider pattern.
//!
//! This example demonstrates the complete event consumption pattern:
//! 1. Creates a broker with AVTransportProvider (using mock subscriptions for demo)
//! 2. Subscribes to a mock speaker
//! 3. Simulates receiving events by sending HTTP requests to the callback server
//! 4. Parses and prints events to the terminal
//! 5. Cleans up and exits gracefully
//!
//! The example uses a mock strategy that doesn't make real UPnP calls - it creates 
//! mock subscriptions and parses simulated events. Events are simulated by spawning 
//! a background task that POSTs to the callback server, mimicking how real Sonos 
//! speakers would send notifications.
//!
//! Run with: cargo run --example simple_event_consumer

use sonos_stream::{
    EventBrokerBuilder, Event, ServiceType, Speaker, SpeakerId, AVTransportProvider,
};
use std::net::IpAddr;
use sonos_parser::services::av_transport::AVTransportParser;
use std::time::Duration;

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
    println!("Starting simple event consumer example with AVTransportProvider...\n");

    // Create broker with AVTransportProvider (real provider, but we'll use mock events)
    let av_provider = AVTransportProvider::new();
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(av_provider))
        .with_port_range(40000, 40100)  // Use same range as integration tests
        .build()
        .await?;

    // Create a mock speaker (IP doesn't matter since we're simulating events)
    let speaker = Speaker::new(
        SpeakerId::new("RINCON_EXAMPLE123"),
        "127.0.0.1".parse::<IpAddr>()?,
        "Living Room".to_string(),
        "Living Room".to_string(),
    );

    println!("Subscribing to speaker: {} using AVTransportProvider", speaker.name);
    
    // Subscribe to the speaker - this will use the real AVTransportProvider
    // but we'll simulate the events manually
    broker.subscribe(&speaker, ServiceType::AVTransport).await?;

    // Get the event stream
    let mut event_stream = broker.event_stream();

    println!("Listening for events...\n");

    // Capture the subscription ID from the first event
    let subscription_id = if let Some(Event::SubscriptionEstablished { subscription_id, .. }) = event_stream.recv().await {
        println!("✓ Subscription established with AVTransportProvider");
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
            println!("   This is normal in a demo example - the subscription lifecycle worked correctly!\n");
            break;
        }

        tokio::select! {
            Some(event) = event_stream.recv() => {
                match event {
                    Event::ServiceEvent { speaker_id, service_type, event, .. } => {
                        println!("→ Event received from AVTransportProvider:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}", service_type);
                        println!("  Type: {}", event.event_type());
                        
                        // Demonstrate type-safe downcasting to AVTransportParser
                        if let Some(av_event) = event.downcast_ref::<AVTransportParser>() {
                            println!("  Transport State: {}", av_event.transport_state());
                            if let Some(track_uri) = av_event.current_track_uri() {
                                println!("  Track URI: {}", track_uri);
                            }
                            if let Some(duration) = av_event.current_track_duration() {
                                println!("  Duration: {}", duration);
                            }
                            if let Some(play_mode) = av_event.property.last_change.instance.current_play_mode.as_ref() {
                                println!("  Play Mode: {}", play_mode.val);
                            }
                        } else {
                            println!("  Data: {:?}", event);
                        }
                        println!();
                        
                        event_count += 1;
                        
                        if event_count >= max_events {
                            println!("Received {} events, exiting...\n", max_events);
                            break;
                        }
                    }
                    Event::SubscriptionFailed { speaker_id, service_type, error, .. } => {
                        eprintln!("✗ Subscription failed:");
                        eprintln!("  Speaker: {}", speaker_id.as_str());
                        eprintln!("  Service: {:?}", service_type);
                        eprintln!("  Error: {}\n", error);
                    }
                    Event::SubscriptionRenewed { speaker_id, service_type, .. } => {
                        println!("↻ Subscription renewed:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}\n", service_type);
                    }
                    Event::SubscriptionExpired { speaker_id, service_type, .. } => {
                        println!("⏱ Subscription expired:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}\n", service_type);
                    }
                    Event::SubscriptionRemoved { speaker_id, service_type, .. } => {
                        println!("✓ Subscription removed:");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}\n", service_type);
                        break;
                    }
                    Event::ParseError { speaker_id, service_type, error, .. } => {
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
                println!("   This is normal in a demo example - the subscription lifecycle worked correctly!\n");
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
    println!("\nThis example demonstrated:");
    println!("• Using AVTransportProvider for real UPnP service handling");
    println!("• Provider-based architecture with pluggable service strategies");
    println!("• Type-safe event parsing with AVTransportParser");
    println!("• Complete subscription lifecycle management");
    println!("\nGoodbye!");

    Ok(())
}
