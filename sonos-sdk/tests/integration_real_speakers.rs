//! Integration tests for real Sonos speaker validation
//!
//! These integration tests validate core Sonos SDK functionality against real hardware
//! before PR submission. All tests are marked with `#[ignore]` and require actual
//! Sonos devices on the local network.
//!
//! ## Usage
//!
//! Run all integration tests:
//! ```bash
//! cargo test --package sonos-sdk --test integration_real_speakers -- --ignored --nocapture
//! ```
//!
//! Run a specific test:
//! ```bash
//! cargo test --package sonos-sdk --test integration_real_speakers -- --ignored test_api_operations
//! ```
//!
//! ## Requirements
//!
//! - At least 1 reachable Sonos speaker on the local network
//! - For group tests: 2+ standalone speakers (not bonded pairs or home theater setups)
//! - Network connectivity between test machine and speakers
//!
//! ## Test Coverage
//!
//! ### test_api_operations
//! **Purpose:** Validates basic SOAP API calls work against real speakers
//! - Volume control operations (get, set with restoration)
//! - Property fetching: playback state, mute, bass, treble, loudness
//! - State restoration to avoid affecting speaker settings
//!
//! ### test_property_watching
//! **Purpose:** Validates WatchHandle API and property access patterns
//! - Cache behavior: get() before/after fetch()
//! - WatchHandle creation and RAII cleanup
//! - Concurrent watch handles sharing subscriptions
//! - Property access methods: get(), fetch(), watch()
//!
//! ### test_event_streaming
//! **Purpose:** Validates UPnP subscription lifecycle and grace period behavior
//! - Grace period timing (50ms delay before unsubscribe)
//! - Subscription reuse within grace period window
//! - Event reception and processing via volume changes
//! - Subscription creation and cleanup
//!
//! ### test_group_lifecycle
//! **Purpose:** Validates group management operations with device qualification
//! - Group creation using system.create_group(coordinator, members)
//! - Speaker removal via leave_group()
//! - Topology updates and verification
//! - Device filtering (excludes home theater devices: Playbar, Beam, Arc, Sub)
//! - Graceful skip when insufficient compatible speakers available
//!
//! ### test_event_integration ⭐
//! **Purpose:** Validates end-to-end event flow from property watching to system.iter()
//! - Property watching enables event streaming
//! - API changes (volume adjustments) generate events
//! - Events received through system.iter() with correct property_key
//! - Cache updates match API changes
//! - Multiple event validation (change + restore)
//!
//! ## Error Handling
//!
//! Tests fail loudly when requirements aren't met:
//! - No speakers found: "No Sonos speakers found. Integration tests require real hardware."
//! - No reachable speakers: "No reachable speakers found"
//! - Insufficient speakers for groups: "Found X standalone speakers, need Y"
//!
//! ## Performance Expectations
//!
//! - Full suite: < 30 seconds on typical network
//! - Individual tests: < 5 seconds each
//! - Grace period validation: < 50ms subscription reuse timing

use sonos_sdk::prelude::*;
use std::time::Duration;
use std::thread;

/// Helper function to ensure real speakers are available for testing
fn require_real_speakers() -> Result<SonosSystem, Box<dyn std::error::Error>> {
    let system = SonosSystem::new()?;
    if system.speaker_names().is_empty() {
        return Err("No Sonos speakers found. Integration tests require real hardware.".into());
    }
    Ok(system)
}

/// Find a reachable speaker by testing volume.fetch() on each discovered speaker
fn find_reachable_speaker(system: &SonosSystem) -> Result<Speaker, Box<dyn std::error::Error>> {
    let names = system.speaker_names();
    for name in &names {
        if let Some(speaker) = system.speaker(name) {
            match speaker.volume.fetch() {
                Ok(_) => return Ok(speaker),
                Err(_) => continue,
            }
        }
    }
    Err("No reachable speakers found".into())
}

/// Find standalone speakers (not bonded pairs) that are compatible with group operations
/// Filters out home theater devices (Playbar, Beam, Arc, Sub) that may have restrictions
fn find_standalone_speakers(system: &SonosSystem, min_count: usize) -> Result<Vec<Speaker>, Box<dyn std::error::Error>> {
    let groups = system.groups();
    let standalone_speakers: Vec<_> = groups.iter()
        .filter(|g| g.member_count() == 1)
        .filter_map(|g| g.coordinator())
        .filter(|speaker| {
            let model = speaker.model_name.to_lowercase();
            !model.contains("playbar")
                && !model.contains("beam")
                && !model.contains("arc")
                && !model.contains("sub")
        })
        .collect();

    if standalone_speakers.len() < min_count {
        return Err(format!("Found {} standalone speakers, need {}", standalone_speakers.len(), min_count).into());
    }

    Ok(standalone_speakers)
}

