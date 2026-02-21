//! Basic usage example demonstrating optimal state management pattern
//!
//! This example shows the recommended approach for maintaining local state from Sonos events:
//! 1. Initialize local state through direct queries (not from events)
//! 2. Process change events to maintain local state using sync iterator (best practice)
//! 3. Handle events from both UPnP notifications and polling transparently
//! 4. Get clear feedback about firewall detection and polling reasons

use sonos_stream::{
    BrokerConfig, EventBroker, EventData, PollingReason,
};
use sonos_api::{SonosClient, Service, OperationBuilder};
use sonos_api::services::av_transport::{GetTransportInfoOperation, GetTransportInfoOperationRequest};
use sonos_api::services::rendering_control::{GetVolumeOperation, GetVolumeOperationRequest};
use sonos_discovery;
use std::net::IpAddr;

/// Local transport state maintained by the consumer
#[derive(Debug, Clone)]
struct LocalTransportState {
    transport_state: String,
    current_track_uri: String,
    track_duration: String,
    rel_time: String,
    track_metadata: String,
}

/// Local rendering control state maintained by the consumer
#[derive(Debug, Clone)]
struct LocalVolumeState {
    volume: u16,
    mute: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎵 Sonos Stream - Basic State Management Example");
    println!("=================================================");

    // Create broker with default configuration (includes proactive firewall detection)
    let mut broker = EventBroker::new(BrokerConfig::default()).await?;
    let client = SonosClient::new();

    // Discover Sonos devices on the network
    println!("\n🔍 Discovering Sonos devices on the network...");
    let devices = tokio::task::spawn_blocking(|| {
        sonos_discovery::get_with_timeout(std::time::Duration::from_secs(5))
    }).await?;

    if devices.is_empty() {
        println!("❌ No Sonos devices found on the network!");
        println!("   Make sure your Sonos speakers are powered on and connected to the same network.");
        return Ok(());
    }

    println!("✅ Found {} Sonos device(s):", devices.len());
    for (i, device) in devices.iter().enumerate() {
        println!("   {}. {} ({}) - {} at {}:{}",
                 i + 1,
                 device.name,
                 device.room_name,
                 device.model_name,
                 device.ip_address,
                 device.port);
    }

    // Use a device that supports subscriptions - try to find the Sonos Playbar or Amp
    let selected_device = devices
        .iter()
        .find(|d| d.model_name.contains("Playbar") || d.model_name.contains("Amp"))
        .unwrap_or(&devices[0]);
    let device_ip: IpAddr = selected_device.ip_address.parse()?;

    println!("\n🎯 Using device: {} ({}) at {}",
             selected_device.name,
             selected_device.room_name,
             device_ip);

    println!("\n📋 Registering Sonos services...");

    // Register services with enhanced firewall detection feedback
    let transport_reg = broker.register_speaker_service(device_ip, Service::AVTransport).await?;
    let volume_reg = broker.register_speaker_service(device_ip, Service::RenderingControl).await?;
    let group_mgmt_reg = broker.register_speaker_service(device_ip, Service::GroupManagement).await?;
    let group_rc_reg = broker.register_speaker_service(device_ip, Service::GroupRenderingControl).await?;

    // Provide user feedback based on firewall detection results
    println!("\n🔍 Registration Results:");
    print_registration_feedback(&transport_reg.firewall_status, transport_reg.polling_reason.as_ref(), "AVTransport");
    print_registration_feedback(&volume_reg.firewall_status, volume_reg.polling_reason.as_ref(), "RenderingControl");
    print_registration_feedback(&group_mgmt_reg.firewall_status, group_mgmt_reg.polling_reason.as_ref(), "GroupManagement");
    print_registration_feedback(&group_rc_reg.firewall_status, group_rc_reg.polling_reason.as_ref(), "GroupRenderingControl");

    println!("\n📊 STEP 1: Initialize local state through direct queries");
    println!("(This is how consumers should handle initial state population)");

    // Initialize local state through direct device queries - NOT from events
    let mut local_transport_state = query_initial_transport_state(&client, &device_ip).await?;
    let mut local_volume_state = query_initial_volume_state(&client, &device_ip).await?;

    println!("✅ Initial state loaded:");
    println!("  Transport: {} | Track: {} | Position: {}",
             local_transport_state.transport_state,
             extract_track_title(&local_transport_state.track_metadata),
             local_transport_state.rel_time);
    println!("  Volume: {} | Muted: {}", local_volume_state.volume, local_volume_state.mute);

    println!("\n🔄 STEP 2: Process change events to maintain local state");
    println!("(Using async iterator for real-time event processing)");
    println!("Waiting for events... (try changing volume or playing/pausing on your Sonos device)\n");

