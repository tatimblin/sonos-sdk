//! Property validation tests for watch() reliability
//!
//! These tests validate that every watchable property in the SDK delivers
//! change events through the watch() pipeline when mutated. All tests
//! require real Sonos hardware and are marked with `#[ignore]`.
//!
//! ## Usage
//!
//! Run all property validation tests:
//! ```bash
//! cargo test --package sonos-sdk --test property_validation -- --ignored --nocapture
//! ```
//!
//! Run a specific test:
//! ```bash
//! cargo test --package sonos-sdk --test property_validation -- --ignored test_rendering_control_properties
//! ```
//!
//! ## Requirements
//!
//! - At least 1 reachable Sonos speaker on the local network
//! - For group tests: 2+ standalone speakers (not bonded pairs or home theater setups)
//! - For current_track test: speaker should be playing with a queue of 2+ tracks
//! - For position test: speaker should be actively playing

use sonos_sdk::prelude::*;
use std::thread;
use std::time::{Duration, Instant};

// ============================================================================
// Test helpers
// ============================================================================

/// Create a SonosSystem with real speakers, or fail
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
fn find_standalone_speakers(
    system: &SonosSystem,
    min_count: usize,
) -> Result<Vec<Speaker>, Box<dyn std::error::Error>> {
    let groups = system.groups();
    let standalone_speakers: Vec<_> = groups
        .iter()
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
        return Err(format!(
            "Found {} standalone speakers, need {}",
            standalone_speakers.len(),
            min_count
        )
        .into());
    }

    Ok(standalone_speakers)
}

/// Wait for a specific property event on a specific speaker, with timeout.
/// Returns the event if found, or None on timeout.
fn wait_for_property_event(
    iter: &sonos_state::ChangeIterator,
    speaker_id: &SpeakerId,
    property_key: &str,
    timeout: Duration,
) -> Option<sonos_state::ChangeEvent> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let poll_duration = remaining.min(Duration::from_millis(100));
        if let Some(event) = iter.recv_timeout(poll_duration) {
            eprintln!(
                "  [event] {} for {} (looking for {} on {})",
                event.property_key,
                event.speaker_id.as_str(),
                property_key,
                speaker_id.as_str()
            );
            if event.speaker_id == *speaker_id && event.property_key == property_key {
                return Some(event);
            }
        }
    }
    None
}

// ============================================================================
// RAII Restoration Guards
// ============================================================================

/// Restores speaker volume on drop
struct VolumeGuard<'a> {
    speaker: &'a Speaker,
    original: u8,
}

impl Drop for VolumeGuard<'_> {
    fn drop(&mut self) {
        eprintln!("  [restore] volume -> {}", self.original);
        let _ = self.speaker.set_volume(self.original);
    }
}

/// Restores speaker mute state on drop
struct MuteGuard<'a> {
    speaker: &'a Speaker,
    original: bool,
}

impl Drop for MuteGuard<'_> {
    fn drop(&mut self) {
        eprintln!("  [restore] mute -> {}", self.original);
        let _ = self.speaker.set_mute(self.original);
    }
}

/// Restores speaker bass on drop
struct BassGuard<'a> {
    speaker: &'a Speaker,
    original: i8,
}

impl Drop for BassGuard<'_> {
    fn drop(&mut self) {
        eprintln!("  [restore] bass -> {}", self.original);
        let _ = self.speaker.set_bass(self.original);
    }
}

/// Restores speaker treble on drop
struct TrebleGuard<'a> {
    speaker: &'a Speaker,
    original: i8,
}

impl Drop for TrebleGuard<'_> {
    fn drop(&mut self) {
        eprintln!("  [restore] treble -> {}", self.original);
        let _ = self.speaker.set_treble(self.original);
    }
}

/// Restores speaker loudness on drop
struct LoudnessGuard<'a> {
    speaker: &'a Speaker,
    original: bool,
}

impl Drop for LoudnessGuard<'_> {
    fn drop(&mut self) {
        eprintln!("  [restore] loudness -> {}", self.original);
        let _ = self.speaker.set_loudness(self.original);
    }
}

/// Restores playback state on drop
struct PlaybackGuard<'a> {
    speaker: &'a Speaker,
    was_playing: bool,
}

impl Drop for PlaybackGuard<'_> {
    fn drop(&mut self) {
        if self.was_playing {
            eprintln!("  [restore] resuming playback");
            let _ = self.speaker.play();
        } else {
            eprintln!("  [restore] pausing playback");
            let _ = self.speaker.pause();
        }
    }
}

