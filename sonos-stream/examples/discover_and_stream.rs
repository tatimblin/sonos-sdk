//! Complete example showing the new sonos-sdk architecture.
//!
//! This example demonstrates how to use the three main crates together:
//! 1. sonos-discovery: Find all Sonos speakers on the network
//! 2. sonos-api: Create managed subscriptions for AVTransport service
//! 3. sonos-stream: Process events from all speakers in a unified stream
//!
//! The example shows the separation of concerns:
//! - Discovery handles finding devices
//! - API handles subscription management and device control
//! - Stream handles event processing and callback server
//!
//! Run with: cargo run --example discover_and_stream

use sonos_discovery::get_with_timeout;
use sonos_api::{SonosClient, Service};
use sonos_stream::{EventBrokerBuilder, Event, AVTransportProvider};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎵 Sonos SDK Example: Discover and Stream Events");
    println!("================================================");
    
    // Step 1: Set up the event broker for streaming
    println!("\n📡 Setting up event streaming...");
    let mut broker = EventBrokerBuilder::new()
        .with_strategy(Box::new(AVTransportProvider::new()))
        .with_port_range(3400, 3500)
        .build()
        .await?;
    
    let callback_url = broker.callback_url();
    println!("✅ Event broker ready, callback URL: {}", callback_url);
    
    // Get the event stream
    let mut event_stream = broker.event_stream();
    
    // Step 2: Discover Sonos speakers on the network
    println!("\n🔍 Discovering Sonos speakers...");
    let discovery_timeout = Duration::from_secs(10);
    println!("This may take up to {} seconds...", discovery_timeout.as_secs());
    
    let speakers = tokio::task::spawn_blocking(move || {
        get_with_timeout(discovery_timeout)
    }).await.map_err(|e| format!("Task join error: {}", e))?;
    
    if speakers.is_empty() {
        println!("❌ No Sonos speakers found on the network");
        println!("💡 Make sure you have Sonos speakers powered on and connected to the same network");
        return Ok(());
    }
    
    println!("✅ Found {} Sonos speaker(s):", speakers.len());
    for device in &speakers {
        println!("   📻 {} ({}) at {}", device.name, device.room_name, device.ip_address);
    }
    
    // Step 3: Create API client and subscriptions
    println!("\n🔗 Creating subscriptions for AVTransport service...");
    let client = SonosClient::new();
    let mut subscriptions = HashMap::new();
    
    for speaker in &speakers {
        match client.create_managed_subscription(
            &speaker.ip_address,
            Service::AVTransport,
            &callback_url,
            1800, // 30 minutes
        ) {
            Ok(subscription) => {
                println!("✅ Subscribed to {} ({})", speaker.name, speaker.ip_address);
                subscriptions.insert(speaker.ip_address.clone(), subscription);
            }
            Err(e) => {
                println!("⚠️  Failed to subscribe to {} ({}): {}", speaker.name, speaker.ip_address, e);
            }
        }
    }
    
    if subscriptions.is_empty() {
        println!("❌ No subscriptions were created successfully");
        return Ok(());
    }
    
    println!("✅ Created {} subscription(s)", subscriptions.len());
    
    // Step 4: Process events from all speakers
    println!("\n🎧 Listening for events from all speakers...");
    println!("💡 Try playing, pausing, or changing tracks on your Sonos speakers");
    println!("⏹️  Press Ctrl+C to stop\n");
    
    let mut event_count = 0;
    let max_events = 50; // Limit for demo purposes
    
    // Set up graceful shutdown
    let shutdown_duration = Duration::from_secs(60); // Run for 1 minute max
    let start_time = std::time::Instant::now();
    
    loop {
        // Check if we should stop
        if event_count >= max_events {
            println!("\n📊 Reached maximum event count ({}), stopping...", max_events);
            break;
        }
        
        if start_time.elapsed() > shutdown_duration {
            println!("\n⏰ Time limit reached ({}s), stopping...", shutdown_duration.as_secs());
            break;
        }
        
        // Wait for events with timeout
        match timeout(Duration::from_secs(5), event_stream.recv()).await {
            Ok(Some(event)) => {
                event_count += 1;
                handle_event(event, &speakers);
            }
            Ok(None) => {
                println!("📡 Event stream closed");
                break;
            }
            Err(_) => {
                // Timeout - check subscription health
                check_subscription_health(&subscriptions);
            }
        }
    }
    
    // Step 5: Clean up
    println!("\n🧹 Cleaning up...");
    
    // Unsubscribe from all services
    for (ip, subscription) in subscriptions {
        if let Err(e) = subscription.unsubscribe() {
            println!("⚠️  Failed to unsubscribe from {}: {}", ip, e);
        } else {
            println!("✅ Unsubscribed from {}", ip);
        }
    }
    
    // Shutdown the broker
    if let Err(e) = broker.shutdown().await {
        println!("⚠️  Failed to shutdown broker: {}", e);
    } else {
        println!("✅ Event broker shutdown complete");
    }
    
    println!("\n📊 Session summary:");
    println!("   🔍 Discovered: {} speakers", speakers.len());
    println!("   📡 Processed: {} events", event_count);
    println!("   ⏱️  Duration: {:.1}s", start_time.elapsed().as_secs_f32());
    
    Ok(())
}

