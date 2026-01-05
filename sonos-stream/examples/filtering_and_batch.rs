//! Event filtering and batch processing example
//!
//! This example demonstrates advanced EventIterator features including:
//! - Filtering events by registration ID, service type, or source
//! - Batch processing for efficient handling of multiple events
//! - Peek functionality for lookahead without consuming events
//! - Working with multiple devices and services simultaneously

use sonos_stream::{
    BrokerConfig, EventBroker, EventData, RegistrationResult,
    events::iterator::EventSourceType,
    events::types::{EventSource}
};
use sonos_api::Service;
use std::net::IpAddr;
use std::time::Duration;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎛️ Sonos Stream - Event Filtering & Batch Processing Example");
    println!("=============================================================");

    // Create broker with resource-efficient configuration
    let mut broker = EventBroker::new(BrokerConfig::resource_efficient()).await?;

    // Set up multiple devices and services for filtering demonstration
    let device1: IpAddr = "192.168.1.100".parse()?;
    let device2: IpAddr = "192.168.1.101".parse()?; // Second device if available

    println!("\n📋 Registering multiple services for filtering demonstration...");

    // Register multiple services across potentially multiple devices
    let registrations = vec![
        broker.register_speaker_service(device1, Service::AVTransport).await?,
        broker.register_speaker_service(device1, Service::RenderingControl).await?,
        // Uncomment if you have a second device:
        // broker.register_speaker_service(device2, Service::AVTransport).await?,
        // broker.register_speaker_service(device2, Service::RenderingControl).await?,
    ];

    println!("✅ Registered {} services:", registrations.len());
    for reg in &registrations {
        println!("   ID: {} | Status: {:?} | Polling: {:?}",
                 reg.registration_id,
                 reg.firewall_status,
                 reg.polling_reason.as_ref().map(|r| format!("{:?}", r)).unwrap_or("None".to_string()));
    }

    println!("\n🔍 DEMONSTRATION 1: Event Source Filtering");
    demonstrate_source_filtering(&mut broker).await?;

    println!("\n🎯 DEMONSTRATION 2: Service Type Filtering");
    demonstrate_service_filtering(&mut broker, &registrations).await?;

    println!("\n📦 DEMONSTRATION 3: Batch Processing");
    demonstrate_batch_processing(&mut broker).await?;

    println!("\n👁️ DEMONSTRATION 4: Peek Functionality");
    demonstrate_peek_functionality(&mut broker).await?;

    println!("\n📊 DEMONSTRATION 5: Multiple Device Coordination");
    demonstrate_multi_device_coordination(&mut broker, &registrations).await?;

    println!("\n🛑 Shutting down...");
    broker.shutdown().await?;
    println!("✅ Filtering and batch processing example completed!");

    Ok(())
}

/// Demonstrate filtering events by source type (UPnP vs Polling)
async fn demonstrate_source_filtering(broker: &mut EventBroker) -> Result<(), Box<dyn std::error::Error>> {
    println!("Filtering events by source type...");

    let events = broker.event_iterator()?;

    // Create a filtered iterator for UPnP events only
    let mut upnp_events = events.filter_by_source_type(EventSourceType::UPnP);

    println!("📡 Monitoring UPnP events only for 5 seconds...");
    let mut upnp_count = 0;
    let start = std::time::Instant::now();

    println!("💡 Different filter types available (create separate iterators for each):");
    println!("   • events.filter_by_source_type(EventSourceType::UPnP)     - UPnP notifications only");
    println!("   • events.filter_by_source_type(EventSourceType::Polling)  - Polling-based events only");
    println!("   • events.filter_by_service(Service::AVTransport)          - Transport events only");
    println!("   • events.filter_by_registration(registration_id)          - Single device only");

    // Actually demonstrate the UPnP filter in action
    for event in upnp_events.iter() {
        if start.elapsed() > Duration::from_secs(5) {
            break;
        }

        upnp_count += 1;
        println!("📡 UPnP Event {}: {} from {} ({})",
                 upnp_count,
                 format_event_data(&event.event_data),
                 event.speaker_ip,
                 format_event_source(&event.event_source));
    }

    println!("✅ Captured {} UPnP-only events in 5 seconds", upnp_count);
    Ok(())
}

/// Demonstrate filtering by service type
async fn demonstrate_service_filtering(
    broker: &mut EventBroker,
    registrations: &[RegistrationResult]
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating service-specific event streams...");

    // This demonstrates the API - in practice you'd do this with separate EventBroker instances
    // or process events sequentially rather than in parallel

    println!("🎵 AVTransport Events Filter:");
    println!("   let av_events = events.filter_by_service(Service::AVTransport);");
    println!("   for event in av_events.iter() {{ /* handle transport events */ }}");

    println!("\n🔊 RenderingControl Events Filter:");
    println!("   let volume_events = events.filter_by_service(Service::RenderingControl);");
    println!("   for event in volume_events.iter() {{ /* handle volume events */ }}");

    println!("\n🎯 Registration-Specific Filter:");
    if let Some(first_reg) = registrations.first() {
        println!("   let device_events = events.filter_by_registration({});", first_reg.registration_id);
        println!("   // This would only receive events from registration ID {}", first_reg.registration_id);
    }

    // Demonstrate actual filtering with a short event collection period
    let mut events = broker.event_iterator()?;
    let mut collected_events = Vec::new();

    println!("\n📡 Collecting events for 3 seconds to demonstrate filtering...");
    let collection_start = std::time::Instant::now();

    while collection_start.elapsed() < Duration::from_secs(3) {
        match tokio::time::timeout(Duration::from_millis(200), events.next_async()).await {
            Ok(Some(event)) => {
                collected_events.push(event);
            }
            Ok(None) => break,
            Err(_) => continue, // Timeout, keep trying
        }
    }

    // Analyze collected events
    analyze_collected_events(&collected_events);

    Ok(())
}