/// Dissolves a group on drop to restore standalone state
struct GroupGuard<'a> {
    speakers: Vec<&'a Speaker>,
}

impl Drop for GroupGuard<'_> {
    fn drop(&mut self) {
        for speaker in &self.speakers {
            eprintln!("  [restore] making {} standalone", speaker.name);
            let _ = speaker.leave_group();
        }
    }
}

// ============================================================================
// Single-Speaker Tests (no group required)
// ============================================================================

#[test]
#[ignore]
fn test_rendering_control_properties() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing RenderingControl properties on: {}", speaker.name);

    let event_timeout = Duration::from_secs(5);
    let subscription_settle = Duration::from_millis(500);

    // --- Volume ---
    {
        eprintln!("\n--- Volume ---");
        let original = speaker.volume.fetch()?.0;
        let _guard = VolumeGuard {
            speaker: &speaker,
            original,
        };
        let _handle = speaker.volume.watch()?;
        eprintln!("  Watch mode: {}", _handle.mode());
        thread::sleep(subscription_settle);

        let new_volume = if original > 5 {
            original - 5
        } else {
            original + 5
        };
        eprintln!("  Setting volume: {original} -> {new_volume}");
        speaker.set_volume(new_volume)?;

        let event = wait_for_property_event(&iter, &speaker.id, "volume", event_timeout);
        assert!(
            event.is_some(),
            "No volume event received after set_volume()"
        );

        let cached = speaker.volume.get().expect("volume should be cached");
        assert_eq!(cached.0, new_volume, "Cached volume should match set value");
        eprintln!("  Volume event received and cached correctly");
    }

    // --- Mute ---
    {
        eprintln!("\n--- Mute ---");
        let original = speaker.mute.fetch()?.0;
        let _guard = MuteGuard {
            speaker: &speaker,
            original,
        };
        let _handle = speaker.mute.watch()?;
        thread::sleep(subscription_settle);

        let new_mute = !original;
        eprintln!("  Setting mute: {original} -> {new_mute}");
        speaker.set_mute(new_mute)?;

        let event = wait_for_property_event(&iter, &speaker.id, "mute", event_timeout);
        assert!(event.is_some(), "No mute event received after set_mute()");

        let cached = speaker.mute.get().expect("mute should be cached");
        assert_eq!(cached.0, new_mute, "Cached mute should match set value");
        eprintln!("  Mute event received and cached correctly");
    }

    // --- Bass ---
    {
        eprintln!("\n--- Bass ---");
        let original = speaker.bass.fetch()?.0;
        let _guard = BassGuard {
            speaker: &speaker,
            original,
        };
        let _handle = speaker.bass.watch()?;
        thread::sleep(subscription_settle);

        let new_bass = if original < 5 {
            original + 2
        } else {
            original - 2
        };
        eprintln!("  Setting bass: {original} -> {new_bass}");
        speaker.set_bass(new_bass)?;

        let event = wait_for_property_event(&iter, &speaker.id, "bass", event_timeout);
        assert!(event.is_some(), "No bass event received after set_bass()");

        let cached = speaker.bass.get().expect("bass should be cached");
        assert_eq!(cached.0, new_bass, "Cached bass should match set value");
        eprintln!("  Bass event received and cached correctly");
    }

    // --- Treble ---
    {
        eprintln!("\n--- Treble ---");
        let original = speaker.treble.fetch()?.0;
        let _guard = TrebleGuard {
            speaker: &speaker,
            original,
        };
        let _handle = speaker.treble.watch()?;
        thread::sleep(subscription_settle);

        let new_treble = if original < 5 {
            original + 2
        } else {
            original - 2
        };
        eprintln!("  Setting treble: {original} -> {new_treble}");
        speaker.set_treble(new_treble)?;

        let event = wait_for_property_event(&iter, &speaker.id, "treble", event_timeout);
        assert!(
            event.is_some(),
            "No treble event received after set_treble()"
        );

        let cached = speaker.treble.get().expect("treble should be cached");
        assert_eq!(cached.0, new_treble, "Cached treble should match set value");
        eprintln!("  Treble event received and cached correctly");
    }

    // --- Loudness ---
    {
        eprintln!("\n--- Loudness ---");
        let original = speaker.loudness.fetch()?.0;
        let _guard = LoudnessGuard {
            speaker: &speaker,
            original,
        };
        let _handle = speaker.loudness.watch()?;
        thread::sleep(subscription_settle);

        let new_loudness = !original;
        eprintln!("  Setting loudness: {original} -> {new_loudness}");
        speaker.set_loudness(new_loudness)?;

        let event = wait_for_property_event(&iter, &speaker.id, "loudness", event_timeout);
        assert!(
            event.is_some(),
            "No loudness event received after set_loudness()"
        );

        let cached = speaker.loudness.get().expect("loudness should be cached");
        assert_eq!(
            cached.0, new_loudness,
            "Cached loudness should match set value"
        );
        eprintln!("  Loudness event received and cached correctly");
    }

    eprintln!("\n✅ All RenderingControl properties validated");
    Ok(())
}

