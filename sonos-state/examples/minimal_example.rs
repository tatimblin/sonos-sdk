//! Minimal example demonstrating sonos-state sync-first API
//!
//! This example shows:
//! 1. Creating a StateManager (sync)
//! 2. Adding devices
//! 3. Getting property values
//! 4. Watching for changes via blocking iteration

use sonos_state::{StateManager, SpeakerId, Volume, Property};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("1. Initializing StateManager...");
    let manager = StateManager::new()?;
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

    let speaker_id = SpeakerId::new(&devices[0].id);
    println!("Using speaker: {}", devices[0].name);

    println!("\n4. Getting current volume (from cache)...");
    if let Some(vol) = manager.get_property::<Volume>(&speaker_id) {
        println!("  Current volume: {}%", vol.0);
    } else {
        println!("  No volume data available yet (cache empty)");
    }

    println!("\n5. Setting up property watch...");
    manager.register_watch(&speaker_id, Volume::KEY);
    println!("  Watching for volume changes");
    println!("  (Change volume within 10s to see events)");

    println!("\n6. Listening for change events...");
    let iter = manager.iter();
    let mut event_count = 0;

    // Listen for events with timeout
    for event in iter.timeout_iter(Duration::from_secs(10)) {
        event_count += 1;
        println!(
            "  [{}] Property '{}' changed on {}",
            event_count, event.property_key, event.speaker_id
        );

        // Get the new value
        if let Some(vol) = manager.get_property::<Volume>(&event.speaker_id) {
            println!("       New volume: {}%", vol.0);
        }

        // Stop after 5 events or continue listening
        if event_count >= 5 {
            println!("  Received 5 events, stopping");
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