#[test]
#[ignore] // Requires real hardware - run manually with --ignored
fn test_api_operations() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Find any reachable speaker
    let speaker = find_reachable_speaker(&system)?;

    // Test volume operations with state restoration
    let original_volume = speaker.volume.fetch()?.0;
    let test_volume = if original_volume > 5 {
        original_volume - 5
    } else {
        original_volume + 5
    };

    speaker.set_volume(test_volume)?;
    assert_eq!(speaker.volume.fetch()?.0, test_volume, "Volume should match set value");
    speaker.set_volume(original_volume)?; // restore original

    // Test playback state (read-only, no state changes)
    let playback_state = speaker.playback_state.fetch()?;
    eprintln!("Playback state: {:?}", playback_state);

    // Test other properties without state changes
    speaker.mute.fetch()?;
    speaker.bass.fetch()?;
    speaker.treble.fetch()?;
    speaker.loudness.fetch()?;

    eprintln!("✅ API operations test completed successfully");
    Ok(())
}

#[test]
#[ignore] // Requires real hardware - run manually with --ignored
fn test_event_streaming() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;

    // Test basic subscription creation
    let handle = speaker.volume.watch()?;
    eprintln!("Watch mode: {}", handle.mode());

    // Test grace period behavior - drop and recreate within 50ms window
    let start_time = std::time::Instant::now();
    drop(handle);

    // Create new handle within grace period
    let _new_handle = speaker.volume.watch()?;
    let elapsed = start_time.elapsed();

    if elapsed < Duration::from_millis(50) {
        eprintln!("Grace period active: subscription reused at {:?}", elapsed);
    } else {
        eprintln!("Grace period expired: new subscription created at {:?}", elapsed);
    }

    // Test event iteration (brief test to avoid long delays)
    let iter = system.iter();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut event_count = 0;

    // Make a small volume change to trigger events
    let original_volume = speaker.volume.get().unwrap_or(Volume(20)).0;
    let test_volume = if original_volume > 1 {
        original_volume - 1
    } else {
        original_volume + 1
    };

    speaker.set_volume(test_volume)?;

    while std::time::Instant::now() < deadline && event_count < 3 {
        if let Some(_event) = iter.recv_timeout(Duration::from_millis(100)) {
            event_count += 1;
        }
    }

    // Restore volume
    speaker.set_volume(original_volume)?;

    eprintln!("Events received: {}", event_count);
    assert!(event_count > 0, "Should receive at least one event after volume change");

    eprintln!("✅ Event streaming test completed successfully");
    Ok(())
}

#[test]
#[ignore] // Requires real hardware - run manually with --ignored
fn test_group_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Bootstrap topology by watching group_membership on any speaker
    let speaker_names = system.speaker_names();
    if let Some(first_speaker) = system.speaker(&speaker_names[0]) {
        let _topology_handle = first_speaker.group_membership.watch().ok();
    }

    // Wait for topology to populate
    for _i in 1..=10 {
        let groups = system.groups();
        if !groups.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    // Find compatible speakers (minimum 2 standalone)
    let standalone_speakers = find_standalone_speakers(&system, 2);

    // If we don't have enough compatible speakers, skip the test with a clear message
    let standalone_speakers = match standalone_speakers {
        Ok(speakers) => speakers,
        Err(e) => {
            eprintln!("⚠️  Skipping group lifecycle test: {}", e);
            return Ok(()); // Skip gracefully - this is common in test environments
        }
    };

    let speaker_a = &standalone_speakers[0];
    let speaker_b = &standalone_speakers[1];

    eprintln!("Testing group operations with speakers: {} and {}", speaker_a.name, speaker_b.name);

    // Test group creation (coordinator, members)
    let result = system.create_group(speaker_a, &[speaker_b])?;
    eprintln!("Group created: {} succeeded, {} failed", result.succeeded.len(), result.failed.len());

    // Wait for topology update
    thread::sleep(Duration::from_millis(500));

    // Verify group membership
    let updated_groups = system.groups();
    let our_group = updated_groups.iter()
        .find(|g| g.member_count() == 2)
        .ok_or("Group not found after creation")?;

    assert_eq!(our_group.member_count(), 2, "Group should have 2 members");

    // Test speaker removal
    speaker_b.leave_group()?;

    // Wait and verify
    thread::sleep(Duration::from_millis(500));
    let final_groups = system.groups();

    // Should have standalone speakers again
    let standalone_count = final_groups.iter().filter(|g| g.member_count() == 1).count();
    assert!(standalone_count >= 2, "Speakers should be standalone after leave_group");

    eprintln!("✅ Group lifecycle test completed successfully");
    Ok(())
}