    // Get event iterator and use async pattern for real-time event processing
    let mut events = broker.event_iterator()?;
    let mut event_count = 0;

    // ASYNC ITERATOR PATTERN - Real-time event processing within async context
    while let Some(event) = events.next_async().await {
        event_count += 1;

        println!("📨 Event #{} received from {} ({})",
                 event_count,
                 event.speaker_ip,
                 format_event_source(&event.event_source));

        match event.event_data {
            // Complete event data - all services now provide full state
            EventData::AVTransportEvent(transport_event) => {
                println!("🎵 Transport event received:");
                if let Some(ref state) = transport_event.transport_state {
                    println!("   → Transport state: {}", state);
                    local_transport_state.transport_state = state.clone();
                }
                if let Some(ref uri) = transport_event.current_track_uri {
                    println!("   → Track URI: {}", uri);
                    local_transport_state.current_track_uri = uri.clone();
                }
                if let Some(ref position) = transport_event.rel_time {
                    println!("   → Position: {}", position);
                    local_transport_state.rel_time = position.clone();
                }
                if let Some(ref metadata) = transport_event.track_metadata {
                    local_transport_state.track_metadata = metadata.clone();
                }
                if let Some(ref duration) = transport_event.track_duration {
                    local_transport_state.track_duration = duration.clone();
                }

                println!("   → Updated state: {} | Position: {}",
                         local_transport_state.transport_state,
                         local_transport_state.rel_time);
            }

            EventData::RenderingControlEvent(volume_event) => {
                println!("🔊 Volume event received:");
                if let Some(ref volume) = volume_event.master_volume {
                    if let Ok(vol_num) = volume.parse::<u16>() {
                        println!("   → Volume level: {}", vol_num);
                        local_volume_state.volume = vol_num;
                    }
                }
                if let Some(ref mute) = volume_event.master_mute {
                    let mute_bool = mute == "1" || mute.to_lowercase() == "true";
                    println!("   → Mute state: {}", mute_bool);
                    local_volume_state.mute = mute_bool;
                }

                println!("   → Updated state: Volume {} | Muted: {}",
                         local_volume_state.volume,
                         local_volume_state.mute);
            }

            // ZoneGroupTopology events - complete speaker topology information
            EventData::ZoneGroupTopologyEvent(topology) => {
                println!("🏠 Speaker topology event received:");
                println!("   → {} zone group(s) found", topology.zone_groups.len());

                for (i, group) in topology.zone_groups.iter().enumerate() {
                    println!("   → Group {}: Coordinator {} with {} member(s)",
                             i + 1,
                             group.coordinator,
                             group.members.len());

                    for member in &group.members {
                        let wireless_status = if member.network_info.wifi_enabled == "1" {
                            format!("WiFi ({}MHz)", member.network_info.channel_freq)
                        } else {
                            "Ethernet".to_string()
                        };

                        println!("     • {} ({}) - {} - {}",
                                 member.zone_name,
                                 member.uuid,
                                 member.software_version,
                                 wireless_status);

                        if !member.satellites.is_empty() {
                            println!("       └─ {} satellite speaker(s)", member.satellites.len());
                        }
                    }
                }
            }

            // Device properties events
            EventData::DevicePropertiesEvent(device_event) => {
                println!("⚙️  Device properties event received:");
                if let Some(ref zone_name) = device_event.zone_name {
                    println!("   → Zone name: {}", zone_name);
                }
                if let Some(ref model) = device_event.model_name {
                    println!("   → Model: {}", model);
                }
                if let Some(ref version) = device_event.software_version {
                    println!("   → Software version: {}", version);
                }
            }

            // GroupManagement events
            EventData::GroupManagementEvent(gm_event) => {
                println!("🔗 Group management event received:");
                if let Some(is_local) = gm_event.group_coordinator_is_local {
                    println!("   → Coordinator is local: {}", is_local);
                }
                if let Some(ref group_uuid) = gm_event.local_group_uuid {
                    println!("   → Local group UUID: {}", group_uuid);
                }
                if let Some(reset_vol) = gm_event.reset_volume_after {
                    println!("   → Reset volume after ungroup: {}", reset_vol);
                }
            }

            // GroupRenderingControl events
            EventData::GroupRenderingControlEvent(grc_event) => {
                println!("🔊 Group rendering control event received:");
                if let Some(volume) = grc_event.group_volume {
                    println!("   → Group volume: {}", volume);
                }
                if let Some(mute) = grc_event.group_mute {
                    println!("   → Group mute: {}", mute);
                }
                if let Some(changeable) = grc_event.group_volume_changeable {
                    println!("   → Group volume changeable: {}", changeable);
                }
            }
        }

        // Show current combined state
        println!("📊 Current State Summary:");
        println!("   Transport: {} | Track: {} | Position: {}",
                 local_transport_state.transport_state,
                 extract_track_title(&local_transport_state.track_metadata),
                 local_transport_state.rel_time);
        println!("   Volume: {} | Muted: {}", local_volume_state.volume, local_volume_state.mute);
        println!();

        // Stop after 10 events for demonstration purposes
        if event_count >= 10 {
            println!("📋 Processed {} events, stopping demonstration", event_count);
            break;
        }
    }

