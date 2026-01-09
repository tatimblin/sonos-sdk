//! Reactive Dashboard - demonstrates demand-driven property subscriptions
//!
//! This example shows how properties can automatically trigger event subscriptions:
//! - First access to a property automatically subscribes to the required UPnP service
//! - Multiple watchers share the same subscription (reference counting)
//! - Last watcher dropping automatically cleans up the subscription
//! - Zero manual service management required!
//!
//! Run with: cargo run -p sonos-state --example reactive_dashboard

use std::time::Duration;

use sonos_state::{
    StateManager,
    property::{Volume, Mute, PlaybackState},
    model::SpeakerId,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Reactive Property Dashboard ===\n");

    // Step 1: Create reactive state manager (automatically includes EventManager)
    let state_manager = StateManager::new().await?;
    println!("✓ Created reactive state manager");

    // Step 2: Discover and add devices
    println!("Discovering Sonos devices...");
    let devices = tokio::task::spawn_blocking(|| {
        sonos_discovery::get_with_timeout(Duration::from_secs(5))
    })
    .await?;

    if devices.is_empty() {
        println!("No Sonos devices found on the network.");
        return Ok(());
    }

    state_manager.add_devices(devices.clone()).await?;
    println!("✓ Added {} devices", devices.len());

    // Step 3: Create speaker IDs for property watching
    let speaker_ids: Vec<SpeakerId> = devices
        .iter()
        .map(|d| SpeakerId::new(&d.id))
        .collect();

    if speaker_ids.is_empty() {
        println!("No speaker IDs available");
        return Ok(());
    }

    let first_speaker = &speaker_ids[0];
    let first_device = &devices[0];
    println!("Using speaker: {} ({})", first_device.name, first_device.ip_address);

    // Step 4: Demonstrate demand-driven subscriptions
    println!("\n=== Demand-Driven Property Access ===");

    // Before any properties are accessed, no subscriptions exist
    let stats = state_manager.subscription_stats().await;
    println!("Initial subscriptions: {} active", stats.len());

    // Step 5: Watch Volume (this automatically subscribes to RenderingControl!)
    println!("\nWatching Volume for {}...", first_device.name);
    let volume_watcher = state_manager.watch_property::<Volume>(first_speaker.clone()).await?;

    let stats = state_manager.subscription_stats().await;
    println!("After Volume watch - subscriptions: {} active", stats.len());
    for (key, ref_count) in &stats {
        println!("  - {:?} -> {} watchers", key, ref_count);
    }

    // Step 6: Watch Mute (shares the same RenderingControl subscription!)
    println!("\nWatching Mute for {}...", first_device.name);
    let mute_watcher = state_manager.watch_property::<Mute>(first_speaker.clone()).await?;

    let stats = state_manager.subscription_stats().await;
    println!("After Mute watch - subscriptions: {} active", stats.len());
    for (key, ref_count) in &stats {
        println!("  - {:?} -> {} watchers", key, ref_count);
    }

    // Step 7: Watch PlaybackState (this subscribes to AVTransport!)
    println!("\nWatching PlaybackState for {}...", first_device.name);
    let playback_watcher = state_manager.watch_property::<PlaybackState>(first_speaker.clone()).await?;

    let stats = state_manager.subscription_stats().await;
    println!("After PlaybackState watch - subscriptions: {} active", stats.len());
    for (key, ref_count) in &stats {
        println!("  - {:?} -> {} watchers", key, ref_count);
    }

    // Step 8: Show current property values (non-reactive access)
    println!("\n=== Current Property Values ===");
    if let Some(volume) = state_manager.get_property::<Volume>(first_speaker) {
        println!("Current Volume: {}%", volume.0);
    } else {
        println!("Volume: Not available (waiting for first event)");
    }

    if let Some(mute) = state_manager.get_property::<Mute>(first_speaker) {
        println!("Current Mute: {}", mute.0);
    } else {
        println!("Mute: Not available (waiting for first event)");
    }

    // Step 9: Demonstrate automatic cleanup
    println!("\n=== Automatic Subscription Cleanup ===");
    println!("Dropping volume watcher...");
    drop(volume_watcher);

    // Give cleanup a moment
    tokio::time::sleep(Duration::from_millis(100)).await;

    let stats = state_manager.subscription_stats().await;
    println!("After dropping volume watcher - subscriptions: {} active", stats.len());
    for (key, ref_count) in &stats {
        println!("  - {:?} -> {} watchers", key, ref_count);
    }

    println!("\nDropping mute watcher...");
    drop(mute_watcher);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let stats = state_manager.subscription_stats().await;
    println!("After dropping mute watcher - subscriptions: {} active", stats.len());
    for (key, ref_count) in &stats {
        println!("  - {:?} -> {} watchers", key, ref_count);
    }

    // Step 10: Show property info
    println!("\n=== Property Subscription Info ===");
    println!("PlaybackState watcher: speaker={:?}", playback_watcher.speaker_id());

    println!("\nDemo complete! All remaining subscriptions will be cleaned up automatically.");
    println!("Key benefits demonstrated:");
    println!("  ✓ Properties automatically trigger the right UPnP subscriptions");
    println!("  ✓ Multiple properties share subscriptions (RenderingControl for Volume+Mute)");
    println!("  ✓ Reference counting ensures efficient resource usage");
    println!("  ✓ Automatic cleanup when properties are no longer watched");
    println!("  ✓ Zero manual service management required");

    Ok(())
}

