//! Global Change Iterator Example
//!
//! Demonstrates how to use the global change iterator for detecting when
//! applications should rerender. Shows both async and blocking patterns,
//! filtering capabilities, and rerender decision making.
//!
//! Run with: `cargo run -p sonos-state --example change_iterator_example`

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sonos_api::Service;
use sonos_discovery;
use sonos_state::{
    ChangeEvent, ChangeFilter, ChangeType, RerenderScope, SpeakerId, StateManager, Volume,
};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎵 Sonos Global Change Iterator Example");
    println!("=====================================\n");

    // Create state manager
    let state_manager = Arc::new(StateManager::new().await?);
    println!("✅ Created StateManager with event processing");

    // Discover devices
    println!("🔍 Discovering Sonos devices...");
    let devices = sonos_discovery::get();

    if devices.is_empty() {
        println!("❌ No Sonos devices found. Make sure devices are on the same network.");
        println!("💡 For testing, you can simulate changes using the update_property method");

        // Demo with simulated changes
        demo_simulated_changes(state_manager).await?;
        return Ok(());
    }

    println!("📱 Found {} device(s):", devices.len());
    for device in &devices {
        println!("  - {} at {}", device.name, device.ip_address);
    }

    // Add devices to state manager
    state_manager.add_devices(devices.clone()).await?;
    println!("✅ Added devices to state manager\n");

    // Convert to SpeakerIds
    let speaker_ids: Vec<SpeakerId> = devices
        .iter()
        .map(|d| SpeakerId::new(&d.id))
        .collect();

    // Demo 1: Basic async change stream
    println!("🚀 Demo 1: Basic Async Change Stream");
    demo_basic_async_stream(&state_manager, &speaker_ids[0]).await?;

    // Demo 2: Filtered change stream
    println!("\n🚀 Demo 2: Filtered Change Stream");
    demo_filtered_stream(&state_manager, &speaker_ids[0]).await?;

    // Demo 3: Rerender-only changes
    println!("\n🚀 Demo 3: Rerender-Only Changes");
    demo_rerender_only(&state_manager, &speaker_ids[0]).await?;

    // Demo 4: Blocking iterator (sync context)
    println!("\n🚀 Demo 4: Blocking Iterator");
    demo_blocking_iterator(&state_manager, &speaker_ids[0])?;

    println!("\n✨ All demos completed!");

    Ok(())
}

/// Demo basic async change stream functionality
async fn demo_basic_async_stream(
    state_manager: &StateManager,
    speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Creating global change stream...");
    let mut changes = state_manager.changes();

    // Start monitoring changes in background
    let state_manager_clone = Arc::new(state_manager.clone());
    let speaker_id_clone = speaker_id.clone();

    tokio::spawn(async move {
        sleep(Duration::from_millis(500)).await;

        // Simulate some property changes
        println!("  🔄 Simulating volume change to 75%");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(75));

        sleep(Duration::from_millis(200)).await;

        println!("  🔄 Simulating volume change to 50%");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(50));

        sleep(Duration::from_millis(200)).await;

        println!("  🔄 Simulating volume change to 25%");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(25));
    });

    // Monitor changes with timeout
    let mut change_count = 0;
    let max_changes = 3;

    println!("  📡 Listening for changes (timeout: 2s)...");

    while change_count < max_changes {
        tokio::select! {
            Some(change) = changes.next() => {
                change_count += 1;
                print_change_event(&change, change_count);

                // Demo: How an application would handle rerender decisions
                match change.context.rerender_scope {
                    RerenderScope::Device(ref speaker_id) => {
                        println!("    💡 Application should: Update device UI for {}", speaker_id.0);
                    }
                    RerenderScope::Full => {
                        println!("    💡 Application should: Full UI refresh");
                    }
                    RerenderScope::Group(ref group_id) => {
                        println!("    💡 Application should: Update group UI for {}", group_id.0);
                    }
                    RerenderScope::System => {
                        println!("    💡 Application should: Update system status");
                    }
                }
            }
            _ = sleep(Duration::from_secs(2)) => {
                println!("  ⏱️ Timeout reached");
                break;
            }
        }
    }

    println!("  ✅ Received {} change(s)", change_count);
    Ok(())
}