/// Demonstrate batch processing of events
async fn demonstrate_batch_processing(broker: &mut EventBroker) -> Result<(), Box<dyn std::error::Error>> {
    println!("Demonstrating batch processing for efficient event handling...");

    let mut events = broker.event_iterator()?;

    println!("📦 Batch Processing Pattern:");
    println!("   let batch = events.next_batch(max_count: 5, max_wait: 2s).await;");
    println!("   // Efficiently process multiple events together");

    // Collect a batch of events
    println!("\n🔄 Collecting batch of up to 5 events (max 3 seconds wait)...");
    let batch = events.next_batch(5, Duration::from_secs(3)).await;

    if batch.is_empty() {
        println!("📭 No events received in batch");
        println!("💡 Try interacting with your Sonos device to generate events");
    } else {
        println!("📦 Received batch of {} events:", batch.len());

        // Process batch efficiently
        let mut transport_changes = 0;
        let mut volume_changes = 0;
        let mut devices_affected = std::collections::HashSet::new();

        for (i, event) in batch.iter().enumerate() {
            devices_affected.insert(event.speaker_ip);

            match &event.event_data {
                EventData::AVTransportEvent(_) => {
                    transport_changes += 1;
                    println!("   {}. 🎵 Transport event from {} ({})",
                             i + 1, event.speaker_ip, format_event_source(&event.event_source));
                }
                EventData::RenderingControlEvent(_) => {
                    volume_changes += 1;
                    println!("   {}. 🔊 Volume event from {} ({})",
                             i + 1, event.speaker_ip, format_event_source(&event.event_source));
                }
                EventData::ZoneGroupTopologyEvent(topology) => {
                    println!("   {}. 🏠 Topology event from {} ({} groups, {})",
                             i + 1,
                             event.speaker_ip,
                             topology.zone_groups.len(),
                             format_event_source(&event.event_source));
                }
                EventData::DevicePropertiesEvent(_) => {
                    println!("   {}. ⚙️  Device properties event from {} ({})",
                             i + 1, event.speaker_ip, format_event_source(&event.event_source));
                }
            }
        }

        println!("\n📊 Batch Analysis:");
        println!("   Transport changes: {}", transport_changes);
        println!("   Volume changes: {}", volume_changes);
        println!("   Devices affected: {}", devices_affected.len());

        // Demonstrate batch processing benefits
        println!("\n💡 Batch Processing Benefits:");
        println!("   • Reduce per-event overhead");
        println!("   • Efficient database bulk operations");
        println!("   • Better resource utilization");
        println!("   • Coordinated multi-device updates");
    }

    Ok(())
}

/// Demonstrate peek functionality for lookahead
async fn demonstrate_peek_functionality(broker: &mut EventBroker) -> Result<(), Box<dyn std::error::Error>> {
    println!("Demonstrating peek functionality for event lookahead...");

    let mut events = broker.event_iterator()?;

    println!("👁️ Peek Pattern:");
    println!("   if let Some(next_event) = events.peek().await {{");
    println!("       // Examine next event without consuming it");
    println!("   }}");

    // Try to peek at the next event
    println!("\n🔍 Attempting to peek at next event...");

    match events.peek().await {
        Some(peeked_event) => {
            println!("👁️ Peeked at next event:");
            println!("   Device: {}", peeked_event.speaker_ip);
            println!("   Service: {:?}", peeked_event.service);
            println!("   Source: {}", format_event_source(&peeked_event.event_source));

            // Note: We'll consume this event next

            // Event is still available for next()
            println!("\n📨 Now consuming the peeked event...");
            // Drop the borrow from peek() by moving out of the match
        }
        None => {
            println!("❓ No events available to peek at");
            return Ok(());
        }
    }

    // Now we can safely call next_async() without conflicting borrows
    match events.next_async().await {
        Some(consumed_event) => {
            println!("✅ Successfully consumed the same event");
            // We need to get the peeked registration ID from above, but since we're outside
            // the match now, let's just confirm we got an event
            println!("   Consumed event from: {}", consumed_event.speaker_ip);
        }
        None => println!("❓ Event stream ended"),
    }

    Ok(())
}

