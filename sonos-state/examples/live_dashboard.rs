//! Live Dashboard - displays real-time Sonos state using the consolidated reactive StateManager
//!
//! Run with: cargo run -p sonos-state --example live_dashboard

use std::time::Duration;

use sonos_state::{
    model::SpeakerId,
    property::{CurrentTrack, GroupMembership, Mute, PlaybackState, Position, Volume},
    StateManager,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Sonos Live Dashboard ===\n");

    // Step 1: Discover devices
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

    // Step 2: Create reactive StateManager (automatic event processing)
    let manager = StateManager::new().await?;

    // Step 3: Add devices (automatically sets up subscriptions as needed)
    manager.add_devices(devices.clone()).await?;

    // Build speaker ID map for display
    let speaker_info: Vec<_> = devices
        .iter()
        .map(|device| (SpeakerId::new(&device.id), device.name.clone()))
        .collect();

    // Set up subscriptions for all properties we want to display
    // This triggers automatic UPnP subscriptions
    println!("Setting up property subscriptions...");
    let mut _volume_watchers = Vec::new();
    let mut _mute_watchers = Vec::new();
    let mut _playback_watchers = Vec::new();

    for (speaker_id, _) in &speaker_info {
        // Set up watchers for properties we'll display
        // The watchers will trigger automatic subscriptions
        _volume_watchers.push(manager.watch_property::<Volume>(speaker_id.clone()).await?);
        _mute_watchers.push(manager.watch_property::<Mute>(speaker_id.clone()).await?);
        _playback_watchers.push(manager.watch_property::<PlaybackState>(speaker_id.clone()).await?);

        // Uncomment these if you want to display track info (might take longer to set up)
        // let _track_watcher = manager.watch_property::<CurrentTrack>(speaker_id.clone()).await?;
        // let _position_watcher = manager.watch_property::<Position>(speaker_id.clone()).await?;
        // let _group_watcher = manager.watch_property::<GroupMembership>(speaker_id.clone()).await?;
    }
    println!("✓ Property subscriptions set up");

    // Give subscriptions a moment to receive initial state
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("Starting live dashboard (Ctrl+C to quit)...\n");

    // Step 4: Periodically display current state
    let mut interval = tokio::time::interval(Duration::from_secs(2));

    loop {
        interval.tick().await;

        // Clear screen (simple approach)
        print!("\x1B[2J\x1B[1;1H");

        println!("=== Sonos Live Dashboard ===");
        println!("Last updated: {}\n", chrono::Utc::now().format("%H:%M:%S"));

        // Display state for each speaker
        for (speaker_id, name) in &speaker_info {
            print_speaker_state(&manager, speaker_id, name).await;
        }

        // Handle Ctrl+C gracefully
        tokio::select! {
            _ = interval.tick() => continue,
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
                break;
            }
        }
    }

    Ok(())
}

/// Print current state for a speaker using the new consolidated API
async fn print_speaker_state(manager: &StateManager, speaker_id: &SpeakerId, name: &str) {
    println!("--- {} ---", name);

    // Volume & Mute (these will automatically subscribe to RenderingControl if needed)
    if let Some(vol) = manager.get_property::<Volume>(speaker_id) {
        let mute_str = manager
            .get_property::<Mute>(speaker_id)
            .map(|m| if m.0 { " [MUTED]" } else { "" })
            .unwrap_or("");
        println!("  Volume: {}%{}", vol.0, mute_str);
    }

    // Playback State (automatically subscribes to AVTransport if needed)
    if let Some(state) = manager.get_property::<PlaybackState>(speaker_id) {
        let state_str = match state {
            PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused",
            PlaybackState::Stopped => "Stopped",
            PlaybackState::Transitioning => "Transitioning",
        };
        println!("  State: {}", state_str);
    }

    // Current Track
    if let Some(track) = manager.get_property::<CurrentTrack>(speaker_id) {
        if let Some(title) = &track.title {
            println!("  Track: {}", title);
        }
        if let Some(artist) = &track.artist {
            println!("  Artist: {}", artist);
        }
    }

    // Position
    if let Some(pos) = manager.get_property::<Position>(speaker_id) {
        if pos.duration_ms > 0 {
            let progress = pos.progress();
            let pos_str = format_time(pos.position_ms);
            let dur_str = format_time(pos.duration_ms);
            println!("  Position: {} / {} ({:.0}%)", pos_str, dur_str, progress);
        }
    }

    // Group membership
    if let Some(membership) = manager.get_property::<GroupMembership>(speaker_id) {
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
