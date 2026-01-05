//! Firewall detection and polling fallback example
//!
//! This example demonstrates how the EventBroker handles different firewall scenarios
//! and provides transparent switching between UPnP events and polling based on
//! network conditions.

use sonos_stream::{
    BrokerConfig, EventBroker, EventData, PollingReason,
    events::types::EventSource
};
use sonos_api::Service;
use callback_server::firewall_detection::FirewallStatus;
use std::net::IpAddr;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 Sonos Stream - Firewall Detection & Polling Example");
    println!("======================================================");
    println!();
    println!("This example demonstrates:");
    println!("  🔍 Proactive firewall detection");
    println!("  🔄 Automatic polling fallback");
    println!("  📡 Transparent switching between events and polling");
    println!("  🎯 Clear feedback about why polling is being used");
    println!();

    // Example device IP - replace with your actual Sonos device
    let device_ip: IpAddr = "192.168.1.100".parse()?;

    // Demonstrate different configuration scenarios
    println!("🧪 Testing different firewall detection configurations:");
    println!();

    // Test 1: Default configuration with proactive firewall detection
    println!("📋 TEST 1: Default Configuration (Proactive Detection Enabled)");
    test_firewall_scenario(
        BrokerConfig::default(),
        device_ip,
        "Default - proactive firewall detection enabled"
    ).await?;

    // Test 2: Fast polling configuration for poor network conditions
    println!("\n📋 TEST 2: Fast Polling Configuration");
    test_firewall_scenario(
        BrokerConfig::fast_polling(),
        device_ip,
        "Fast polling - optimized for unstable networks"
    ).await?;

    // Test 3: Configuration without firewall detection (fallback only)
    println!("\n📋 TEST 3: No Firewall Detection (Fallback Only)");
    test_firewall_scenario(
        BrokerConfig::no_firewall_detection(),
        device_ip,
        "No firewall detection - relies on event timeout fallback"
    ).await?;

    println!("\n✅ All firewall handling scenarios tested successfully!");

    Ok(())
}

/// Test a specific firewall detection scenario
async fn test_firewall_scenario(
    config: BrokerConfig,
    device_ip: IpAddr,
    description: &str
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔧 Configuration: {}", description);
    println!("   Firewall Detection: {}", config.enable_proactive_firewall_detection);
    println!("   Event Timeout: {:?}", config.event_timeout);
    println!("   Base Polling Interval: {:?}", config.base_polling_interval);

    // Create broker with the test configuration
    let mut broker = EventBroker::new(config).await?;

    println!("\n📋 Registering services...");

    // Register services and analyze the results
    let transport_reg = broker.register_speaker_service(device_ip, Service::AVTransport).await?;
    let volume_reg = broker.register_speaker_service(device_ip, Service::RenderingControl).await?;

    // Analyze and report on the registration results
    println!("\n🔍 Registration Analysis:");
    analyze_registration_result(&transport_reg.firewall_status, transport_reg.polling_reason.as_ref(), "AVTransport");
    analyze_registration_result(&volume_reg.firewall_status, volume_reg.polling_reason.as_ref(), "RenderingControl");

    // Demonstrate event processing for a short period
    println!("\n🎧 Monitoring events for 10 seconds...");
    monitor_events(&mut broker, Duration::from_secs(10)).await?;

    // Check final statistics
    let stats = broker.stats().await;
    println!("\n📊 Final Statistics:");
    println!("   Firewall Status: {:?}", stats.firewall_status);
    println!("   Background Tasks: {}", stats.background_tasks_count);
    println!("   Registry: {} active registrations", stats.registry_stats.total_registrations);
    println!("   Polling: {} active tasks", stats.polling_stats.total_active_tasks);

    // Shutdown cleanly
    broker.shutdown().await?;
    println!("🛑 Scenario test completed\n");

    Ok(())
}

/// Analyze and explain registration results
fn analyze_registration_result(
    firewall_status: &FirewallStatus,
    polling_reason: Option<&PollingReason>,
    service_name: &str
) {
    println!("  {} Service Analysis:", service_name);

    match firewall_status {
        FirewallStatus::Accessible => {
            println!("    🟢 Firewall Status: Accessible - UPnP events should work");

            if let Some(reason) = polling_reason {
                match reason {
                    PollingReason::FirewallBlocked => {
                        println!("    🔄 Mode: Polling (firewall blocking detected despite accessible status)");
                        println!("    💡 Explanation: Initial detection might have been incorrect, switched to polling");
                    }
                    PollingReason::EventTimeout => {
                        println!("    🔄 Mode: Polling (event timeout fallback)");
                        println!("    💡 Explanation: UPnP events were expected but didn't arrive within timeout");
                    }
                    PollingReason::SubscriptionFailed => {
                        println!("    🔄 Mode: Polling (subscription failed)");
                        println!("    💡 Explanation: UPnP subscription creation failed, using polling as backup");
                    }
                    PollingReason::NetworkIssues => {
                        println!("    🔄 Mode: Polling (network issues detected)");
                        println!("    💡 Explanation: Network connectivity problems detected");
                    }
                }
            } else {
                println!("    📡 Mode: UPnP Events - Real-time event delivery active");
                println!("    💡 Explanation: Firewall allows events, UPnP subscriptions working normally");
            }
        }

        FirewallStatus::Blocked => {
            println!("    🔴 Firewall Status: Blocked - UPnP events cannot reach this device");
            println!("    🔄 Mode: Polling (immediate activation due to proactive detection)");
            println!("    💡 Explanation: Firewall blocks incoming HTTP connections, polling provides updates");
            println!("    ⚡ Benefit: No 30-second wait time - polling started immediately");
        }

        FirewallStatus::Unknown => {
            println!("    🟡 Firewall Status: Unknown - detection was inconclusive");

            if let Some(reason) = polling_reason {
                println!("    🔄 Mode: Polling ({})", format_polling_reason(reason));
                println!("    💡 Explanation: Uncertain network conditions, fell back to polling");
            } else {
                println!("    📡 Mode: UPnP Events (with close monitoring)");
                println!("    💡 Explanation: Attempting events but ready to switch to polling quickly");
            }
        }

        FirewallStatus::Error => {
            println!("    🔴 Firewall Status: Error - detection failed due to errors");
            println!("    🔄 Mode: Polling (automatic fallback)");
            println!("    💡 Explanation: Detection process encountered errors, using polling as safe fallback");
        }
    }
}