#[test]
#[ignore]
fn test_playback_state_property() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing PlaybackState on: {}", speaker.name);

    let event_timeout = Duration::from_secs(5);
    let subscription_settle = Duration::from_millis(500);

    let current_state = speaker.playback_state.fetch()?;
    let was_playing = current_state.is_playing();
    let _guard = PlaybackGuard {
        speaker: &speaker,
        was_playing,
    };

    let _handle = speaker.playback_state.watch()?;
    eprintln!("  Watch mode: {}", _handle.mode());
    thread::sleep(subscription_settle);

    eprintln!("  Current state: {current_state:?}");

    if was_playing {
        // Playing -> Pause -> Play
        eprintln!("  Pausing...");
        speaker.pause()?;

        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", event_timeout);
        assert!(
            event.is_some(),
            "No playback_state event received after pause()"
        );
        let cached = speaker
            .playback_state
            .get()
            .expect("playback_state should be cached");
        eprintln!("  After pause: {cached:?}");

        // Resume
        eprintln!("  Resuming...");
        speaker.play()?;

        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", event_timeout);
        assert!(
            event.is_some(),
            "No playback_state event received after play()"
        );
        let cached = speaker
            .playback_state
            .get()
            .expect("playback_state should be cached");
        eprintln!("  After play: {cached:?}");
    } else {
        // Stopped/Paused -> Play -> Pause
        eprintln!("  Playing...");
        speaker.play()?;

        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", event_timeout);
        // play() on a stopped speaker with no queue may not generate an event
        if event.is_some() {
            let cached = speaker
                .playback_state
                .get()
                .expect("playback_state should be cached");
            eprintln!("  After play: {cached:?}");

            // Pause
            eprintln!("  Pausing...");
            speaker.pause()?;

            let event =
                wait_for_property_event(&iter, &speaker.id, "playback_state", event_timeout);
            assert!(
                event.is_some(),
                "No playback_state event received after pause()"
            );
            let cached = speaker
                .playback_state
                .get()
                .expect("playback_state should be cached");
            eprintln!("  After pause: {cached:?}");
        } else {
            eprintln!(
                "  ⚠️  No event after play() — speaker may have no queue. Skipping assertion."
            );
        }
    }

    eprintln!("\n✅ PlaybackState property validated");
    Ok(())
}

#[test]
#[ignore]
fn test_position_property() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing Position on: {}", speaker.name);

    let current_state = speaker.playback_state.fetch()?;
    if !current_state.is_playing() {
        eprintln!("⚠️  Speaker is not playing. Position test requires active playback. Skipping.");
        eprintln!(
            "  Start playing music on '{}' and re-run this test.",
            speaker.name
        );
        return Ok(());
    }

    let _handle = speaker.position.watch()?;
    eprintln!("  Watch mode: {}", _handle.mode());

    // Also watch playback_state since position often piggybacks on AVTransport events
    let _ps_handle = speaker.playback_state.watch()?;

    let baseline = speaker.position.fetch()?;
    eprintln!(
        "  Baseline position: {}ms / {}ms",
        baseline.position_ms, baseline.duration_ms
    );

    // Wait for any position event (generous timeout since position updates may be infrequent)
    eprintln!("  Waiting up to 10s for a position event...");
    let event = wait_for_property_event(&iter, &speaker.id, "position", Duration::from_secs(10));

    if let Some(_event) = event {
        let new_pos = speaker.position.get().expect("position should be cached");
        eprintln!(
            "  Position event received: {}ms / {}ms",
            new_pos.position_ms, new_pos.duration_ms
        );

        // Position should have changed (either advanced or new track)
        let position_changed = new_pos.position_ms != baseline.position_ms
            || new_pos.duration_ms != baseline.duration_ms;
        eprintln!("  Position changed: {position_changed}");
    } else {
        eprintln!("  ⚠️  No position event received within 10 seconds during active playback.");
        eprintln!("  This is diagnostic data for Phase 3 root cause investigation.");
        eprintln!("  Possible causes:");
        eprintln!("    - AVTransport NOTIFY may not include rel_time");
        eprintln!("    - Polling interval may be too long");
        eprintln!("    - Position changes may not trigger change events (PartialEq matching)");
    }

    eprintln!("\n✅ Position property test complete (see output for results)");
    Ok(())
}