    println!("\n🛑 Shutting down EventBroker...");
    broker.shutdown().await?;
    println!("✅ Example completed successfully!");

    Ok(())
}

/// Query initial transport state directly from the device
/// This demonstrates how consumers should handle initial state population
async fn query_initial_transport_state(
    client: &SonosClient,
    device_ip: &IpAddr
) -> Result<LocalTransportState, Box<dyn std::error::Error>> {
    // Get transport info using proper operation API
    let request = GetTransportInfoOperationRequest { instance_id: 0 };
    let operation = OperationBuilder::<GetTransportInfoOperation>::new(request).build()?;
    let transport_info = client.execute_enhanced(&device_ip.to_string(), operation)?;

    Ok(LocalTransportState {
        transport_state: transport_info.current_transport_state,
        current_track_uri: "N/A".to_string(), // Position info not available in current API
        track_duration: "N/A".to_string(),
        rel_time: "N/A".to_string(),
        track_metadata: "N/A".to_string(),
    })
}

/// Query initial volume state directly from the device
async fn query_initial_volume_state(
    client: &SonosClient,
    device_ip: &IpAddr
) -> Result<LocalVolumeState, Box<dyn std::error::Error>> {
    // Get volume using proper operation API
    let request = GetVolumeOperationRequest {
        instance_id: 0,
        channel: "Master".to_string(),
    };
    let operation = OperationBuilder::<GetVolumeOperation>::new(request).build()?;
    let volume_response = client.execute_enhanced(&device_ip.to_string(), operation)?;

    Ok(LocalVolumeState {
        volume: volume_response.current_volume as u16,
        mute: false, // Mute status not available in current API
    })
}


/// Print user-friendly registration feedback based on firewall status
fn print_registration_feedback(
    firewall_status: &callback_server::firewall_detection::FirewallStatus,
    polling_reason: Option<&PollingReason>,
    service_name: &str
) {
    use callback_server::firewall_detection::FirewallStatus;

    match firewall_status {
        FirewallStatus::Accessible => {
            if let Some(reason) = polling_reason {
                match reason {
                    PollingReason::EventTimeout => {
                        println!("  {} 📡→🔄 UPnP events timed out - switched to polling", service_name);
                    }
                    PollingReason::SubscriptionFailed => {
                        println!("  {} ❌→🔄 UPnP subscription failed - using polling", service_name);
                    }
                    _ => {
                        println!("  {} 🔄 Using polling mode: {:?}", service_name, reason);
                    }
                }
            } else {
                println!("  {} 📡 UPnP events active - real-time updates enabled", service_name);
            }
        }
        FirewallStatus::Blocked => {
            println!("  {} 🔥 Firewall detected - using polling for immediate updates", service_name);
        }
        FirewallStatus::Unknown => {
            if polling_reason.is_some() {
                println!("  {} ❓ Firewall status unknown - using polling as fallback", service_name);
            } else {
                println!("  {} ❓ Firewall status unknown - monitoring events closely", service_name);
            }
        }
        FirewallStatus::Error => {
            println!("  {} ⚠️  Firewall detection error - using polling as safe fallback", service_name);
        }
    }
}

/// Format event source for display
fn format_event_source(source: &sonos_stream::events::types::EventSource) -> String {
    use sonos_stream::events::types::EventSource;

    match source {
        EventSource::UPnPNotification { .. } => "UPnP Event".to_string(),
        EventSource::PollingDetection { poll_interval } => {
            format!("Polling ({}s interval)", poll_interval.as_secs())
        }
    }
}


/// Extract track title from metadata (simplified)
fn extract_track_title(metadata: &str) -> String {
    if metadata.is_empty() || metadata == "NOT_IMPLEMENTED" {
        return "No Track".to_string();
    }

    // This is a simplified extraction - in practice you'd parse the DIDL-Lite XML
    if metadata.contains("<dc:title>") {
        if let Some(start) = metadata.find("<dc:title>") {
            let start = start + "<dc:title>".len();
            if let Some(end) = metadata[start..].find("</dc:title>") {
                return metadata[start..start+end].to_string();
            }
        }
    }

    "Unknown Track".to_string()
}