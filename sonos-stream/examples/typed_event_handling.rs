//! Advanced example demonstrating type-safe event handling with the provider pattern.
//!
//! This example shows how to:
//! 1. Use AVTransportProvider for real service strategy implementation
//! 2. Handle multiple event types safely using downcasting
//! 3. Create custom event processors for different service types
//! 4. Implement event filtering and routing based on event types
//! 5. Demonstrate error handling for invalid downcasts
//!
//! Run with: cargo run --example typed_event_handling

use sonos_stream::{
    EventBrokerBuilder, Event, ServiceType, Speaker, SpeakerId, AVTransportProvider, TypedEvent,
};
use std::net::IpAddr;
use sonos_parser::services::av_transport::AVTransportParser;
use std::time::Duration;

/// Event processor that handles different event types with type safety
struct TypedEventProcessor {
    av_transport_count: u32,
    unknown_count: u32,
}

impl TypedEventProcessor {
    fn new() -> Self {
        Self {
            av_transport_count: 0,
            unknown_count: 0,
        }
    }

    /// Process a typed event with type-safe downcasting
    fn process_event(&mut self, speaker_id: &SpeakerId, event: &TypedEvent) {
        println!("Processing event from speaker: {}", speaker_id.as_str());
        println!("  Event type: {}", event.event_type());
        println!("  Service type: {:?}", event.service_type());

        // Demonstrate type-safe event handling
        match event.service_type() {
            ServiceType::AVTransport => {
                self.process_av_transport_event(event);
                self.av_transport_count += 1;
            }
            ServiceType::RenderingControl => {
                self.process_rendering_control_event(event);
            }
            ServiceType::ZoneGroupTopology => {
                self.process_zone_group_event(event);
            }
        }
        println!();
    }

    /// Process AVTransport events with full type safety
    fn process_av_transport_event(&self, event: &TypedEvent) {
        if let Some(av_event) = event.downcast_ref::<AVTransportParser>() {
            println!("  ✓ Successfully downcast to AVTransportParser");
            println!("    Transport State: {}", av_event.transport_state());
            
            // Demonstrate accessing optional fields safely
            if let Some(track_uri) = av_event.current_track_uri() {
                println!("    Track URI: {}", track_uri);
            }
            
            if let Some(duration) = av_event.current_track_duration() {
                println!("    Duration: {}", duration);
            }
            
            if let Some(track_num) = av_event.property.last_change.instance.current_track.as_ref().and_then(|v| v.val.parse::<u32>().ok()) {
                if let Some(total_tracks) = av_event.property.last_change.instance.number_of_tracks.as_ref().and_then(|v| v.val.parse::<u32>().ok()) {
                    println!("    Track: {} of {}", track_num, total_tracks);
                }
            }
            
            if let Some(play_mode) = av_event.property.last_change.instance.current_play_mode.as_ref() {
                println!("    Play Mode: {}", play_mode.val);
            }

            // Demonstrate conditional logic based on transport state
            match av_event.transport_state() {
                "PLAYING" => println!("    → Music is currently playing"),
                "PAUSED_PLAYBACK" => println!("    → Music is paused"),
                "STOPPED" => println!("    → Playback is stopped"),
                "TRANSITIONING" => println!("    → Transport state is changing"),
                _ => println!("    → Unknown transport state: {}", av_event.transport_state()),
            }
        } else {
            println!("  ✗ Failed to downcast AVTransport event - unexpected event type");
        }
    }

    /// Process RenderingControl events (would need RenderingControlEvent type)
    fn process_rendering_control_event(&self, event: &TypedEvent) {
        println!("  RenderingControl event processing (type-safe implementation would go here)");
        println!("    Event type: {}", event.event_type());
        // In a real implementation, we would downcast to RenderingControlEvent
        // if let Some(rc_event) = event.downcast_ref::<RenderingControlEvent>() { ... }
    }

    /// Process ZoneGroupTopology events (would need ZoneGroupTopologyEvent type)
    fn process_zone_group_event(&self, event: &TypedEvent) {
        println!("  ZoneGroupTopology event processing (type-safe implementation would go here)");
        println!("    Event type: {}", event.event_type());
        // In a real implementation, we would downcast to ZoneGroupTopologyEvent
        // if let Some(zgt_event) = event.downcast_ref::<ZoneGroupTopologyEvent>() { ... }
    }

    /// Get processing statistics
    fn get_stats(&self) -> (u32, u32) {
        (self.av_transport_count, self.unknown_count)
    }
}