/// Demo filtered change stream
async fn demo_filtered_stream(
    state_manager: &StateManager,
    speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Creating filtered change stream (RenderingControl service only)...");

    // Filter for only RenderingControl service changes
    let filter = ChangeFilter {
        services: Some([Service::RenderingControl].into_iter().collect()),
        ..Default::default()
    };
    let mut changes = state_manager.changes_filtered(filter);

    // Simulate changes to different services
    let state_manager_clone = Arc::new(state_manager.clone());
    let speaker_id_clone = speaker_id.clone();

    tokio::spawn(async move {
        sleep(Duration::from_millis(300)).await;

        // This should appear (RenderingControl)
        println!("  🔄 Simulating volume change (RenderingControl service)");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(80));

        // Note: We can't easily simulate AVTransport changes here without more complex setup
        // In a real scenario, these would come from actual device events

        sleep(Duration::from_millis(300)).await;

        println!("  🔄 Simulating another volume change (should be filtered)");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(60));
    });

    println!("  📡 Listening for filtered changes (timeout: 1s)...");

    let mut change_count = 0;
    while change_count < 2 {
        tokio::select! {
            Some(change) = changes.next() => {
                change_count += 1;
                print_change_event(&change, change_count);
                println!("    ✅ Change passed filter (RenderingControl service)");
            }
            _ = sleep(Duration::from_secs(1)) => {
                println!("  ⏱️ Filter timeout");
                break;
            }
        }
    }

    println!("  ✅ Filter demo completed");
    Ok(())
}

/// Demo rerender-only filtering
async fn demo_rerender_only(
    state_manager: &StateManager,
    speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Creating rerender-only change stream...");

    let filter = ChangeFilter::rerender_only();
    let mut changes = state_manager.changes_filtered(filter);

    // Simulate both render and non-render changes
    let state_manager_clone = Arc::new(state_manager.clone());
    let speaker_id_clone = speaker_id.clone();

    tokio::spawn(async move {
        sleep(Duration::from_millis(300)).await;

        // Volume changes typically require rerender
        println!("  🔄 Simulating volume change (requires rerender)");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(90));

        // Note: Position updates are marked as not requiring immediate rerender
        // in the real system based on property type
    });

    println!("  📡 Listening for rerender-only changes (timeout: 1s)...");

    tokio::select! {
        Some(change) = changes.next() => {
            print_change_event(&change, 1);
            if change.context.requires_rerender {
                println!("    ✅ Change requires rerender - would trigger UI update");

                // Demo application rerender logic
                match change.change_type {
                    ChangeType::DeviceProperty { ref property_name, .. } => {
                        println!("    🎨 Rerendering {} widget", property_name);
                    }
                    _ => {
                        println!("    🎨 Rerendering appropriate UI section");
                    }
                }
            }
        }
        _ = sleep(Duration::from_secs(1)) => {
            println!("  ⏱️ Rerender timeout");
        }
    }

    println!("  ✅ Rerender-only demo completed");
    Ok(())
}