/// Handle different types of events from the stream
fn handle_event(event: Event, speakers: &[sonos_discovery::Device]) {
    match event {
        Event::ServiceEvent { speaker_id, service_type, event, timestamp } => {
            // Find the speaker name for better display
            let speaker_name = speakers
                .iter()
                .find(|s| s.ip_address.contains(&speaker_id.as_str()[7..]))  // Remove "RINCON_" prefix
                .map(|s| s.name.as_str())
                .unwrap_or("Unknown");
            
            println!("🎵 [{}] {} ({:?}): {}", 
                format_timestamp(timestamp),
                speaker_name,
                service_type,
                format_event_data(&event)
            );
        }
        Event::SubscriptionEstablished { speaker_id, service_type, subscription_id, timestamp } => {
            println!("🔗 [{}] Subscription established for {} ({:?}) - ID: {}", 
                format_timestamp(timestamp),
                speaker_id.as_str(),
                service_type,
                subscription_id
            );
        }
        Event::SubscriptionFailed { speaker_id, service_type, error, timestamp } => {
            println!("❌ [{}] Subscription failed for {} ({:?}): {}", 
                format_timestamp(timestamp),
                speaker_id.as_str(),
                service_type,
                error
            );
        }
        Event::SubscriptionRemoved { speaker_id, service_type, timestamp } => {
            println!("🔌 [{}] Subscription removed for {} ({:?})", 
                format_timestamp(timestamp),
                speaker_id.as_str(),
                service_type
            );
        }
        Event::SubscriptionRenewed { speaker_id, service_type, timestamp } => {
            println!("🔄 [{}] Subscription renewed for {} ({:?})", 
                format_timestamp(timestamp),
                speaker_id.as_str(),
                service_type
            );
        }
        Event::SubscriptionExpired { speaker_id, service_type, timestamp } => {
            println!("💀 [{}] Subscription expired for {} ({:?})", 
                format_timestamp(timestamp),
                speaker_id.as_str(),
                service_type
            );
        }
        Event::ParseError { speaker_id, service_type, error, timestamp } => {
            println!("⚠️  [{}] Parse error for {} ({:?}): {}", 
                format_timestamp(timestamp),
                speaker_id.as_str(),
                service_type,
                error
            );
        }
    }
}

/// Format event data for display
fn format_event_data(event: &sonos_stream::TypedEvent) -> String {
    // Try to downcast to known event types from sonos-parser
    format!("Event type: {} (service: {:?})", event.event_type(), event.service_type())
}

/// Format timestamp for display
fn format_timestamp(timestamp: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    
    match timestamp.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let secs = duration.as_secs();
            let millis = duration.subsec_millis();
            format!("{}.{:03}", 
                chrono::DateTime::from_timestamp(secs as i64, 0)
                    .map(|dt| dt.format("%H:%M:%S").to_string())
                    .unwrap_or_else(|| "??:??:??".to_string()),
                millis
            )
        }
        Err(_) => "??:??:??.???".to_string()
    }
}

/// Check the health of subscriptions
fn check_subscription_health(subscriptions: &HashMap<String, sonos_api::ManagedSubscription>) {
    let mut active_count = 0;
    let mut renewal_needed = 0;
    
    for (ip, subscription) in subscriptions {
        if subscription.is_active() {
            active_count += 1;
            if subscription.needs_renewal() {
                renewal_needed += 1;
                println!("🔄 Subscription for {} needs renewal", ip);
                
                // Attempt renewal
                if let Err(e) = subscription.renew() {
                    println!("❌ Failed to renew subscription for {}: {}", ip, e);
                } else {
                    println!("✅ Renewed subscription for {}", ip);
                }
            }
        } else {
            println!("💀 Subscription for {} is inactive", ip);
        }
    }
    
    if active_count == 0 {
        println!("⚠️  No active subscriptions remaining");
    } else {
        println!("📊 Health check: {}/{} subscriptions active, {} need renewal", 
            active_count, subscriptions.len(), renewal_needed);
    }
}