/// Simulate sending different types of events
async fn simulate_typed_events(callback_url: String, subscription_id: String) {
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    
    let events = vec![
        ("<event><PLAYING>true</PLAYING></event>", "Started playing"),
        ("<event><PAUSED>true</PAUSED></event>", "Paused playback"),
        ("<event><PLAYING>true</PLAYING></event>", "Resumed playing"),
        ("<event><STOPPED>true</STOPPED></event>", "Stopped playback"),
    ];

    for (xml, description) in events {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        
        println!("📤 Simulating event: {}", description);
        
        let url = format!("{}/notify/{}", callback_url, subscription_id);
        let result = client
            .post(&url)
            .header("SID", subscription_id.clone())
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
    println!("Starting typed event handling example with AVTransportProvider...\n");

    // Create broker with AVTransportProvider (real provider implementation)
    let av_provider = AVTransportProvider::new();
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(av_provider))
        .with_port_range(41000, 41100)
        .build()
        .await?;

    // Create a test speaker
    let speaker = Speaker::new(
        SpeakerId::new("RINCON_TYPED_EXAMPLE"),
        "127.0.0.1".parse::<IpAddr>()?,
        "Typed Event Speaker".to_string(),
        "Demo Room".to_string(),
    );

    println!("Subscribing to speaker: {} using AVTransportProvider", speaker.name);
    
    // Subscribe to the speaker using the real AVTransportProvider
    broker.subscribe(&speaker, ServiceType::AVTransport).await?;

    // Get the event stream
    let mut event_stream = broker.event_stream();

    // Create event processor
    let mut processor = TypedEventProcessor::new();

    println!("Listening for typed events...\n");

    // Capture the subscription ID from the first event
    let subscription_id = if let Some(Event::SubscriptionEstablished { subscription_id, .. }) = event_stream.recv().await {
        println!("✓ Subscription established with AVTransportProvider");
        println!("  ID: {}\n", subscription_id);
        subscription_id
    } else {
        return Err("Failed to establish subscription".into());
    };

    let callback_url = format!("http://127.0.0.1:41000");

    // Spawn task to simulate events
    let sim_url = callback_url.clone();
    let sim_sub_id = subscription_id.clone();
    tokio::spawn(async move {
        simulate_typed_events(sim_url, sim_sub_id).await;
    });

    // Process events with timeout
    let mut event_count = 0;
    let max_events = 4; // Wait for 4 service events then exit
    let timeout_duration = Duration::from_secs(15);

    println!("Processing typed events (timeout: {}s)...\n", timeout_duration.as_secs());

    let start_time = std::time::Instant::now();

    loop {
        let remaining_time = timeout_duration.saturating_sub(start_time.elapsed());
        if remaining_time.is_zero() {
            println!("⏰ Demo timeout reached.");
            break;
        }

        tokio::select! {
            Some(event) = event_stream.recv() => {
                match event {
                    Event::ServiceEvent { speaker_id, service_type: _, event, .. } => {
                        processor.process_event(&speaker_id, &event);
                        event_count += 1;
                        
                        if event_count >= max_events {
                            println!("Processed {} events, exiting...\n", max_events);
                            break;
                        }
                    }
                    Event::ParseError { speaker_id, service_type, error, .. } => {
                        println!("✗ Parse error (demonstrating error handling):");
                        println!("  Speaker: {}", speaker_id.as_str());
                        println!("  Service: {:?}", service_type);
                        println!("  Error: {}\n", error);
                    }
                    _ => {
                        // Handle other event types as needed
                    }
                }
            }
            _ = tokio::time::sleep(remaining_time) => {
                println!("⏰ Demo timeout reached.");
                break;
            }
        }
    }

    // Show processing statistics
    let (av_count, unknown_count) = processor.get_stats();
    println!("Processing Statistics:");
    println!("  AVTransport events: {}", av_count);
    println!("  Unknown events: {}", unknown_count);
    println!();

    // Cleanup
    println!("Demo complete! Cleaning up...");
    broker.unsubscribe(&speaker, ServiceType::AVTransport).await?;
    
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
    println!("• Type-safe event downcasting with AVTransportParser from AVTransportProvider");
    println!("• Conditional processing based on event and service types");
    println!("• Safe handling of optional fields in typed events");
    println!("• Error handling for unsupported event types");
    println!("• Event processing statistics and monitoring");
    println!("• Using real provider implementations instead of mock strategies");

    Ok(())
}