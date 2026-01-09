//! Smart Dashboard - demonstrates the consolidated reactive StateManager
//!
//! This example shows how the consolidated StateManager simplifies Sonos integration:
//! - Single StateManager handles both state and events automatically
//! - No manual event manager setup required
//! - Automatic UPnP subscription management
//! - Property-driven reactive updates
//!
//! Run with: cargo run -p sonos-event-manager --example smart_dashboard

use std::time::Duration;
use sonos_state::{
    model::SpeakerId,
    property::{Volume, Mute, PlaybackState, CurrentTrack, Position},
    StateManager,
};

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("=== Sonos Smart Dashboard (using Consolidated StateManager) ===\n");

    // Step 1: Create reactive state manager (handles events automatically!)
    let manager = StateManager::new().await?;
    println!("✓ Created reactive state manager with integrated event processing");

    // Step 2: Discover and add devices (simplified!)
    println!("Discovering Sonos devices...");
    let devices = tokio::task::spawn_blocking(|| {
        sonos_discovery::get_with_timeout(Duration::from_secs(5))
    })
    .await?;

    if devices.is_empty() {
        println!("No Sonos devices found on the network.");
        return Ok(());
    }

    // Add devices - this automatically sets up event management
    manager.add_devices(devices.clone()).await?;

    println!("Found {} devices:", devices.len());
    for device in &devices {
        println!(
            "  - {} ({}) at {}",
            device.name, device.model_name, device.ip_address
        );
    }
    println!("✓ Added all devices to state manager");

    // Step 3: Create speaker IDs for property watching
    let speaker_ids: Vec<(SpeakerId, String)> = devices
        .iter()
        .map(|d| (SpeakerId::new(&d.id), d.name.clone()))
        .collect();

    if speaker_ids.is_empty() {
        println!("No speakers available for monitoring");
        return Ok(());
    }

    // Step 4: Set up property watching (triggers automatic UPnP subscriptions!)
    println!("\nSetting up property subscriptions...");
    let mut _volume_watchers = Vec::new();
    let mut _mute_watchers = Vec::new();
    let mut _playback_watchers = Vec::new();

    for (speaker_id, name) in &speaker_ids {
        // Watch volume - automatically subscribes to RenderingControl service!
        match manager.watch_property::<Volume>(speaker_id.clone()).await {
            Ok(watcher) => {
                println!("  ✓ Watching volume for {}", name);
                _volume_watchers.push(watcher);
            }
            Err(e) => {
                println!("  ✗ Failed to watch volume for {}: {}", name, e);
            }
        }

        // Watch mute - shares the same RenderingControl subscription!
        match manager.watch_property::<Mute>(speaker_id.clone()).await {
            Ok(watcher) => {
                println!("  ✓ Watching mute for {}", name);
                _mute_watchers.push(watcher);
            }
            Err(e) => {
                println!("  ✗ Failed to watch mute for {}: {}", name, e);
            }
        }

        // Watch playback state - automatically subscribes to AVTransport service!
        match manager.watch_property::<PlaybackState>(speaker_id.clone()).await {
            Ok(watcher) => {
                println!("  ✓ Watching playback state for {}", name);
                _playback_watchers.push(watcher);
            }
            Err(e) => {
                println!("  ✗ Failed to watch playback state for {}: {}", name, e);
            }
        }
    }

    // Step 5: Show subscription stats (debug only in release builds)
    #[cfg(debug_assertions)]
    {
        let stats = manager.subscription_stats().await;
        println!("\nActive UPnP subscriptions:");
        for (key, ref_count) in &stats {
            println!("  - {:?} at {} -> {} watchers", key.service, key.speaker_ip, ref_count);
        }
    }

    println!("\n=== Live Property Dashboard ===");
    println!("Showing current state every 3 seconds (Ctrl+C to quit)...\n");

    // Give initial subscriptions time to receive data
    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut interval = tokio::time::interval(Duration::from_secs(3));
    let mut iteration = 0;

    loop {
        interval.tick().await;
        iteration += 1;

        // Clear screen (simple approach)
        print!("\x1B[2J\x1B[1;1H");

        println!("=== Sonos Smart Dashboard === (Update #{})", iteration);
        println!("Last updated: {}\n", chrono::Utc::now().format("%H:%M:%S"));

        // Display current state for each speaker using consolidated API
        for (speaker_id, name) in &speaker_ids {
            println!("--- {} ---", name);

            // Volume & Mute (automatically managed subscriptions!)
            if let Some(vol) = manager.get_property::<Volume>(speaker_id) {
                let mute_str = manager
                    .get_property::<Mute>(speaker_id)
                    .map(|m| if m.0 { " [MUTED]" } else { "" })
                    .unwrap_or("");
                println!("  Volume: {}%{}", vol.0, mute_str);
            } else {
                println!("  Volume: Not available");
            }

            // Playback State (automatically managed subscriptions!)
            if let Some(state) = manager.get_property::<PlaybackState>(speaker_id) {
                let state_str = match state {
                    PlaybackState::Playing => "Playing",
                    PlaybackState::Paused => "Paused",
                    PlaybackState::Stopped => "Stopped",
                    PlaybackState::Transitioning => "Transitioning",
                };
                println!("  State: {}", state_str);
            } else {
                println!("  State: Not available");
            }

            // Current Track (if available)
            if let Some(track) = manager.get_property::<CurrentTrack>(speaker_id) {
                if let Some(title) = &track.title {
                    println!("  Track: {}", title);
                }
                if let Some(artist) = &track.artist {
                    println!("  Artist: {}", artist);
                }
            }

            // Position (if available)
            if let Some(pos) = manager.get_property::<Position>(speaker_id) {
                if pos.duration_ms > 0 {
                    let progress = pos.progress();
                    let pos_str = format_time(pos.position_ms);
                    let dur_str = format_time(pos.duration_ms);
                    println!("  Position: {} / {} ({:.0}%)", pos_str, dur_str, progress);
                }
            }

            println!();
        }

        // Handle Ctrl+C gracefully
        tokio::select! {
            _ = interval.tick() => continue,
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down gracefully...");
                break;
            }
        }
    }

    println!("\nDemo complete! All subscriptions cleaned up automatically.");
    println!("Key benefits demonstrated:");
    println!("  ✓ Single StateManager handles both state and events");
    println!("  ✓ Properties automatically trigger UPnP subscriptions");
    println!("  ✓ Multiple properties efficiently share subscriptions");
    println!("  ✓ Reference counting ensures optimal resource usage");
    println!("  ✓ Automatic cleanup when watchers are dropped");
    println!("  ✓ Zero manual service management required");

    Ok(())
}

/// Format milliseconds as MM:SS
fn format_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}