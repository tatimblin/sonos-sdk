//! Live Dashboard - displays real-time Sonos state using the new sonos-state system
//!
//! Run with: cargo run -p sonos-state --example live_dashboard

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use sonos_api::Service;
use sonos_state::{
    decoder::{
        AVTransportData, EventData, RawEvent, RenderingControlData, TopologyData, ZoneGroupData,
        ZoneMemberData,
    },
    model::SpeakerId,
    property::{CurrentTrack, GroupMembership, Mute, PlaybackState, Position, Volume},
    StateManager,
};
use sonos_stream::{BrokerConfig, EventBroker, EventData as StreamEventData, EnrichedEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Sonos Live Dashboard ===\n");

    // Step 1: Discover devices (run in blocking context since discovery uses blocking I/O)
    println!("Discovering Sonos devices...");
    let devices = tokio::task::spawn_blocking(|| {
        sonos_discovery::get_with_timeout(Duration::from_secs(5))
    })
    .await?;

    if devices.is_empty() {
        println!("No Sonos devices found on the network.");
        return Ok(());
    }

    println!("Found {} devices:", devices.len());
    for device in &devices {
        println!(
            "  - {} ({}) at {}",
            device.name, device.model_name, device.ip_address
        );
    }
    println!();

    // Step 2: Create StateManager
    let mut manager = StateManager::new();

    // Step 3: Create EventBroker and register services
    let mut broker = EventBroker::new(BrokerConfig::default()).await?;

    // Register all devices for all relevant services
    for device in &devices {
        let ip: IpAddr = device.ip_address.parse()?;

        // Register for RenderingControl (volume, mute)
        if let Err(e) = broker
            .register_speaker_service(ip, Service::RenderingControl)
            .await
        {
            eprintln!(
                "Warning: Failed to register RenderingControl for {}: {}",
                device.name, e
            );
        }

        // Register for AVTransport (playback state, track info)
        if let Err(e) = broker
            .register_speaker_service(ip, Service::AVTransport)
            .await
        {
            eprintln!(
                "Warning: Failed to register AVTransport for {}: {}",
                device.name, e
            );
        }

        // Register for ZoneGroupTopology (groups)
        if let Err(e) = broker
            .register_speaker_service(ip, Service::ZoneGroupTopology)
            .await
        {
            eprintln!(
                "Warning: Failed to register ZoneGroupTopology for {}: {}",
                device.name, e
            );
        }

        // Add speaker to state manager with basic info
        manager
            .store()
            .add_speaker(sonos_state::model::SpeakerInfo {
                id: SpeakerId::new(&device.id),
                name: device.name.clone(),
                room_name: device.name.clone(),
                ip_address: ip,
                port: 1400,
                model_name: device.model_name.clone(),
                software_version: "unknown".to_string(),
                satellites: vec![],
            });
    }

    // Build IP to speaker name map for display
    let ip_to_name: HashMap<IpAddr, String> = devices
        .iter()
        .filter_map(|d| d.ip_address.parse().ok().map(|ip| (ip, d.name.clone())))
        .collect();

    println!("Starting event stream (Ctrl+C to quit)...\n");

    // Step 4: Process events using async iteration
    let mut events = broker.event_iterator()?;

    while let Some(event) = events.next_async().await {
        // Convert sonos-stream event to sonos-state RawEvent
        if let Some(raw_event) = convert_event(&event) {
            // Process through StateManager
            let update_count = manager.process(raw_event);

            if update_count > 0 {
                // Print current state for this speaker
                let speaker_name = ip_to_name
                    .get(&event.speaker_ip)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");

                print_speaker_state(&manager, &event.speaker_ip, speaker_name);
            }
        }
    }

    Ok(())
}