/// Demo blocking iterator for synchronous contexts
fn demo_blocking_iterator(
    state_manager: &StateManager,
    speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Creating blocking change iterator...");

    let rt = tokio::runtime::Handle::current();
    let mut changes = state_manager.changes_blocking(rt.clone());

    // Simulate changes in another thread
    let state_manager_clone = state_manager.clone();
    let speaker_id_clone = speaker_id.clone();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));

        println!("  🔄 Simulating volume change from sync context");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(40));

        std::thread::sleep(Duration::from_millis(300));

        println!("  🔄 Simulating final volume change");
        state_manager_clone.update_property(&speaker_id_clone, Volume::new(30));
    });

    println!("  📡 Using blocking iterator (sync context)...");

    // In a real CLI app, you'd use changes.into_iter()
    // For demo, we'll use try_next with timeout simulation
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(1);

    let mut change_count = 0;
    while start.elapsed() < timeout && change_count < 2 {
        match changes.try_next() {
            Ok(Some(change)) => {
                change_count += 1;
                print_change_event(&change, change_count);
                println!("    ✅ Processed change in sync context");
            }
            Ok(None) => {
                // No changes available, sleep a bit
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                println!("    ❌ Error: {}", e);
                break;
            }
        }
    }

    println!("  ✅ Blocking iterator demo completed ({} changes)", change_count);
    Ok(())
}

/// Demo with simulated changes (when no devices found)
async fn demo_simulated_changes(
    state_manager: Arc<StateManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🎭 Demo: Simulated Changes (No Real Devices)");

    // Create a mock speaker ID
    let mock_speaker = SpeakerId::new("MOCK_DEVICE");

    println!("  Creating change stream for simulated device...");
    let mut changes = state_manager.changes();

    // Simulate device discovery and property changes
    let state_manager_clone = Arc::clone(&state_manager);
    let mock_speaker_clone = mock_speaker.clone();

    tokio::spawn(async move {
        sleep(Duration::from_millis(200)).await;

        println!("  🔄 Simulating device added");
        // Note: In real usage, device addition happens through add_devices()
        // Here we just simulate property changes

        println!("  🔄 Simulating volume changes...");
        state_manager_clone.update_property(&mock_speaker_clone, Volume::new(100));
        sleep(Duration::from_millis(300)).await;

        state_manager_clone.update_property(&mock_speaker_clone, Volume::new(75));
        sleep(Duration::from_millis(300)).await;

        state_manager_clone.update_property(&mock_speaker_clone, Volume::new(50));
        sleep(Duration::from_millis(300)).await;

        state_manager_clone.update_property(&mock_speaker_clone, Volume::new(25));
        sleep(Duration::from_millis(300)).await;

        state_manager_clone.update_property(&mock_speaker_clone, Volume::new(0));
    });

    println!("  📡 Listening for simulated changes...");

    let mut change_count = 0;
    let max_changes = 5;

    while change_count < max_changes {
        tokio::select! {
            Some(change) = changes.next() => {
                change_count += 1;
                print_change_event(&change, change_count);

                // Show how application would respond
                if change.context.requires_rerender {
                    match change.context.rerender_scope {
                        RerenderScope::Device(_) => {
                            println!("    🎨 Would update device UI");
                        }
                        RerenderScope::Full => {
                            println!("    🎨 Would refresh entire application UI");
                        }
                        _ => {
                            println!("    🎨 Would update relevant UI section");
                        }
                    }
                }
            }
            _ = sleep(Duration::from_secs(3)) => {
                println!("  ⏱️ Demo timeout");
                break;
            }
        }
    }

    println!("  ✅ Simulated changes demo completed");
    Ok(())
}

/// Helper function to print change events in a formatted way
fn print_change_event(change: &ChangeEvent, number: usize) {
    let elapsed = change.timestamp.elapsed();

    println!("  📨 Change #{} ({}ms ago):", number, elapsed.as_millis());
    println!("    📍 Type: {:?}", change.change_type);
    println!("    📝 Description: {}", change.context.description);
    println!("    🔄 Requires rerender: {}", change.context.requires_rerender);
    println!("    🎯 Rerender scope: {:?}", change.context.rerender_scope);
}

/// Helper trait to make ChangeFilter more chainable (for demo)
trait ChangeFilterExt {
    fn and_properties(self, properties: &[&'static str]) -> Self;
}

impl ChangeFilterExt for ChangeFilter {
    fn and_properties(mut self, properties: &[&'static str]) -> Self {
        self.property_names = Some(properties.iter().copied().collect());
        self
    }
}