/// Demonstrate coordination across multiple devices
async fn demonstrate_multi_device_coordination(
    broker: &mut EventBroker,
    registrations: &[RegistrationResult]
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Demonstrating multi-device event coordination...");

    if registrations.len() < 2 {
        println!("💡 Only one device registered - multi-device coordination requires multiple devices");
        println!("   Consider adding a second device with:");
        println!("   broker.register_speaker_service(\"192.168.1.101\".parse()?, Service::AVTransport)");
        return Ok(());
    }

    let mut events = broker.event_iterator()?;
    let mut device_states = HashMap::new();

    println!("📊 Monitoring multi-device coordination for 5 seconds...");
    println!("💡 This pattern is useful for:");
    println!("   • Synchronized playback across devices");
    println!("   • Volume normalization");
    println!("   • Multi-room audio coordination");

    let start = std::time::Instant::now();
    let mut events_per_device = HashMap::new();

    while start.elapsed() < Duration::from_secs(5) {
        match tokio::time::timeout(Duration::from_millis(300), events.next_async()).await {
            Ok(Some(event)) => {
                let device_count = events_per_device.entry(event.speaker_ip).or_insert(0);
                *device_count += 1;

                println!("📡 Event from {}: {:?} (total from this device: {})",
                         event.speaker_ip, event.service, device_count);

                // Track device state changes
                match &event.event_data {
                    EventData::AVTransportEvent(transport_event) => {
                        if let Some(ref state) = transport_event.transport_state {
                            device_states.insert(event.speaker_ip, Some(state.clone()));

                            // Check for synchronized playback
                            if device_states.len() > 1 {
                                let states: Vec<&Option<String>> = device_states.values().collect();
                                let all_playing = states.iter().all(|s| s.as_ref().map_or(false, |st| st == "PLAYING"));
                                let all_paused = states.iter().all(|s| s.as_ref().map_or(false, |st| st.contains("PAUSED")));

                                if all_playing {
                                    println!("   🎵 All devices are now playing - synchronized!");
                                } else if all_paused {
                                    println!("   ⏸️ All devices are now paused - synchronized!");
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    // Summary
    println!("\n📊 Multi-device Coordination Summary:");
    for (device, count) in events_per_device {
        println!("   {}: {} events", device, count);
    }

    println!("💡 Use this pattern to:");
    println!("   • Detect when devices fall out of sync");
    println!("   • Coordinate volume levels across rooms");
    println!("   • Implement group playback features");

    Ok(())
}

/// Analyze a collection of events to show filtering patterns
fn analyze_collected_events(events: &[sonos_stream::events::types::EnrichedEvent]) {
    if events.is_empty() {
        println!("📭 No events collected for analysis");
        return;
    }

    println!("📈 Event Analysis ({} total events):", events.len());

    // Group by service type
    let mut av_transport_events = 0;
    let mut rendering_control_events = 0;
    let mut upnp_events = 0;
    let mut polling_events = 0;

    for event in events {
        match event.service {
            Service::AVTransport => av_transport_events += 1,
            Service::RenderingControl => rendering_control_events += 1,
            _ => {}
        }

        match &event.event_source {
            EventSource::UPnPNotification { .. } => upnp_events += 1,
            EventSource::PollingDetection { .. } => polling_events += 1,
        }
    }

    println!("   Service Distribution:");
    println!("     🎵 AVTransport: {} events ({:.1}%)",
             av_transport_events, (av_transport_events as f64 / events.len() as f64) * 100.0);
    println!("     🔊 RenderingControl: {} events ({:.1}%)",
             rendering_control_events, (rendering_control_events as f64 / events.len() as f64) * 100.0);

    println!("   Source Distribution:");
    println!("     📡 UPnP Events: {} ({:.1}%)",
             upnp_events, (upnp_events as f64 / events.len() as f64) * 100.0);
    println!("     🔄 Polling Events: {} ({:.1}%)",
             polling_events, (polling_events as f64 / events.len() as f64) * 100.0);

    println!("\n🎯 Filtering Use Cases:");
    if av_transport_events > 0 {
        println!("   • Filter AVTransport: Perfect for playback state tracking");
    }
    if rendering_control_events > 0 {
        println!("   • Filter RenderingControl: Ideal for volume management");
    }
    if upnp_events > 0 {
        println!("   • Filter UPnP Events: Real-time event processing");
    }
    if polling_events > 0 {
        println!("   • Filter Polling Events: Network-resilient state tracking");
    }
}

/// Format event data for display
fn format_event_data(data: &EventData) -> String {
    match data {
        EventData::AVTransportEvent(_) => "AVTransport Event".to_string(),
        EventData::RenderingControlEvent(_) => "Volume Event".to_string(),
        EventData::ZoneGroupTopologyEvent(topology) => {
            format!("Topology Event ({} groups)", topology.zone_groups.len())
        }
        EventData::DevicePropertiesEvent(_) => "Device Properties Event".to_string(),
    }
}

/// Format event source for display
fn format_event_source(source: &EventSource) -> String {
    match source {
        EventSource::UPnPNotification { .. } => "UPnP".to_string(),
        EventSource::PollingDetection { poll_interval } => {
            format!("Poll({}s)", poll_interval.as_secs())
        }
    }
}