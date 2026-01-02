//! Async real-time processing example
//!
//! This example demonstrates the async interface for real-time event processing
//! when you need to perform async operations in response to events or handle
//! multiple event streams concurrently.

use sonos_stream::{
    BrokerConfig, EventBroker, EventData, PollingReason,
    events::types::{AVTransportDelta, RenderingControlDelta, ResyncReason}
};
use sonos_api::{SonosClient, ServiceType};
use std::net::IpAddr;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎵 Sonos Stream - Async Real-time Processing Example");
    println!("===================================================");

    // Create broker with fast polling configuration for demonstration
    let config = BrokerConfig::fast_polling();
    let mut broker = EventBroker::new(config).await?;

    // Example device IP - replace with your actual Sonos device
    let device_ip: IpAddr = "192.168.1.100".parse()?;

    println!("\n📋 Registering Sonos services...");

    // Register multiple services
    let transport_reg = broker.register_speaker_service(device_ip, ServiceType::AVTransport).await?;
    let volume_reg = broker.register_speaker_service(device_ip, ServiceType::RenderingControl).await?;

    println!("✅ Services registered with IDs: {} and {}",
             transport_reg.registration_id, volume_reg.registration_id);

    // Print firewall status
    println!("🔍 Firewall Status: {:?}", transport_reg.firewall_status);

    println!("\n🚀 ASYNC PATTERN: Starting real-time event processing");
    println!("This pattern is best for:");
    println!("  • Performing async operations in response to events");
    println!("  • Real-time notifications or streaming to other services");
    println!("  • When you need concurrent processing of events");
    println!();

    // Get event iterator for async processing
    let mut events = broker.event_iterator()?;
    let mut event_count = 0;
    let max_events = 20;

    // ASYNC PATTERN - Process events in real-time
    loop {
        // Use timeout to prevent hanging if no events arrive
        match timeout(Duration::from_secs(30), events.next_async()).await {
            Ok(Some(event)) => {
                event_count += 1;

                println!("📨 Event #{} received at {:?}",
                         event_count, event.timestamp);
                println!("   Device: {} | Service: {:?} | Source: {}",
                         event.speaker_ip,
                         event.service,
                         format_event_source(&event.event_source));

                // Process different event types with async operations
                match event.event_data {
                    EventData::AVTransportChange(delta) => {
                        handle_transport_change_async(event.speaker_ip, delta).await;
                    }
                    EventData::RenderingControlChange(delta) => {
                        handle_volume_change_async(event.speaker_ip, delta).await;
                    }
                    EventData::AVTransportResync(full_state) => {
                        handle_transport_resync_async(event.speaker_ip, full_state, &event.event_source).await;
                    }
                    EventData::RenderingControlResync(full_state) => {
                        handle_volume_resync_async(event.speaker_ip, full_state, &event.event_source).await;
                    }
                }

                println!();

                // Stop after max events for demonstration
                if event_count >= max_events {
                    println!("📋 Processed {} events, stopping demonstration", event_count);
                    break;
                }
            }
            Ok(None) => {
                println!("📡 Event stream closed");
                break;
            }
            Err(_) => {
                println!("⏰ No events received in 30 seconds");
                println!("💡 Try changing volume or playing/pausing on your Sonos device");

                // Continue waiting for events
                continue;
            }
        }
    }

    // Demonstrate additional async iterator features
    println!("\n🔍 Additional Async Iterator Features:");
    demonstrate_async_features(&mut events).await;

    println!("\n🛑 Shutting down EventBroker...");
    broker.shutdown().await?;
    println!("✅ Async example completed successfully!");

    Ok(())
}

/// Handle transport changes asynchronously
async fn handle_transport_change_async(device_ip: IpAddr, delta: AVTransportDelta) {
    println!("🎵 Processing transport change asynchronously...");

    if let Some(ref state) = delta.transport_state {
        match state.as_str() {
            "PLAYING" => {
                println!("   ▶️  Playback started on {}", device_ip);
                // Example: Send notification to external service
                simulate_external_notification("play_started", device_ip).await;
            }
            "PAUSED_PLAYBACK" => {
                println!("   ⏸️  Playback paused on {}", device_ip);
                simulate_external_notification("play_paused", device_ip).await;
            }
            "STOPPED" => {
                println!("   ⏹️  Playback stopped on {}", device_ip);
                simulate_external_notification("play_stopped", device_ip).await;
            }
            _ => {
                println!("   🔄 Transport state: {} on {}", state, device_ip);
            }
        }
    }

    if let Some(ref track_uri) = delta.current_track_uri {
        println!("   🎵 Track changed to: {}", track_uri);
        // Example: Update external playlist or database
        simulate_track_update(device_ip, track_uri).await;
    }

    if let Some(ref position) = delta.rel_time {
        println!("   ⏱️  Position updated: {}", position);
    }
}

