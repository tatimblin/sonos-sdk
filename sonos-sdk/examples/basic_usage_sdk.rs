//! Basic usage example of the DOM-like Sonos SDK API
//!
//! This example demonstrates the three core methods on each property:
//! - get() - Get cached value
//! - fetch() - API call + update cache
//! - watch() - Start watching for changes
//!
//! Run with: cargo run -p sonos-sdk --example basic_usage_sdk

use sonos_sdk::{SonosSystem, SdkError};

fn main() -> Result<(), SdkError> {
    println!("Sonos SDK - DOM-like API Example");
    println!("=================================");

    // Create system and discover devices (sync)
    println!("Discovering Sonos devices...");
    let system = SonosSystem::new()?;

    let speaker_names = system.speaker_names();
    if speaker_names.is_empty() {
        println!("No Sonos speakers found on the network");
        println!("Make sure speakers are powered on and connected to the same network");
        return Ok(());
    }

    println!("Found {} speakers: {}", speaker_names.len(), speaker_names.join(", "));

    // Get the first available speaker
    let speaker_name = &speaker_names[0];
    let speaker = system.get_speaker_by_name(speaker_name)
        .ok_or_else(|| SdkError::SpeakerNotFound(speaker_name.clone()))?;

    println!("\nUsing speaker: {} ({})", speaker.name, speaker.ip);
    println!("Speaker ID: {}", speaker.id);

    // Demonstrate the three property access methods
    println!("\nVolume Property Methods:");
    println!("========================");

    // Method 1: get() - Get cached value (fast)
    println!("get() - Cached value:");
    match speaker.volume.get() {
        Some(vol) => println!("   Current volume: {}%", vol.0),
        None => println!("   No cached volume available"),
    }

    // Method 2: fetch() - Fresh API call + update cache (sync)
    println!("fetch() - Fresh from device:");
    match speaker.volume.fetch() {
        Ok(vol) => println!("   Fresh volume: {}%", vol.0),
        Err(e) => println!("   Error fetching volume: {}", e),
    }

    // Method 3: watch() - Register for change notifications (sync)
    println!("watch() - Register for changes:");
    match speaker.volume.watch() {
        Ok(status) => {
            println!("   Started watching volume changes (mode: {})", status.mode);
            if let Some(vol) = status.current {
                println!("   Current volume: {}%", vol.0);
            }
            println!("   (Changes will appear in system.iter())");
        },
        Err(e) => println!("   Error starting volume watcher: {}", e),
    }

    // Demonstrate playback state access
    println!("\nPlayback State Property:");
    println!("========================");

    println!("get() - Cached playback state:");
    match speaker.playback_state.get() {
        Some(state) => println!("   Current state: {:?}", state),
        None => println!("   No cached playback state available"),
    }

    println!("fetch() - Fresh playback state:");
    match speaker.playback_state.fetch() {
        Ok(state) => println!("   Fresh state: {:?}", state),
        Err(e) => println!("   Error fetching playback state: {}", e),
    }

    println!("\nDOM-like API demonstration complete!");
    println!("The API works exactly as designed:");
    println!("  system.get_speaker_by_name(\"Speaker\").volume.get()");
    println!("  system.get_speaker_by_name(\"Speaker\").volume.fetch()");
    println!("  system.get_speaker_by_name(\"Speaker\").volume.watch()");

    Ok(())
}