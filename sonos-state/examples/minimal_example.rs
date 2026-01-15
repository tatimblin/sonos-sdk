use sonos_state::{StateManager, SpeakerId, Volume};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("1. Initializing StateManager...");
    let manager = StateManager::new().await?;
    println!("✓ StateManager initialized");

    println!("2. Discovering devices...");
    let devices = tokio::task::spawn_blocking(|| sonos_discovery::get()).await?;
    if devices.is_empty() {
        println!("✗ No Sonos devices found on network");
        return Err("No Sonos devices found".into());
    }
    println!("✓ Found {} devices", devices.len());

    println!("3. Adding devices to StateManager...");
    manager.add_devices(devices.clone()).await?;
    println!("✓ Devices added successfully");

    let speaker_id = SpeakerId::new(&devices[0].id);
    println!("Using speaker: {}", devices[0].name);

    println!("\n4. Getting current volume (non-reactive)...");
    if let Some(vol) = manager.get_property::<Volume>(&speaker_id) {
        println!("✓ Current volume: {}%", vol.0);
    } else {
        println!("✗ No volume data available yet");
    }

    println!("\n5. Setting up reactive property watcher...");
    let mut watcher = manager.watch_property::<Volume>(speaker_id.clone()).await?;
    println!("✓ Volume watcher created");
    println!("Watching for volume changes (change volume within 10s)...");

    match timeout(Duration::from_secs(10), watcher.changed()).await {
        Ok(Ok(())) => {
            if let Some(v) = watcher.current() {
                println!("✓ Volume changed: {}%", v.0);
            }
        }
        Ok(Err(e)) => println!("✗ Watcher error: {}", e),
        Err(_) => println!("✗ No volume changes detected within 10s timeout"),
    }

    println!("\n6. Testing change stream (10s window - try changing volume)...");
    let mut changes = manager.changes();
    let mut event_count = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match timeout(remaining, changes.next()).await {
            Ok(Some(event)) => {
                event_count += 1;
                println!("  [{}] {} (rerender: {})",
                    event_count,
                    event.context.description,
                    event.context.requires_rerender);
            }
            Ok(None) => {
                println!("✗ Change stream ended unexpectedly");
                break;
            }
            Err(_) => break, // Timeout reached
        }
    }

    if event_count > 0 {
        println!("✓ Received {} change events", event_count);
    } else {
        println!("✗ No state changes detected");
    }

    println!("\n✓ Example completed successfully!");
    Ok(())
}
