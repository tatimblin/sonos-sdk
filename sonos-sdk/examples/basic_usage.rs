//! Basic usage example of the DOM-like Sonos SDK API
//!
//! This example demonstrates the three core methods on each property:
//! - get() - Get cached value
//! - fetch() - API call + update cache
//! - watch() - UPnP event streaming
//!
//! Run with: cargo run -p sonos-sdk --example basic_usage

use sonos_sdk::{SonosSystem, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    println!("🎵 Sonos SDK - DOM-like API Example");
    println!("===================================");

    // Create system and discover devices
    println!("🔍 Discovering Sonos devices...");
    let system = SonosSystem::new().await?;

    let speaker_names = system.speaker_names().await;
    if speaker_names.is_empty() {
        println!("❌ No Sonos speakers found on the network");
        println!("   Make sure speakers are powered on and connected to the same network");
        return Ok(());
    }

    println!("✅ Found {} speakers: {}", speaker_names.len(), speaker_names.join(", "));

    // Get the first available speaker
    let speaker_name = &speaker_names[0];
    let speaker = system.get_speaker_by_name(speaker_name).await
        .ok_or_else(|| SdkError::SpeakerNotFound(speaker_name.clone()))?;

    println!("\n🔊 Using speaker: {} ({})", speaker.name, speaker.ip);
    println!("📊 Speaker ID: {}", speaker.id);

    // Demonstrate the three property access methods
    println!("\n🎛️  Volume Property Methods:");
    println!("===========================");

    // Method 1: get() - Get cached value (fast)
    println!("📋 get() - Cached value:");
    match speaker.volume.get() {
        Some(vol) => println!("   Current volume: {}%", vol.0),
        None => println!("   No cached volume available"),
    }

    // Method 2: fetch() - Fresh API call + update cache
    println!("🌐 fetch() - Fresh from device:");
    match speaker.volume.fetch().await {
        Ok(vol) => println!("   Fresh volume: {}%", vol.0),
        Err(e) => println!("   Error fetching volume: {}", e),
    }

    // Method 3: watch() - Reactive UPnP event streaming
    println!("👁️  watch() - Reactive streaming:");
    match speaker.volume.watch().await {
        Ok(watcher) => {
            println!("   Started watching volume changes");
            if let Some(vol) = watcher.current() {
                println!("   Current streamed volume: {}%", vol.0);
            }

            // Note: In a real app, you'd use watcher.changed().await to react to changes
            println!("   (PropertyWatcher is ready for reactive updates)");
        },
        Err(e) => println!("   Error starting volume watcher: {}", e),
    }

    // Demonstrate playback state access
    println!("\n🎵 Playback State Property:");
    println!("============================");

    println!("📋 get() - Cached playback state:");
    match speaker.playback_state.get() {
        Some(state) => println!("   Current state: {:?}", state),
        None => println!("   No cached playback state available"),
    }

    println!("🌐 fetch() - Fresh playback state:");
    match speaker.playback_state.fetch().await {
        Ok(state) => println!("   Fresh state: {:?}", state),
        Err(e) => println!("   Error fetching playback state: {}", e),
    }

    println!("\n✨ DOM-like API demonstration complete!");
    println!("   The API works exactly as designed:");
    println!("   system.get_speaker_by_name(\"Speaker\").volume.get()");
    println!("   system.get_speaker_by_name(\"Speaker\").volume.fetch().await");
    println!("   system.get_speaker_by_name(\"Speaker\").volume.watch().await");

    Ok(())
}