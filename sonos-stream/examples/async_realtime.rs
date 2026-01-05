//! Async real-time processing example
//!
//! This example demonstrates the async interface for real-time event processing
//! when you need to perform async operations in response to events or handle
//! multiple event streams concurrently.

use sonos_stream::{
    BrokerConfig, EventBroker, EventData,
};
use sonos_api::Service;
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
    let transport_reg = broker.register_speaker_service(device_ip, Service::AVTransport).await?;
    let volume_reg = broker.register_speaker_service(device_ip, Service::RenderingControl).await?;

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
                    EventData::AVTransportEvent(transport_event) => {
                        handle_transport_event_async(event.speaker_ip, transport_event).await;
                    }
                    EventData::RenderingControlEvent(volume_event) => {
                        handle_volume_event_async(event.speaker_ip, volume_event).await;
                    }
                    EventData::ZoneGroupTopologyEvent(topology) => {
                        handle_topology_change_async(event.speaker_ip, topology).await;
                    }
                    EventData::DevicePropertiesEvent(device_event) => {
                        handle_device_properties_async(event.speaker_ip, device_event).await;
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

/// Handle transport events asynchronously
async fn handle_transport_event_async(device_ip: IpAddr, transport_event: sonos_stream::events::types::AVTransportEvent) {
    println!("🎵 Processing transport event asynchronously...");

    if let Some(ref state) = transport_event.transport_state {
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

    if let Some(ref track_uri) = transport_event.current_track_uri {
        println!("   🎵 Track changed to: {}", track_uri);
        // Example: Update external playlist or database
        simulate_track_update(device_ip, track_uri).await;
    }

    if let Some(ref position) = transport_event.rel_time {
        println!("   ⏱️  Position updated: {}", position);
    }

    if let Some(ref metadata) = transport_event.track_metadata {
        println!("   📄 Track metadata available ({} chars)", metadata.len());
    }
}

/// Handle volume events asynchronously
async fn handle_volume_event_async(device_ip: IpAddr, volume_event: sonos_stream::events::types::RenderingControlEvent) {
    println!("🔊 Processing volume event asynchronously...");

    if let Some(ref volume_str) = volume_event.master_volume {
        if let Ok(volume) = volume_str.parse::<u16>() {
            println!("   🎚️  Volume changed to {} on {}", volume, device_ip);

            // Example: Adjust related systems based on volume
            if volume == 0 {
                simulate_external_notification("volume_muted", device_ip).await;
            } else if volume > 75 {
                simulate_external_notification("volume_high", device_ip).await;
            }
        }
    }

    if let Some(ref mute_str) = volume_event.master_mute {
        let mute = mute_str == "1" || mute_str.to_lowercase() == "true";
        println!("   🔇 Mute {} on {}", if mute { "enabled" } else { "disabled" }, device_ip);
        if mute {
            simulate_external_notification("device_muted", device_ip).await;
        }
    }

    if let Some(ref bass) = volume_event.bass {
        println!("   🎵 Bass level: {}", bass);
    }

    if let Some(ref treble) = volume_event.treble {
        println!("   🎶 Treble level: {}", treble);
    }
}

/// Handle topology changes asynchronously
async fn handle_topology_change_async(
    device_ip: IpAddr,
    topology: sonos_stream::events::types::ZoneGroupTopologyEvent
) {
    println!("🏠 Processing topology change asynchronously...");
    println!("   📡 Received from: {}", device_ip);
    println!("   🔢 Zone groups: {}", topology.zone_groups.len());

    // Analyze topology changes
    let total_speakers = topology.zone_groups.iter()
        .map(|group| group.members.len() + group.members.iter().map(|m| m.satellites.len()).sum::<usize>())
        .sum::<usize>();

    println!("   📊 Total speakers in household: {}", total_speakers);

    // Process each zone group
    for (i, group) in topology.zone_groups.iter().enumerate() {
        println!("   🏠 Group {}: {} ({} members)",
                 i + 1, group.coordinator, group.members.len());

        // Check for multi-room groups
        if group.members.len() > 1 {
            let zone_names: Vec<&str> = group.members.iter()
                .map(|m| m.zone_name.as_str())
                .collect();
            println!("      🔗 Multi-room group: {}", zone_names.join(" + "));

            // Example: Notify external system about multi-room group
            simulate_external_notification("multiroom_group_detected", device_ip).await;
        }

        // Check for home theater setups
        for member in &group.members {
            if !member.satellites.is_empty() {
                println!("      🎭 Home theater setup detected in {}: {} satellites",
                         member.zone_name, member.satellites.len());
                simulate_external_notification("hometheater_detected", device_ip).await;
            }

            // Check network configuration
            if member.network_info.wifi_enabled == "1" {
                println!("      📶 {} using WiFi on {}MHz",
                         member.zone_name, member.network_info.channel_freq);
            } else if member.network_info.eth_link == "1" {
                println!("      🔌 {} using Ethernet", member.zone_name);
            }
        }
    }

    // Example: Update external topology database
    simulate_topology_update(device_ip, &topology).await;
}

/// Handle device properties events asynchronously
async fn handle_device_properties_async(
    device_ip: IpAddr,
    device_event: sonos_stream::events::types::DevicePropertiesEvent
) {
    println!("⚙️  Processing device properties event asynchronously...");
    println!("   Device: {}", device_ip);

    if let Some(ref zone_name) = device_event.zone_name {
        println!("   📍 Zone name: {}", zone_name);
        // Example: Update room database
        simulate_external_notification("zone_renamed", device_ip).await;
    }

    if let Some(ref model) = device_event.model_name {
        println!("   📱 Model: {}", model);
    }

    if let Some(ref version) = device_event.software_version {
        println!("   💾 Software version: {}", version);
        // Example: Track firmware updates
        simulate_external_notification("firmware_updated", device_ip).await;
    }

    if let Some(ref config) = device_event.configuration {
        println!("   ⚙️  Configuration: {}", config);
    }

    // Example: Update device registry
    simulate_device_update(device_ip, &device_event).await;
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


/// Simulate topology database update
async fn simulate_topology_update(
    device_ip: IpAddr,
    topology: &sonos_stream::events::types::ZoneGroupTopologyEvent
) {
    // Simulate async database update
    tokio::time::sleep(Duration::from_millis(120)).await;
    println!("   💾 Updated topology database: {} with {} groups",
             device_ip, topology.zone_groups.len());
}

/// Simulate device properties database update
async fn simulate_device_update(
    device_ip: IpAddr,
    device_event: &sonos_stream::events::types::DevicePropertiesEvent
) {
    // Simulate async database update
    tokio::time::sleep(Duration::from_millis(100)).await;
    let properties_count = [
        &device_event.zone_name, &device_event.model_name,
        &device_event.software_version, &device_event.configuration
    ].iter().filter(|prop| prop.is_some()).count();

    println!("   💾 Updated device database: {} with {} properties",
             device_ip, properties_count);
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
    }
}