/// Monitor events for a specified duration
async fn monitor_events(
    broker: &mut EventBroker,
    duration: Duration
) -> Result<(), Box<dyn std::error::Error>> {
    let mut events = broker.event_iterator()?;
    let start_time = std::time::Instant::now();
    let mut event_count = 0;
    let mut upnp_events = 0;
    let mut polling_events = 0;

    // Monitor events until duration expires
    while start_time.elapsed() < duration {
        // Try to get an event with a short timeout
        match tokio::time::timeout(Duration::from_millis(500), events.next_async()).await {
            Ok(Some(event)) => {
                event_count += 1;

                // Categorize the event source
                match &event.event_source {
                    EventSource::UPnPNotification { .. } => {
                        upnp_events += 1;
                        println!("    📡 UPnP Event #{}: {} {:?}", event_count, event.speaker_ip, event.service);
                    }
                    EventSource::PollingDetection { poll_interval } => {
                        polling_events += 1;
                        println!("    🔄 Polling Event #{}: {} {:?} ({}s interval)",
                                 event_count, event.speaker_ip, event.service, poll_interval.as_secs());
                    }
                }

                // Show event content
                match &event.event_data {
                    EventData::AVTransportEvent(transport_event) => {
                        if transport_event.transport_state.is_some() || transport_event.current_track_uri.is_some() {
                            println!("       🎵 Transport event: state={:?}, track={:?}",
                                     transport_event.transport_state, transport_event.current_track_uri);
                        }
                    }
                    EventData::RenderingControlEvent(volume_event) => {
                        if volume_event.master_volume.is_some() || volume_event.master_mute.is_some() {
                            println!("       🔊 Volume event: level={:?}, mute={:?}",
                                     volume_event.master_volume, volume_event.master_mute);
                        }
                    }
                    EventData::ZoneGroupTopologyEvent(topology) => {
                        println!("       🏠 Topology event: {} groups, {} total speakers",
                                 topology.zone_groups.len(),
                                 topology.zone_groups.iter()
                                     .map(|g| g.members.len() + g.members.iter().map(|m| m.satellites.len()).sum::<usize>())
                                     .sum::<usize>());
                    }
                    EventData::DevicePropertiesEvent(_) => {
                        println!("       ⚙️ Device properties event received");
                    }
                }
            }
            Ok(None) => {
                println!("    📡 Event stream ended");
                break;
            }
            Err(_) => {
                // Timeout - continue monitoring
            }
        }
    }

    // Summary of what happened
    if event_count == 0 {
        println!("    📭 No events received during monitoring period");
        println!("    💡 This could indicate:");
        println!("       • No device activity occurred");
        println!("       • Polling interval longer than monitoring duration");
        println!("       • Device not accessible or configured");
    } else {
        println!("\n    📈 Event Summary:");
        println!("       Total Events: {}", event_count);
        println!("       UPnP Events: {} ({:.1}%)", upnp_events, (upnp_events as f64 / event_count as f64) * 100.0);
        println!("       Polling Events: {} ({:.1}%)", polling_events, (polling_events as f64 / event_count as f64) * 100.0);

        if upnp_events > 0 && polling_events > 0 {
            println!("    🔄 Observed transparent switching between UPnP events and polling!");
        } else if upnp_events > 0 {
            println!("    📡 All events came from UPnP - firewall not blocking");
        } else if polling_events > 0 {
            println!("    🔄 All events came from polling - likely firewall blocking or no UPnP subscription");
        }
    }

    Ok(())
}

/// Format polling reason for display
fn format_polling_reason(reason: &PollingReason) -> String {
    match reason {
        PollingReason::FirewallBlocked => "firewall blocked".to_string(),
        PollingReason::EventTimeout => "event timeout".to_string(),
        PollingReason::SubscriptionFailed => "subscription failed".to_string(),
        PollingReason::NetworkIssues => "network issues".to_string(),
    }
}