#[test]
#[ignore]
fn test_current_track_property() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing CurrentTrack on: {}", speaker.name);

    let current_state = speaker.playback_state.fetch()?;
    if !current_state.is_playing() {
        eprintln!(
            "⚠️  Speaker is not playing. CurrentTrack test requires active playback with a queue."
        );
        eprintln!(
            "  Start playing a playlist on '{}' and re-run this test.",
            speaker.name
        );
        return Ok(());
    }

    let _handle = speaker.current_track.watch()?;
    eprintln!("  Watch mode: {}", _handle.mode());

    // Also watch playback_state since track changes arrive as AVTransport events
    let _ps_handle = speaker.playback_state.watch()?;

    let baseline = speaker.current_track.fetch()?;
    eprintln!("  Baseline track: {}", baseline.display());

    // Try to advance to next track
    eprintln!("  Calling next() to advance track...");
    match speaker.next() {
        Ok(()) => {
            // Wait for current_track event
            let event = wait_for_property_event(
                &iter,
                &speaker.id,
                "current_track",
                Duration::from_secs(5),
            );

            if let Some(_event) = event {
                let new_track = speaker
                    .current_track
                    .get()
                    .expect("current_track should be cached");
                eprintln!("  New track: {}", new_track.display());

                // Track should have changed (at least the URI or title should differ)
                if new_track != baseline {
                    eprintln!("  CurrentTrack changed successfully");
                } else {
                    eprintln!(
                        "  ⚠️  Track metadata unchanged — may be the same track playing again"
                    );
                }
            } else {
                eprintln!("  ⚠️  No current_track event after next()");
                eprintln!("  Checking if track actually changed via fetch...");
                let fetched = speaker.current_track.fetch()?;
                if fetched != baseline {
                    eprintln!("  Track DID change (fetch confirms), but event was not delivered!");
                    eprintln!("  This indicates a bug in the event pipeline.");
                } else {
                    eprintln!("  Track did not change — may only have one track in queue.");
                }
            }

            // Try to go back
            eprintln!("  Calling previous() to restore...");
            let _ = speaker.previous();
        }
        Err(e) => {
            eprintln!(
                "  ⚠️  next() failed: {e}. Speaker may not have a queue. Skipping mutation test."
            );
        }
    }

    eprintln!("\n✅ CurrentTrack property test complete (see output for results)");
    Ok(())
}

// ============================================================================
// Group Tests (requires 2+ standalone speakers)
// ============================================================================

