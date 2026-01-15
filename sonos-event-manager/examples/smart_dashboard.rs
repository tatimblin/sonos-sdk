//! Smart Dashboard - demonstrates the sync-first reactive StateManager
//!
//! This example shows how the sync-first StateManager simplifies Sonos integration:
//! - Single StateManager handles both state and events automatically
//! - No async/await required - pure synchronous API
//! - Automatic UPnP subscription management
//! - Property-driven reactive updates via iter()
//!
//! Run with: cargo run -p sonos-event-manager --example smart_dashboard

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use sonos_event_manager::SonosEventManager;
use tracing_subscriber;
use sonos_state::{
    model::SpeakerId,
    property::{CurrentTrack, Mute, PlaybackState, Position, Volume},
    StateManager,
};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("sonos_stream=debug".parse().unwrap())
                .add_directive("sonos_event_manager=debug".parse().unwrap())
                .add_directive("sonos_state=debug".parse().unwrap())
        )
        .init();

    println!("=== Sonos Smart Dashboard (Sync-First API) ===\n");

    // Step 1: Create event manager (sync)
    let event_manager = Arc::new(SonosEventManager::new()?);
    println!("Created event manager");

    // Step 2: Create state manager with event manager wired up (sync)
    let manager = StateManager::builder()
        .with_event_manager(Arc::clone(&event_manager))
        .build()?;
    println!("Created state manager with event integration");

    // Step 3: Discover and add devices (sync)
    println!("\nDiscovering Sonos devices...");
    let devices = sonos_discovery::get_with_timeout(Duration::from_secs(5));

    if devices.is_empty() {
        println!("No Sonos devices found on the network.");
        return Ok(());
    }

    // Add devices - this automatically sets up event management
    manager.add_devices(devices.clone())?;

    println!("Found {} devices:", devices.len());
    for device in &devices {
        println!(
            "  - {} ({}) at {}",
            device.name, device.model_name, device.ip_address
        );
    }
    println!("Added all devices to state manager");

    // Step 4: Create speaker IDs for property watching
    let speaker_ids: Vec<(SpeakerId, String)> = devices
        .iter()
        .map(|d| (SpeakerId::new(&d.id), d.name.clone()))
        .collect();

    if speaker_ids.is_empty() {
        println!("No speakers available for monitoring");
        return Ok(());
    }

    // Step 5: Set up property watching (triggers automatic UPnP subscriptions!)
    println!("\nSetting up property subscriptions...");

    for (speaker_id, name) in &speaker_ids {
        // Watch volume - automatically subscribes to RenderingControl service!
        if let Err(e) = manager.watch_property_with_subscription::<Volume>(speaker_id) {
            println!("  Failed to watch volume for {}: {}", name, e);
        } else {
            println!("  Watching volume for {}", name);
        }

        // Watch mute - shares the same RenderingControl subscription!
        if let Err(e) = manager.watch_property_with_subscription::<Mute>(speaker_id) {
            println!("  Failed to watch mute for {}: {}", name, e);
        } else {
            println!("  Watching mute for {}", name);
        }

        // Watch playback state - automatically subscribes to AVTransport service!
        if let Err(e) = manager.watch_property_with_subscription::<PlaybackState>(speaker_id) {
            println!("  Failed to watch playback state for {}: {}", name, e);
        } else {
            println!("  Watching playback state for {}", name);
        }
    }

    println!("\n=== Live Property Dashboard ===");
    println!("Waiting for events (Ctrl+C to quit)...\n");

    // Give initial subscriptions time to receive data
    thread::sleep(Duration::from_secs(2));

    // Set up Ctrl+C handler
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    })?;

    // Display initial state
    display_dashboard(&manager, &speaker_ids);

    // Process events via blocking iterator with timeout
    let iter = manager.iter();
    while running.load(std::sync::atomic::Ordering::SeqCst) {
        // Try to receive an event with timeout
        if let Some(event) = iter.recv_timeout(Duration::from_secs(1)) {
            println!(
                "\n[Event] {} changed for {}",
                event.property_key,
                event.speaker_id.as_str()
            );

            // Refresh dashboard on change
            display_dashboard(&manager, &speaker_ids);
        }
    }

    println!("\nShutting down gracefully...");
    println!("\nDemo complete! All subscriptions cleaned up automatically.");
    println!("Key benefits demonstrated:");
    println!("  - Single StateManager handles both state and events");
    println!("  - Pure synchronous API - no async/await required");
    println!("  - Properties automatically trigger UPnP subscriptions");
    println!("  - Multiple properties efficiently share subscriptions");
    println!("  - Reference counting ensures optimal resource usage");
    println!("  - Automatic cleanup when manager is dropped");

    Ok(())
}

/// Display dashboard showing current state of all speakers
fn display_dashboard(manager: &StateManager, speaker_ids: &[(SpeakerId, String)]) {
    println!("\n--- Dashboard Update ---");
    println!("Time: {}", chrono::Utc::now().format("%H:%M:%S"));

    for (speaker_id, name) in speaker_ids {
        println!("\n{}", name);
        println!("{}", "=".repeat(name.len()));

        // Volume & Mute
        if let Some(vol) = manager.get_property::<Volume>(speaker_id) {
            let mute_str = manager
                .get_property::<Mute>(speaker_id)
                .map(|m| if m.0 { " [MUTED]" } else { "" })
                .unwrap_or("");
            println!("  Volume: {}%{}", vol.0, mute_str);
        } else {
            println!("  Volume: Not available");
        }

        // Playback State
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
    }
}

/// Format milliseconds as MM:SS
fn format_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}