/// Handle volume changes asynchronously
async fn handle_volume_change_async(device_ip: IpAddr, delta: RenderingControlDelta) {
    println!("🔊 Processing volume change asynchronously...");

    if let Some(volume) = delta.volume {
        println!("   🎚️  Volume changed to {} on {}", volume, device_ip);

        // Example: Adjust related systems based on volume
        if volume == 0 {
            simulate_external_notification("volume_muted", device_ip).await;
        } else if volume > 75 {
            simulate_external_notification("volume_high", device_ip).await;
        }
    }

    if let Some(mute) = delta.mute {
        println!("   🔇 Mute {} on {}", if mute { "enabled" } else { "disabled" }, device_ip);
        if mute {
            simulate_external_notification("device_muted", device_ip).await;
        }
    }
}

/// Handle transport resync events asynchronously
async fn handle_transport_resync_async(
    device_ip: IpAddr,
    full_state: sonos_stream::events::types::AVTransportFullState,
    source: &sonos_stream::events::types::EventSource
) {
    println!("🔄 Processing transport resync asynchronously...");
    println!("   Reason: {}", format_resync_reason(source));
    println!("   Device: {}", device_ip);

    // In a real application, you would:
    // 1. Update your complete local state with the full_state
    // 2. Potentially notify other systems about the resync
    // 3. Log the resync event for monitoring

    simulate_resync_notification(device_ip, "transport").await;
}

/// Handle volume resync events asynchronously
async fn handle_volume_resync_async(
    device_ip: IpAddr,
    full_state: sonos_stream::events::types::RenderingControlFullState,
    source: &sonos_stream::events::types::EventSource
) {
    println!("🔊 Processing volume resync asynchronously...");
    println!("   Reason: {}", format_resync_reason(source));
    println!("   Device: {}", device_ip);

    simulate_resync_notification(device_ip, "volume").await;
}

/// Demonstrate additional async iterator features
async fn demonstrate_async_features(events: &mut sonos_stream::events::iterator::EventIterator) {
    println!("Testing try_next() (non-blocking):");

    match events.try_next() {
        Ok(Some(event)) => {
            println!("  📨 Found buffered event: {} {:?}",
                     event.speaker_ip, event.service);
        }
        Ok(None) => {
            println!("  📭 No events immediately available");
        }
        Err(e) => {
            println!("  ❌ Error: {}", e);
        }
    }

    println!("\nTesting next_timeout() with 2-second timeout:");
    match events.next_timeout(Duration::from_secs(2)).await {
        Ok(Some(event)) => {
            println!("  📨 Received event within timeout: {} {:?}",
                     event.speaker_ip, event.service);
        }
        Ok(None) => {
            println!("  📡 Event stream ended");
        }
        Err(_) => {
            println!("  ⏰ Timeout - no events within 2 seconds");
        }
    }

    // Show iterator statistics
    let stats = events.stats();
    println!("\nIterator Statistics:");
    println!("  Events received: {}", stats.events_received);
    println!("  Events delivered: {}", stats.events_delivered);
    println!("  Resync events: {}", stats.resync_events_emitted);
    println!("  Timeouts: {}", stats.timeouts);
    println!("  Delivery rate: {:.1}%", stats.delivery_rate() * 100.0);
}

/// Simulate sending a notification to an external service
async fn simulate_external_notification(event_type: &str, device_ip: IpAddr) {
    // Simulate async operation (e.g., HTTP request to webhook)
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("   📤 Sent '{}' notification for device {}", event_type, device_ip);
}

/// Simulate updating track information in external service
async fn simulate_track_update(device_ip: IpAddr, track_uri: &str) {
    // Simulate async database update
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!("   💾 Updated track database: {} -> {}", device_ip, track_uri);
}

/// Simulate resync notification to monitoring system
async fn simulate_resync_notification(device_ip: IpAddr, component: &str) {
    // Simulate async monitoring notification
    tokio::time::sleep(Duration::from_millis(75)).await;
    println!("   🔄 Sent resync notification: {} {} resynced", device_ip, component);
}

/// Format event source for display
fn format_event_source(source: &sonos_stream::events::types::EventSource) -> String {
    use sonos_stream::events::types::EventSource;

    match source {
        EventSource::UPnPNotification { subscription_id } => {
            format!("UPnP Event (SID: {}...)", &subscription_id[..8])
        }
        EventSource::PollingDetection { poll_interval } => {
            format!("Polling ({}s)", poll_interval.as_secs())
        }
        EventSource::ResyncDetection { reason } => {
            format!("Resync ({})", format_resync_reason_enum(reason))
        }
    }
}

/// Format resync reason for display
fn format_resync_reason(source: &sonos_stream::events::types::EventSource) -> String {
    use sonos_stream::events::types::EventSource;

    match source {
        EventSource::ResyncDetection { reason } => format_resync_reason_enum(reason),
        _ => "not a resync event".to_string(),
    }
}

/// Format resync reason enum for display
fn format_resync_reason_enum(reason: &ResyncReason) -> String {
    match reason {
        ResyncReason::EventTimeoutDetected => "event timeout detected".to_string(),
        ResyncReason::PollingDiscrepancy => "polling found different state".to_string(),
        ResyncReason::SubscriptionRenewal => "subscription was renewed".to_string(),
        ResyncReason::ExplicitRefresh => "explicit refresh requested".to_string(),
    }
}