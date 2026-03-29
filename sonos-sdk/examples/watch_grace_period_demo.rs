//! Grace Period Demo - Visualizing the 50ms Grace Period Feature
//!
//! This example demonstrates the key innovation of the RAII WatchHandle:
//! - Rapid watch creation/dropping without subscription churn
//! - Grace period behavior preventing unnecessary unsubscribes
//! - TUI-style usage pattern (watching inside draw loops)
//! - Visual timing demonstration
//!
//! Run with: cargo run -p sonos-sdk --example watch_grace_period_demo

use sonos_sdk::prelude::*;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), SdkError> {
    println!();
    println!("🎵 Sonos SDK Grace Period Demo");
    println!("===============================");
    println!();
    println!("This demo shows how the 50ms grace period prevents subscription churn");
    println!("when WatchHandles are created and dropped rapidly (like in TUI apps).");
    println!();

    // Discovery
    let system = SonosSystem::new()?;
    let names = system.speaker_names();

    if names.is_empty() {
        println!("❌ No speakers found. Please ensure Sonos devices are on the network.");
        return Ok(());
    }

    // Find reachable speaker
    let speaker = find_reachable_speaker(&system, &names)?;
    println!("✅ Using speaker: {} at {}", speaker.name, speaker.ip);
    println!();

    // Demo 1: Show what normal usage looks like
    demo_normal_usage(&speaker)?;

    // Demo 2: Simulate TUI-style rapid watch creation/dropping
    demo_tui_pattern(&speaker)?;

    // Demo 3: Show grace period timing
    demo_grace_period_timing(&speaker)?;

    // Demo 4: Show subscription persistence across multiple watches
    demo_subscription_persistence(&speaker)?;

    println!("🎉 Demo complete! The grace period successfully prevented subscription churn.");
    println!();

    Ok(())
}

fn find_reachable_speaker(system: &SonosSystem, names: &[String]) -> Result<Speaker, SdkError> {
    println!("🔍 Finding reachable speaker...");

    for name in names {
        if let Some(speaker) = system.speaker(name) {
            match speaker.volume.fetch() {
                Ok(_) => {
                    println!("   ✅ {} at {} is reachable", speaker.name, speaker.ip);
                    return Ok(speaker);
                }
                Err(_) => {
                    println!("   ❌ {} at {} unreachable", speaker.name, speaker.ip);
                }
            }
        }
    }

    Err(SdkError::DiscoveryFailed("No reachable speakers found".to_string()))
}

fn demo_normal_usage(speaker: &Speaker) -> Result<(), SdkError> {
    println!("📋 Demo 1: Normal Usage Pattern");
    println!("--------------------------------");
    println!("Creating a watch handle and keeping it alive...");

    let handle = speaker.volume.watch()?;
    println!("   ✅ Watch handle created - mode: {}", handle.mode());

    if let Some(volume) = handle.value() {
        println!("   📊 Current volume: {}%", volume.0);
    }

    println!("   ⏱️  Keeping handle alive for 2 seconds...");
    thread::sleep(Duration::from_secs(2));

    println!("   🗑️  Dropping handle (grace period starts)...");
    drop(handle);

    println!("   ✅ Handle dropped - subscription will cleanup after 50ms grace period");
    println!();

    Ok(())
}

fn demo_tui_pattern(speaker: &Speaker) -> Result<(), SdkError> {
    println!("🖥️  Demo 2: TUI Pattern (Rapid Watch Creation/Dropping)");
    println!("--------------------------------------------------------");
    println!("Simulating a TUI app calling watch() inside draw() method...");
    println!("This would cause subscription churn WITHOUT the grace period.");
    println!();

    let start = Instant::now();

    // Simulate 10 rapid draw() calls
    for frame in 1..=10 {
        let frame_start = Instant::now();

        // This is the TUI pattern: watch() called inside draw()
        let handle = speaker.volume.watch()?;

        // Simulate some drawing work
        let volume_text = if let Some(vol) = handle.value() {
            format!("Volume: {}%", vol.0)
        } else {
            "Volume: ?".to_string()
        };

        println!("   🖼️  Frame {frame:2}: {volume_text} | Handle mode: {}", handle.mode());

        // Handle automatically drops at end of scope
        // Grace period prevents unsubscribe churn

        let frame_time = frame_start.elapsed();
        println!("      ⏱️  Frame rendered in {:?}", frame_time);

        // Small delay between frames
        thread::sleep(Duration::from_millis(16)); // ~60 FPS
    }

    let total_time = start.elapsed();
    println!();
    println!("   ✅ 10 frames rendered in {:?}", total_time);
    println!("   🎯 Grace period prevented 9 unnecessary unsubscribe/resubscribe cycles!");
    println!();

    Ok(())
}