/// Convert a sonos-stream EnrichedEvent to a sonos-state RawEvent
fn convert_event(event: &EnrichedEvent) -> Option<RawEvent> {
    let data = match &event.event_data {
        StreamEventData::RenderingControlEvent(rc) => {
            let mut data = RenderingControlData::new();

            if let Some(vol) = &rc.master_volume {
                if let Ok(v) = vol.parse::<u8>() {
                    data = data.with_volume(v);
                }
            }
            if let Some(mute) = &rc.master_mute {
                data = data.with_mute(mute == "1" || mute.to_lowercase() == "true");
            }

            EventData::RenderingControl(data)
        }

        StreamEventData::AVTransportEvent(av) => {
            let mut data = AVTransportData::new();

            if let Some(state) = &av.transport_state {
                data = data.with_transport_state(state);
            }

            EventData::AVTransport(data)
        }

        StreamEventData::ZoneGroupTopologyEvent(topo) => {
            let zone_groups = topo
                .zone_groups
                .iter()
                .map(|zg| ZoneGroupData {
                    id: zg.id.clone(),
                    coordinator: zg.coordinator.clone(),
                    members: zg
                        .members
                        .iter()
                        .map(|m| ZoneMemberData {
                            uuid: m.uuid.clone(),
                            location: m.location.clone(),
                            zone_name: m.zone_name.clone(),
                            software_version: m.software_version.clone(),
                            ip_address: parse_ip_from_location(&m.location),
                            satellites: m.satellites.iter().map(|s| s.uuid.clone()).collect(),
                        })
                        .collect(),
                })
                .collect();

            EventData::ZoneGroupTopology(TopologyData {
                zone_groups,
                vanished_devices: topo.vanished_devices.clone(),
            })
        }

        StreamEventData::DevicePropertiesEvent(_) => {
            return None;
        }
    };

    Some(RawEvent::new(event.speaker_ip, event.service, data))
}

/// Parse IP address from a location URL
fn parse_ip_from_location(location: &str) -> Option<IpAddr> {
    let without_scheme = location.strip_prefix("http://")?;
    let host_port = without_scheme.split('/').next()?;
    let host = host_port.split(':').next()?;
    host.parse().ok()
}

/// Print current state for a speaker
fn print_speaker_state(manager: &StateManager, ip: &IpAddr, name: &str) {
    let store = manager.store();

    let Some(speaker_id) = store.speaker_id_for_ip(*ip) else {
        return;
    };

    println!("--- {} ---", name);

    // Volume & Mute
    if let Some(vol) = store.get::<Volume>(&speaker_id) {
        let mute_str = store
            .get::<Mute>(&speaker_id)
            .map(|m| if m.0 { " [MUTED]" } else { "" })
            .unwrap_or("");
        println!("  Volume: {}%{}", vol.0, mute_str);
    }

    // Playback State
    if let Some(state) = store.get::<PlaybackState>(&speaker_id) {
        let state_str = match state {
            PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused",
            PlaybackState::Stopped => "Stopped",
            PlaybackState::Transitioning => "Transitioning",
        };
        println!("  State: {}", state_str);
    }

    // Current Track
    if let Some(track) = store.get::<CurrentTrack>(&speaker_id) {
        if let Some(title) = &track.title {
            println!("  Track: {}", title);
        }
        if let Some(artist) = &track.artist {
            println!("  Artist: {}", artist);
        }
    }

    // Position
    if let Some(pos) = store.get::<Position>(&speaker_id) {
        if pos.duration_ms > 0 {
            let progress = pos.progress();
            let pos_str = format_time(pos.position_ms);
            let dur_str = format_time(pos.duration_ms);
            println!("  Position: {} / {} ({:.0}%)", pos_str, dur_str, progress);
        }
    }

    // Group membership
    if let Some(membership) = store.get::<GroupMembership>(&speaker_id) {
        if membership.is_coordinator {
            println!("  Role: Coordinator");
        } else if membership.group_id.is_some() {
            println!("  Role: Group member");
        }
    }

    println!();
}

/// Format milliseconds as MM:SS
fn format_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}
