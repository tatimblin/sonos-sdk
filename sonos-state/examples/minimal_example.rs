//! Minimal example demonstrating sonos-state sync-first API
//!
//! This example shows:
//! 1. Creating a StateManager (sync)
//! 2. Adding devices
//! 3. Getting property values
//! 4. Watching for changes via blocking iteration

use sonos_state::{GroupId, GroupInfo, GroupVolume, StateManager, SpeakerId, Topology, Volume, Property};
use sonos_state::property::SonosProperty;
use sonos_event_manager::SonosEventManager;
use std::sync::Arc;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("1. Initializing StateManager with event manager...");
    let event_manager = Arc::new(SonosEventManager::new()?);
    let manager = StateManager::builder()
        .with_event_manager(Arc::clone(&event_manager))
        .build()?;
    println!("  StateManager initialized");

    println!("2. Discovering devices...");
    let devices = sonos_discovery::get();
    if devices.is_empty() {
        println!("  No Sonos devices found on network");
        return Err("No Sonos devices found".into());
    }
    println!("  Found {} devices", devices.len());

    println!("3. Adding devices to StateManager...");
    manager.add_devices(devices.clone())?;
    println!("  Devices added successfully");

    // Initialize topology so group events can route (speaker → group mapping)
    // In a real app, topology comes from ZoneGroupTopology events.
    // Here we create a simple one-speaker-per-group topology.
    let groups: Vec<GroupInfo> = devices.iter().map(|d| {
        let sid = SpeakerId::new(&d.id);
        GroupInfo::new(GroupId::new(format!("{}:1", d.id)), sid.clone(), vec![sid])
    }).collect();
    let topology = Topology::new(manager.speaker_infos(), groups);
    manager.initialize(topology);
    println!("  Topology initialized");

    let speaker_id = SpeakerId::new(&devices[0].id);
    let speaker_ip: std::net::IpAddr = devices[0].ip_address.parse()?;
    println!("Using speaker: {} ({})", devices[0].name, speaker_ip);

    println!("\n4. Getting current values (from cache)...");
    if let Some(vol) = manager.get_property::<Volume>(&speaker_id) {
        println!("  Current volume: {}%", vol.0);
    } else {
        println!("  No volume data available yet (cache empty)");
    }

    println!("\n5. Setting up property watches + UPnP subscriptions...");

    // Watch speaker volume: register watch + subscribe to RenderingControl
    manager.register_watch(&speaker_id, Volume::KEY);
    event_manager.ensure_service_subscribed(speaker_ip, Volume::SERVICE)?;
    println!("  Watching speaker volume (RenderingControl)");

    // Watch group volume: register watch + subscribe to GroupRenderingControl
    manager.register_watch(&speaker_id, GroupVolume::KEY);
    event_manager.ensure_service_subscribed(speaker_ip, GroupVolume::SERVICE)?;
    println!("  Watching group volume (GroupRenderingControl)");

    println!("  (Change volume within 30s to see events)");

    println!("\n6. Listening for change events...");
    let iter = manager.iter();
    let mut event_count = 0;

    // Listen for events with timeout
    for event in iter.timeout_iter(Duration::from_secs(30)) {
        event_count += 1;
        println!(
            "  [{}] Property '{}' changed on {} (service: {:?})",
            event_count, event.property_key, event.speaker_id, event.service
        );

        // Get the new value based on property key
        match event.property_key {
            "volume" => {
                if let Some(vol) = manager.get_property::<Volume>(&event.speaker_id) {
                    println!("       New speaker volume: {}%", vol.0);
                }
            }
            "group_volume" => {
                // Group volume is stored per-group; look up the group for this speaker
                if let Some(group_info) = manager.get_group_for_speaker(&event.speaker_id) {
                    if let Some(gv) = manager.get_group_property::<GroupVolume>(&group_info.id) {
                        println!("       New group volume: {}%", gv.0);
                    }
                } else {
                    println!("       (no group mapping for this speaker)");
                }
            }
            other => {
                println!("       (unhandled property: {})", other);
            }
        }

        // Stop after 10 events or continue listening
        if event_count >= 10 {
            println!("  Received 10 events, stopping");
            break;
        }
    }

    if event_count > 0 {
        println!("\n  Received {} change event(s)", event_count);
    } else {
        println!("\n  No state changes detected within timeout");
    }

    println!("\nExample completed!");
    Ok(())
}