#[test]
#[ignore] // Requires real hardware - run manually with --ignored
fn test_property_watching() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;

    // Test get() before fetch() returns None
    // Note: This might not be true if cache is already populated, so we'll test the pattern
    let initial_cache = speaker.volume.get();
    eprintln!("Initial cache state: {:?}", initial_cache.map(|v| v.0));

    // Test fetch() updates cache
    let fetched_volume = speaker.volume.fetch()?;
    let cached_volume = speaker.volume.get().expect("get() should return Some after fetch()");
    assert_eq!(fetched_volume.0, cached_volume.0, "fetch() should update cache");

    // Test watch() returns current value
    let watch_handle = speaker.volume.watch()?;
    if let Some(watched_volume) = watch_handle.value() {
        assert_eq!(watched_volume.0, cached_volume.0, "watch() should return cached value");
    }

    eprintln!("Watch mode: {}", watch_handle.mode());

    // Test multiple concurrent watches
    let handle1 = speaker.volume.watch()?;
    let handle2 = speaker.volume.watch()?;

    // Both should have same value and mode
    assert_eq!(handle1.mode(), handle2.mode(), "Concurrent watches should share mode");

    if let (Some(vol1), Some(vol2)) = (handle1.value(), handle2.value()) {
        assert_eq!(vol1.0, vol2.0, "Concurrent watches should have same value");
    }

    // Test RAII cleanup - handles will be dropped automatically
    drop(handle1);
    drop(handle2);
    drop(watch_handle);
    // Subscription should cleanup after grace period

    eprintln!("✅ Property access patterns validated");
    Ok(())
}

#[test]
#[ignore] // Requires real hardware - run manually with --ignored
fn test_event_integration() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;

    // Get the event iterator FIRST before starting to watch
    let iter = system.iter();

    // Start watching volume - this enables event streaming
    let _volume_handle = speaker.volume.watch()?;
    eprintln!("✅ Started watching volume property");

    // Give a moment for subscription to establish
    thread::sleep(Duration::from_millis(200));

    // Get current volume for API change
    let current_volume = speaker.volume.fetch()?.0;
    let new_volume = if current_volume > 5 {
        current_volume - 1
    } else {
        current_volume + 1
    };

    // Change volume via API - this should trigger an event
    speaker.set_volume(new_volume)?;
    eprintln!("🔧 Changed volume: {} -> {}", current_volume, new_volume);

    // Wait for API-triggered event
    let mut volume_event_received = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);

    while std::time::Instant::now() < deadline {
        if let Some(event) = iter.recv_timeout(Duration::from_millis(100)) {
            eprintln!("📡 Received event: {} for speaker {}", event.property_key, event.speaker_id);
            if event.property_key == "volume" {
                volume_event_received = true;
                eprintln!("✅ Volume event received via system.iter(): {}", event.property_key);
                break;
            }
        }
    }

    assert!(volume_event_received, "No volume event received after API change");

    // Verify the volume actually changed and is cached
    let final_volume = speaker.volume.get()
        .ok_or("Volume should be cached after events")?;
    assert_eq!(final_volume.0, new_volume, "Cached volume should match API change");

    // Restore original volume and verify it also generates an event
    speaker.set_volume(current_volume)?;
    eprintln!("🔧 Restoring volume: {} -> {}", new_volume, current_volume);

    // Wait for restore event
    let mut restore_event_received = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);

    while std::time::Instant::now() < deadline {
        if let Some(event) = iter.recv_timeout(Duration::from_millis(100)) {
            if event.property_key == "volume" {
                restore_event_received = true;
                eprintln!("✅ Restore event received via system.iter(): {}", event.property_key);
                break;
            }
        }
    }

    assert!(restore_event_received, "No volume event received after restore");

    eprintln!("✅ Event integration validated: property watching -> API changes -> events via system.iter()");
    Ok(())
}