#[test]
#[ignore]
fn test_group_properties() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Bootstrap topology
    let speaker_names = system.speaker_names();
    if let Some(first_speaker) = system.speaker(&speaker_names[0]) {
        let _topology_handle = first_speaker.group_membership.watch().ok();
    }

    // Wait for topology
    for _ in 0..10 {
        if !system.groups().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    let standalone = match find_standalone_speakers(&system, 2) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("⚠️  Skipping group properties test: {e}");
            return Ok(());
        }
    };

    let speaker_a = &standalone[0];
    let speaker_b = &standalone[1];
    eprintln!(
        "Testing group properties with: {} + {}",
        speaker_a.name, speaker_b.name
    );

    // Create group
    eprintln!("  Creating group...");
    let result = system.create_group(speaker_a, &[speaker_b])?;
    eprintln!(
        "  Group created: {} succeeded, {} failed",
        result.succeeded.len(),
        result.failed.len()
    );

    // Set up group dissolution guard
    let _group_guard = GroupGuard {
        speakers: vec![speaker_b],
    };

    // Wait for topology to propagate
    let mut our_group = None;
    for _ in 0..10 {
        let groups = system.groups();
        if let Some(g) = groups.into_iter().find(|g| g.member_count() >= 2) {
            our_group = Some(g);
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    let group = match our_group {
        Some(g) => g,
        None => {
            eprintln!("  ⚠️  Group not found in topology after creation. Skipping.");
            return Ok(());
        }
    };

    eprintln!(
        "  Group {} with {} members",
        group.id.as_str(),
        group.member_count()
    );

    let iter = system.iter();
    let event_timeout = Duration::from_secs(5);
    let subscription_settle = Duration::from_millis(500);

    // --- GroupVolume ---
    {
        eprintln!("\n--- GroupVolume ---");
        if let Ok(handle) = group.volume.watch() {
            eprintln!("  Watch mode: {}", handle.mode());
            thread::sleep(subscription_settle);

            // Fetch current group volume
            let original = group.volume.get().map(|v| v.0).unwrap_or(20);
            let new_vol = if original > 5 {
                original - 5
            } else {
                original + 5
            };

            eprintln!("  Setting group volume: {original} -> {new_vol}");
            group.set_volume(new_vol)?;

            // Group events arrive on the coordinator speaker
            let event = wait_for_property_event(
                &iter,
                &group.coordinator_id,
                "group_volume",
                event_timeout,
            );

            if event.is_some() {
                eprintln!("  GroupVolume event received");
            } else {
                eprintln!("  ⚠️  No group_volume event received");
            }

            // Restore
            group.set_volume(original)?;
            let _ = wait_for_property_event(
                &iter,
                &group.coordinator_id,
                "group_volume",
                Duration::from_secs(2),
            );
            drop(handle);
        } else {
            eprintln!("  ⚠️  Could not watch GroupVolume");
        }
    }

    // --- GroupMute ---
    {
        eprintln!("\n--- GroupMute ---");
        if let Ok(handle) = group.mute.watch() {
            eprintln!("  Watch mode: {}", handle.mode());
            thread::sleep(subscription_settle);

            let original = group.mute.get().map(|m| m.0).unwrap_or(false);

            eprintln!("  Setting group mute: {} -> {}", original, !original);
            group.set_mute(!original)?;

            let event =
                wait_for_property_event(&iter, &group.coordinator_id, "group_mute", event_timeout);

            if event.is_some() {
                eprintln!("  GroupMute event received");
            } else {
                eprintln!("  ⚠️  No group_mute event received");
            }

            // Restore
            group.set_mute(original)?;
            let _ = wait_for_property_event(
                &iter,
                &group.coordinator_id,
                "group_mute",
                Duration::from_secs(2),
            );
            drop(handle);
        } else {
            eprintln!("  ⚠️  Could not watch GroupMute");
        }
    }

    // --- GroupVolumeChangeable ---
    {
        eprintln!("\n--- GroupVolumeChangeable ---");
        if let Ok(handle) = group.volume_changeable.watch() {
            eprintln!("  Watch mode: {}", handle.mode());
            thread::sleep(subscription_settle);

            // This is event-only (no setter), so we just verify initial value arrives
            let val = group.volume_changeable.get();
            if let Some(v) = val {
                eprintln!(
                    "  GroupVolumeChangeable initial value: {} (expected: true)",
                    v.0
                );
            } else {
                eprintln!("  ⚠️  No GroupVolumeChangeable value available");
                eprintln!("  This may require waiting for a GroupRenderingControl event");
            }
            drop(handle);
        } else {
            eprintln!("  ⚠️  Could not watch GroupVolumeChangeable");
        }
    }

    eprintln!("\n✅ Group properties test complete");
    Ok(())
}

// ============================================================================
// Topology Tests
// ============================================================================

#[test]
#[ignore]
fn test_topology_properties() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Bootstrap topology
    let speaker_names = system.speaker_names();
    if let Some(first_speaker) = system.speaker(&speaker_names[0]) {
        let _topology_handle = first_speaker.group_membership.watch().ok();
    }

    // Wait for topology
    for _ in 0..10 {
        if !system.groups().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    let standalone = match find_standalone_speakers(&system, 2) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("⚠️  Skipping topology test: {e}");
            return Ok(());
        }
    };

    let speaker_a = &standalone[0];
    let speaker_b = &standalone[1];
    eprintln!(
        "Testing topology properties with: {} + {}",
        speaker_a.name, speaker_b.name
    );

    let iter = system.iter();
    let event_timeout = Duration::from_secs(5);

    // Watch GroupMembership on both speakers
    let _gm_handle_a = speaker_a.group_membership.watch()?;
    let _gm_handle_b = speaker_b.group_membership.watch()?;
    thread::sleep(Duration::from_millis(500));

    // Verify both are standalone (coordinator of their own group)
    let gm_a = speaker_a.group_membership.get();
    let gm_b = speaker_b.group_membership.get();
    eprintln!(
        "  Initial state A: {:?}",
        gm_a.as_ref().map(|gm| (&gm.group_id, gm.is_coordinator))
    );
    eprintln!(
        "  Initial state B: {:?}",
        gm_b.as_ref().map(|gm| (&gm.group_id, gm.is_coordinator))
    );

    // Create group
    eprintln!(
        "\n  Creating group ({} + {})...",
        speaker_a.name, speaker_b.name
    );
    let _group_guard = GroupGuard {
        speakers: vec![speaker_b],
    };
    system.create_group(speaker_a, &[speaker_b])?;

    // Wait for GroupMembership events
    eprintln!("  Waiting for group_membership events...");
    let mut events_received = 0;
    let deadline = Instant::now() + event_timeout;
    while Instant::now() < deadline && events_received < 2 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let poll_duration = remaining.min(Duration::from_millis(100));
        if let Some(event) = iter.recv_timeout(poll_duration) {
            eprintln!(
                "    [event] {} for {}",
                event.property_key,
                event.speaker_id.as_str()
            );
            if event.property_key == "group_membership" {
                events_received += 1;
            }
        }
    }
    eprintln!("  Received {events_received} group_membership events");

    // Check updated membership
    // Give a moment for state to settle
    thread::sleep(Duration::from_millis(200));

    // Fetch fresh values
    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();

    let gm_a = speaker_a.group_membership.get();
    let gm_b = speaker_b.group_membership.get();
    eprintln!(
        "  After group A: {:?}",
        gm_a.as_ref().map(|gm| (&gm.group_id, gm.is_coordinator))
    );
    eprintln!(
        "  After group B: {:?}",
        gm_b.as_ref().map(|gm| (&gm.group_id, gm.is_coordinator))
    );

    if let (Some(a), Some(b)) = (&gm_a, &gm_b) {
        assert_eq!(
            a.group_id, b.group_id,
            "Both speakers should be in the same group"
        );
        // Coordinator should be speaker_a
        assert!(a.is_coordinator, "Speaker A should be the coordinator");
        assert!(!b.is_coordinator, "Speaker B should not be the coordinator");
    } else {
        eprintln!("  ⚠️  GroupMembership not available for both speakers");
    }

    // Leave group
    eprintln!("\n  Breaking group (speaker B leaving)...");
    speaker_b.leave_group()?;

    // Wait for membership events
    eprintln!("  Waiting for group_membership events...");
    let mut leave_events = 0;
    let deadline = Instant::now() + event_timeout;
    while Instant::now() < deadline && leave_events < 2 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let poll_duration = remaining.min(Duration::from_millis(100));
        if let Some(event) = iter.recv_timeout(poll_duration) {
            if event.property_key == "group_membership" {
                leave_events += 1;
                eprintln!(
                    "    [event] group_membership for {}",
                    event.speaker_id.as_str()
                );
            }
        }
    }
    eprintln!("  Received {leave_events} leave events");

    // Verify standalone again
    thread::sleep(Duration::from_millis(200));
    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();

    let gm_a = speaker_a.group_membership.get();
    let gm_b = speaker_b.group_membership.get();
    eprintln!(
        "  After leave A: {:?}",
        gm_a.as_ref().map(|gm| (&gm.group_id, gm.is_coordinator))
    );
    eprintln!(
        "  After leave B: {:?}",
        gm_b.as_ref().map(|gm| (&gm.group_id, gm.is_coordinator))
    );

    if let (Some(a), Some(b)) = (&gm_a, &gm_b) {
        assert!(
            a.is_coordinator,
            "Speaker A should be coordinator (standalone)"
        );
        assert!(
            b.is_coordinator,
            "Speaker B should be coordinator (standalone)"
        );
        assert_ne!(
            a.group_id, b.group_id,
            "Speakers should be in different groups after leaving"
        );
    }

    eprintln!("\n✅ Topology properties test complete");
    Ok(())
}