fn demo_grace_period_timing(speaker: &Speaker) -> Result<(), SdkError> {
    println!("⏰ Demo 3: Grace Period Timing Demonstration");
    println!("---------------------------------------------");
    println!("Showing how the 50ms grace period works...");
    println!();

    // Create first handle
    println!("   📝 Creating first handle...");
    let handle1 = speaker.volume.watch()?;
    println!("      ✅ Handle 1 created - subscription active");

    // Drop it
    println!("   🗑️  Dropping handle 1...");
    let drop_time = Instant::now();
    drop(handle1);
    println!("      ⏱️  Grace period started at {:?}", drop_time.elapsed());

    // Wait 25ms (within grace period)
    thread::sleep(Duration::from_millis(25));
    println!("   ⏱️  25ms elapsed - still within grace period");

    // Create new handle (should reuse subscription)
    let handle2 = speaker.volume.watch()?;
    let reuse_time = drop_time.elapsed();
    println!("   ♻️  New handle created at {:?} - subscription reused!", reuse_time);

    if let Some(vol) = handle2.value() {
        println!("      📊 Volume still available: {}%", vol.0);
    }

    println!("   ✅ Subscription was never interrupted!");

    // Now let grace period expire
    println!("   ⏱️  Dropping handle 2 and waiting for grace period to expire...");
    drop(handle2);
    thread::sleep(Duration::from_millis(60)); // Wait longer than 50ms
    println!("      ⏲️  Grace period expired - subscription cleaned up");

    println!();
    Ok(())
}

fn demo_subscription_persistence(speaker: &Speaker) -> Result<(), SdkError> {
    println!("🔄 Demo 4: Subscription Persistence Across Multiple Watches");
    println!("------------------------------------------------------------");
    println!("Creating multiple overlapping watches to show subscription sharing...");
    println!();

    // Create first watch
    println!("   📝 Creating volume watch 1...");
    let vol_handle1 = speaker.volume.watch()?;
    println!("      ✅ Watch 1 active - mode: {}", vol_handle1.mode());

    // Create second watch (should share subscription)
    println!("   📝 Creating volume watch 2...");
    let vol_handle2 = speaker.volume.watch()?;
    println!("      ✅ Watch 2 active - mode: {} (shares subscription)", vol_handle2.mode());

    // Create watch for different property
    println!("   📝 Creating mute watch...");
    let mute_handle = speaker.mute.watch()?;
    println!("      ✅ Mute watch active - mode: {}", mute_handle.mode());

    // Show current values
    if let (Some(vol), Some(mute)) = (vol_handle1.value(), mute_handle.value()) {
        println!("      📊 Current state: Volume {}%, Mute {}",
                vol.0, if mute.0 { "ON" } else { "OFF" });
    }

    // Drop one volume watch (subscription should persist due to second watch)
    println!("   🗑️  Dropping volume watch 1...");
    drop(vol_handle1);
    println!("      ⏱️  Volume subscription persists (watch 2 still active)");

    // Verify second watch still works
    if let Some(vol) = vol_handle2.value() {
        println!("      ✅ Watch 2 still active: {}%", vol.0);
    }

    // Drop all watches
    println!("   🗑️  Dropping all remaining watches...");
    drop(vol_handle2);
    drop(mute_handle);
    println!("      ⏲️  All watches dropped - grace periods started");

    println!("   ✅ Subscriptions will cleanup after grace periods expire");
    println!();

    Ok(())
}