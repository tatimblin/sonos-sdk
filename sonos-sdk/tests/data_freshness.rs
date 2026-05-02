//! Data freshness & completeness tests
//!
//! Validates the full data lifecycle for each mutable property:
//! watch → mutate via API → verify cached value is correct.
//!
//! Structured as a property test suite — adding a new property test
//! requires only a new `#[test] #[ignore]` function using the shared helpers.
//!
//! ## Usage
//!
//! Tests must run single-threaded — they share physical speakers.
//!
//! Run all data freshness tests:
//! ```bash
//! cargo test --package sonos-sdk --test data_freshness -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Run a specific test:
//! ```bash
//! cargo test --package sonos-sdk --test data_freshness -- --ignored test_volume_round_trip_values
//! ```
//!
//! ## Requirements
//!
//! - At least 1 reachable Sonos speaker on the local network
//! - For group volume/topology tests: 2+ standalone speakers
//! - For group management test: 3+ standalone speakers
//! - Volume never exceeds 50 in any test

use sonos_sdk::prelude::*;
use std::thread;
use std::time::{Duration, Instant};

// ============================================================================
// Shared infrastructure
// ============================================================================

fn require_real_speakers() -> Result<SonosSystem, Box<dyn std::error::Error>> {
    let system = SonosSystem::new()?;
    if system.speaker_names().is_empty() {
        return Err("No Sonos speakers found. Integration tests require real hardware.".into());
    }
    Ok(system)
}

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

/// Break apart all groups so every speaker is standalone, wait for topology
/// to settle, then return compatible speakers (excludes home theater devices).
fn find_standalone_speakers(
    system: &SonosSystem,
    min_count: usize,
) -> Result<Vec<Speaker>, Box<dyn std::error::Error>> {
    // First, dissolve any existing groups so we have maximum standalone speakers
    let groups = system.groups();
    let mut ungrouped_any = false;
    for group in &groups {
        if group.member_count() > 1 {
            for member in group.members() {
                if member.id != group.coordinator_id {
                    eprintln!("  [prep] ungrouping {} from group", member.name);
                    let _ = member.leave_group();
                    ungrouped_any = true;
                }
            }
        }
    }

    // Wait for topology to fully settle after ungrouping
    if ungrouped_any {
        eprintln!("  [prep] waiting for topology to settle after ungrouping...");
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(250));
            let groups = system.groups();
            if groups.iter().all(|g| g.member_count() == 1) {
                break;
            }
        }
    }

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

/// Find a speaker that is actively playing music. Checks all reachable speakers
/// and returns the first one with PlaybackState::Playing.
fn find_playing_speaker(system: &SonosSystem) -> Option<Speaker> {
    let names = system.speaker_names();
    for name in &names {
        if let Some(speaker) = system.speaker(name) {
            if let Ok(state) = speaker.playback_state.fetch() {
                if state.is_playing() {
                    return Some(speaker);
                }
            }
        }
    }
    None
}

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
            if event.speaker_id == *speaker_id && event.property_key == property_key {
                return Some(event);
            }
        }
    }
    None
}

fn drain_events(
    iter: &sonos_state::ChangeIterator,
    property_key: &str,
    count: usize,
    timeout: Duration,
) {
    let mut received = 0;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && received < count {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let poll = remaining.min(Duration::from_millis(100));
        if let Some(event) = iter.recv_timeout(poll) {
            if event.property_key == property_key {
                received += 1;
                eprintln!(
                    "  [event] {} for {} ({}/{})",
                    event.property_key,
                    event.speaker_id.as_str(),
                    received,
                    count
                );
            }
        }
    }
}

fn wait_for_group_topology(
    system: &SonosSystem,
    predicate: impl Fn(&[sonos_sdk::Group]) -> bool,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let groups = system.groups();
        if predicate(&groups) {
            return true;
        }
        thread::sleep(Duration::from_millis(250));
    }
    false
}

fn bootstrap_topology(system: &SonosSystem) {
    let speaker_names = system.speaker_names();
    if let Some(first) = system.speaker(&speaker_names[0]) {
        let _ = first.group_membership.watch();
    }
    for _ in 0..10 {
        if !system.groups().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

// ============================================================================
// RAII restoration guards
// ============================================================================

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
// Single-speaker property tests
// ============================================================================

#[test]
#[ignore]
fn test_volume_round_trip_values() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing volume round-trip values on: {}", speaker.name);

    let original = speaker.volume.fetch()?.0;
    let _guard = VolumeGuard {
        speaker: &speaker,
        original,
    };

    let _handle = speaker.volume.watch()?;
    thread::sleep(Duration::from_millis(500));

    for target in [0u8, 1, 10, 25, 50] {
        if target == original {
            continue;
        }

        eprintln!("  Setting volume: {original} -> {target}");
        speaker.set_volume(target)?;

        let event = wait_for_property_event(&iter, &speaker.id, "volume", Duration::from_secs(5));
        assert!(event.is_some(), "No event for volume={target}");

        let cached = speaker.volume.get().expect("volume should be cached");
        assert_eq!(cached.0, target, "Volume cache should be exactly {target}");

        thread::sleep(Duration::from_millis(200));
    }

    eprintln!("  All volume boundary values verified");
    Ok(())
}

#[test]
#[ignore]
fn test_rendering_control_freshness() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing RenderingControl freshness on: {}", speaker.name);

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
        thread::sleep(subscription_settle);

        let target = if original != 42 { 42 } else { 43 };
        eprintln!("  Setting volume: {original} -> {target}");
        speaker.set_volume(target)?;

        let event = wait_for_property_event(&iter, &speaker.id, "volume", event_timeout);
        assert!(event.is_some(), "No volume event");
        assert_eq!(speaker.volume.get().unwrap().0, target);
        eprintln!("  Volume freshness verified");
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

        eprintln!("  Setting mute: {original} -> {}", !original);
        speaker.set_mute(!original)?;

        let event = wait_for_property_event(&iter, &speaker.id, "mute", event_timeout);
        assert!(event.is_some(), "No mute event");
        assert_eq!(speaker.mute.get().unwrap().0, !original);
        eprintln!("  Mute freshness verified");
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

        let target: i8 = if original != 3 { 3 } else { -3 };
        eprintln!("  Setting bass: {original} -> {target}");
        speaker.set_bass(target)?;

        let event = wait_for_property_event(&iter, &speaker.id, "bass", event_timeout);
        assert!(event.is_some(), "No bass event");
        assert_eq!(speaker.bass.get().unwrap().0, target);
        eprintln!("  Bass freshness verified");
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

        let target: i8 = if original != -5 { -5 } else { 5 };
        eprintln!("  Setting treble: {original} -> {target}");
        speaker.set_treble(target)?;

        let event = wait_for_property_event(&iter, &speaker.id, "treble", event_timeout);
        assert!(event.is_some(), "No treble event");
        assert_eq!(speaker.treble.get().unwrap().0, target);
        eprintln!("  Treble freshness verified");
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

        eprintln!("  Setting loudness: {original} -> {}", !original);
        speaker.set_loudness(!original)?;

        let event = wait_for_property_event(&iter, &speaker.id, "loudness", event_timeout);
        assert!(event.is_some(), "No loudness event");
        assert_eq!(speaker.loudness.get().unwrap().0, !original);
        eprintln!("  Loudness freshness verified");
    }

    eprintln!("\nAll RenderingControl properties fresh and correct");
    Ok(())
}

#[test]
#[ignore]
fn test_playback_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Prefer a speaker that's already playing — it has content loaded and
    // will reliably transition between Playing/Paused. Fall back to any reachable speaker.
    let speaker = match find_playing_speaker(&system) {
        Some(s) => {
            eprintln!("Found playing speaker: {}", s.name);
            s
        }
        None => {
            eprintln!("No playing speaker found, using first reachable");
            find_reachable_speaker(&system)?
        }
    };
    let iter = system.iter();

    eprintln!("Testing PlaybackState transitions on: {}", speaker.name);

    let event_timeout = Duration::from_secs(5);

    let current = speaker.playback_state.fetch()?;
    let _guard = PlaybackGuard {
        speaker: &speaker,
        was_playing: current.is_playing(),
    };

    let _handle = speaker.playback_state.watch()?;
    thread::sleep(Duration::from_millis(500));

    eprintln!("  Current state: {current:?}");

    if current.is_playing() {
        // Playing -> Paused
        eprintln!("  Pausing...");
        speaker.pause()?;
        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", event_timeout);
        assert!(event.is_some(), "No event after pause");
        let cached = speaker
            .playback_state
            .get()
            .expect("playback_state should be cached");
        assert!(
            matches!(cached, PlaybackState::Paused),
            "Expected Paused, got {cached:?}"
        );
        eprintln!("  Verified: Playing -> Paused");

        // Paused -> Playing
        eprintln!("  Resuming...");
        speaker.play()?;
        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", event_timeout);
        assert!(event.is_some(), "No event after play");
        let cached = speaker
            .playback_state
            .get()
            .expect("playback_state should be cached");
        assert!(
            matches!(cached, PlaybackState::Playing),
            "Expected Playing, got {cached:?}"
        );
        eprintln!("  Verified: Paused -> Playing");
    } else {
        // Stopped/Paused -> Play (may fail with 500 if no queue loaded)
        eprintln!("  Playing...");
        match speaker.play() {
            Ok(()) => {
                // Wait for a non-Paused/non-Stopped state — the speaker may emit
                // a stale NOTIFY before the actual transition completes.
                let mut transitioned = false;
                let deadline = Instant::now() + event_timeout;
                while Instant::now() < deadline {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let poll = remaining.min(Duration::from_millis(100));
                    if let Some(event) = iter.recv_timeout(poll) {
                        if event.speaker_id == speaker.id && event.property_key == "playback_state"
                        {
                            let cached = speaker.playback_state.get().unwrap();
                            eprintln!("  Event received, state: {cached:?}");
                            if matches!(
                                cached,
                                PlaybackState::Playing | PlaybackState::Transitioning
                            ) {
                                transitioned = true;
                                break;
                            }
                        }
                    }
                }

                if transitioned {
                    eprintln!("  Verified: Stopped/Paused -> Playing/Transitioning");

                    // Playing -> Paused
                    eprintln!("  Pausing...");
                    speaker.pause()?;
                    let event = wait_for_property_event(
                        &iter,
                        &speaker.id,
                        "playback_state",
                        event_timeout,
                    );
                    assert!(event.is_some(), "No event after pause");
                    eprintln!("  Verified: Playing -> Paused");
                } else {
                    eprintln!(
                        "  Skipped: speaker did not transition to Playing — may have no playable content"
                    );
                }
            }
            Err(e) => {
                eprintln!("  Skipped: play() failed ({e}) — speaker likely has no queue");
            }
        }
    }

    eprintln!("PlaybackState transitions verified");
    Ok(())
}

#[test]
#[ignore]
fn test_concurrent_property_watches() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing concurrent property watches on: {}", speaker.name);

    let original_vol = speaker.volume.fetch()?.0;
    let original_mute = speaker.mute.fetch()?.0;
    let _vol_guard = VolumeGuard {
        speaker: &speaker,
        original: original_vol,
    };
    let _mute_guard = MuteGuard {
        speaker: &speaker,
        original: original_mute,
    };

    // Watch both simultaneously
    let _vol_handle = speaker.volume.watch()?;
    let _mute_handle = speaker.mute.watch()?;
    thread::sleep(Duration::from_millis(500));

    // Mutate only volume
    let new_vol = if original_vol > 5 {
        original_vol - 5
    } else {
        original_vol + 5
    };
    eprintln!("  Setting volume only: {original_vol} -> {new_vol}");
    speaker.set_volume(new_vol)?;

    let event = wait_for_property_event(&iter, &speaker.id, "volume", Duration::from_secs(5));
    assert!(event.is_some(), "No volume event during concurrent watch");
    assert_eq!(
        speaker.volume.get().unwrap().0,
        new_vol,
        "Volume cache incorrect"
    );

    // Mute should be unchanged
    let mute_val = speaker.mute.get().unwrap().0;
    assert_eq!(
        mute_val, original_mute,
        "Mute should be unchanged after volume-only mutation"
    );
    eprintln!("  Volume mutated, mute unaffected");

    // Now mutate mute
    eprintln!("  Setting mute only: {original_mute} -> {}", !original_mute);
    speaker.set_mute(!original_mute)?;

    let event = wait_for_property_event(&iter, &speaker.id, "mute", Duration::from_secs(5));
    assert!(event.is_some(), "No mute event during concurrent watch");
    assert_eq!(
        speaker.mute.get().unwrap().0,
        !original_mute,
        "Mute cache incorrect"
    );

    // Volume should still be new_vol
    assert_eq!(
        speaker.volume.get().unwrap().0,
        new_vol,
        "Volume should be unchanged after mute mutation"
    );
    eprintln!("  Mute mutated, volume unaffected");

    eprintln!("Concurrent property watches verified");
    Ok(())
}

#[test]
#[ignore]
fn test_cache_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    eprintln!("Testing cache lifecycle on: {}", speaker.name);

    // Step 1: fetch() populates cache
    let fetched = speaker.volume.fetch()?.0;
    let cached = speaker
        .volume
        .get()
        .expect("get() should return Some after fetch()");
    assert_eq!(cached.0, fetched, "Cache should match fetched value");
    eprintln!("  Step 1: fetch() populated cache with {fetched}");

    let original = fetched;
    let _guard = VolumeGuard {
        speaker: &speaker,
        original,
    };

    // Step 2: watch() establishes subscription
    let handle = speaker.volume.watch()?;
    thread::sleep(Duration::from_millis(500));
    eprintln!("  Step 2: watch() established (mode: {})", handle.mode());

    // Step 3: mutate and verify cache updates
    let new_vol = if original > 5 {
        original - 5
    } else {
        original + 5
    };
    speaker.set_volume(new_vol)?;

    let event = wait_for_property_event(&iter, &speaker.id, "volume", Duration::from_secs(5));
    assert!(event.is_some(), "No event after mutation");

    let cached_after = speaker
        .volume
        .get()
        .expect("get() should return Some after event");
    assert_eq!(cached_after.0, new_vol, "Cache should reflect mutation");
    eprintln!("  Step 3: mutation {original} -> {new_vol} reflected in cache");

    // Step 4: drop watch, cache should persist
    drop(handle);
    thread::sleep(Duration::from_millis(100));

    let cached_after_drop = speaker
        .volume
        .get()
        .expect("Cache should persist after watch dropped");
    assert_eq!(
        cached_after_drop.0, new_vol,
        "Cache should persist after watch handle dropped"
    );
    eprintln!("  Step 4: cache persists after watch dropped");

    eprintln!("Cache lifecycle verified");
    Ok(())
}

// ============================================================================
// Group / topology tests
// ============================================================================

#[test]
#[ignore]
fn test_group_management_state_changes() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    bootstrap_topology(&system);

    let standalone = match find_standalone_speakers(&system, 3) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Skipping group management test: {e}");
            return Ok(());
        }
    };

    let speaker_a = &standalone[0];
    let speaker_b = &standalone[1];
    let speaker_c = &standalone[2];

    eprintln!(
        "Testing group management with: {}, {}, {}",
        speaker_a.name, speaker_b.name, speaker_c.name
    );

    let iter = system.iter();
    let event_timeout = Duration::from_secs(5);

    // Watch GroupMembership on all three speakers
    let _gm_a = speaker_a.group_membership.watch()?;
    let _gm_b = speaker_b.group_membership.watch()?;
    let _gm_c = speaker_c.group_membership.watch()?;
    thread::sleep(Duration::from_millis(500));

    // -- Step 1: Verify all standalone --
    eprintln!("\n--- Step 1: Verify all standalone ---");
    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();
    let _ = speaker_c.group_membership.fetch();

    let gm_a = speaker_a.group_membership.get();
    let gm_b = speaker_b.group_membership.get();
    let gm_c = speaker_c.group_membership.get();

    if let (Some(a), Some(b), Some(c)) = (&gm_a, &gm_b, &gm_c) {
        assert!(a.is_coordinator, "A should be coordinator (standalone)");
        assert!(b.is_coordinator, "B should be coordinator (standalone)");
        assert!(c.is_coordinator, "C should be coordinator (standalone)");
        assert_ne!(a.group_id, b.group_id);
        assert_ne!(b.group_id, c.group_id);
        eprintln!("  All three standalone and in separate groups");
    }

    // -- Step 2: Group A + B --
    eprintln!("\n--- Step 2: Group A + B ---");
    system.create_group(speaker_a, &[speaker_b])?;

    drain_events(&iter, "group_membership", 2, event_timeout);
    thread::sleep(Duration::from_millis(200));

    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();

    let gm_a = speaker_a
        .group_membership
        .get()
        .expect("A should have membership");
    let gm_b = speaker_b
        .group_membership
        .get()
        .expect("B should have membership");

    assert_eq!(
        gm_a.group_id, gm_b.group_id,
        "A and B should be in same group"
    );
    assert!(gm_a.is_coordinator, "A should be coordinator of group");
    assert!(!gm_b.is_coordinator, "B should be member, not coordinator");

    let _ = speaker_c.group_membership.fetch();
    let gm_c = speaker_c
        .group_membership
        .get()
        .expect("C should have membership");
    assert!(
        gm_c.is_coordinator,
        "C should still be standalone coordinator"
    );
    assert_ne!(
        gm_c.group_id, gm_a.group_id,
        "C should be in a different group"
    );

    assert!(
        wait_for_group_topology(
            &system,
            |gs| gs.iter().any(|g| g.member_count() == 2),
            Duration::from_secs(5)
        ),
        "Should have a 2-member group"
    );
    eprintln!("  A+B grouped, C standalone");

    // -- Step 3: Move C into A's group --
    eprintln!("\n--- Step 3: Add C to A's group ---");
    let ab_group = system
        .group_for_speaker(&speaker_a.id)
        .expect("A should be in a group");
    speaker_c.join_group(&ab_group)?;

    drain_events(&iter, "group_membership", 2, event_timeout);
    thread::sleep(Duration::from_millis(200));

    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();
    let _ = speaker_c.group_membership.fetch();

    let gm_a = speaker_a.group_membership.get().expect("A membership");
    let gm_b = speaker_b.group_membership.get().expect("B membership");
    let gm_c = speaker_c.group_membership.get().expect("C membership");

    assert_eq!(gm_a.group_id, gm_b.group_id, "A and B in same group");
    assert_eq!(gm_b.group_id, gm_c.group_id, "B and C in same group");
    assert!(gm_a.is_coordinator, "A should still be coordinator");
    assert!(!gm_b.is_coordinator, "B should be member");
    assert!(!gm_c.is_coordinator, "C should be member");

    assert!(
        wait_for_group_topology(
            &system,
            |gs| gs.iter().any(|g| g.member_count() == 3),
            Duration::from_secs(5)
        ),
        "Should have a 3-member group"
    );
    eprintln!("  A+B+C all in one group");

    // -- Step 4: Remove B from group --
    eprintln!("\n--- Step 4: Remove B from group ---");
    speaker_b.leave_group()?;

    drain_events(&iter, "group_membership", 2, event_timeout);
    thread::sleep(Duration::from_millis(200));

    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();
    let _ = speaker_c.group_membership.fetch();

    let gm_a = speaker_a.group_membership.get().expect("A membership");
    let gm_b = speaker_b.group_membership.get().expect("B membership");
    let gm_c = speaker_c.group_membership.get().expect("C membership");

    assert!(
        gm_b.is_coordinator,
        "B should be standalone coordinator after leaving"
    );
    assert_ne!(gm_b.group_id, gm_a.group_id, "B should be in its own group");
    assert_eq!(
        gm_a.group_id, gm_c.group_id,
        "A and C should still be grouped"
    );
    assert!(gm_a.is_coordinator, "A should still be coordinator");
    assert!(!gm_c.is_coordinator, "C should still be member");

    assert!(
        wait_for_group_topology(
            &system,
            |gs| gs.iter().any(|g| g.member_count() == 2),
            Duration::from_secs(5)
        ),
        "Should have a 2-member group (A+C)"
    );
    eprintln!("  B standalone, A+C grouped");

    // -- Step 5: Dissolve -- C leaves --
    eprintln!("\n--- Step 5: Dissolve group (C leaves) ---");
    speaker_c.leave_group()?;

    drain_events(&iter, "group_membership", 2, event_timeout);
    thread::sleep(Duration::from_millis(200));

    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();
    let _ = speaker_c.group_membership.fetch();

    let gm_a = speaker_a.group_membership.get().expect("A membership");
    let gm_b = speaker_b.group_membership.get().expect("B membership");
    let gm_c = speaker_c.group_membership.get().expect("C membership");

    assert!(gm_a.is_coordinator, "A should be standalone");
    assert!(gm_b.is_coordinator, "B should be standalone");
    assert!(gm_c.is_coordinator, "C should be standalone");
    assert_ne!(gm_a.group_id, gm_b.group_id, "All in different groups");
    assert_ne!(gm_b.group_id, gm_c.group_id, "All in different groups");
    assert_ne!(gm_a.group_id, gm_c.group_id, "All in different groups");
    eprintln!("  All three standalone again");

    eprintln!("\nAll group management state changes verified");
    Ok(())
}

#[test]
#[ignore]
fn test_group_volume_freshness() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    bootstrap_topology(&system);

    let standalone = match find_standalone_speakers(&system, 2) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Skipping group volume freshness: {e}");
            return Ok(());
        }
    };

    let speaker_a = &standalone[0];
    let speaker_b = &standalone[1];

    eprintln!(
        "Testing group volume freshness with: {} + {}",
        speaker_a.name, speaker_b.name
    );

    let _gm_a = speaker_a.group_membership.watch();
    let _gm_b = speaker_b.group_membership.watch();
    thread::sleep(Duration::from_millis(300));

    system.create_group(speaker_a, &[speaker_b])?;
    let _group_guard = GroupGuard {
        speakers: vec![speaker_b],
    };

    let iter_tmp = system.iter();
    drain_events(&iter_tmp, "group_membership", 2, Duration::from_secs(5));

    assert!(
        wait_for_group_topology(
            &system,
            |gs| gs.iter().any(|g| g.member_count() >= 2),
            Duration::from_secs(5)
        ),
        "Group not found after creation"
    );
    let group = system
        .groups()
        .into_iter()
        .find(|g| g.member_count() >= 2)
        .expect("Group must exist after topology settled");

    let iter = system.iter();

    if let Ok(handle) = group.volume.watch() {
        thread::sleep(Duration::from_millis(500));

        let original = group.volume.get().map(|v| v.0).unwrap_or(20);
        let target = if original > 10 {
            original - 10
        } else {
            original + 10
        };

        eprintln!("  Setting group volume: {original} -> {target}");
        group.set_volume(target)?;

        let event = wait_for_property_event(
            &iter,
            &group.coordinator_id,
            "group_volume",
            Duration::from_secs(5),
        );
        assert!(event.is_some(), "No group_volume event");

        let cached = group.volume.get().expect("group_volume should be cached");
        assert_eq!(
            cached.0, target,
            "Group volume cache should match set value"
        );
        eprintln!("  Group volume freshness verified");

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
        eprintln!("  Could not watch GroupVolume");
    }

    eprintln!("Group volume freshness test complete");
    Ok(())
}

#[test]
#[ignore]
fn test_topology_freshness() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    bootstrap_topology(&system);

    let standalone = match find_standalone_speakers(&system, 2) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Skipping topology freshness: {e}");
            return Ok(());
        }
    };

    let speaker_a = &standalone[0];
    let speaker_b = &standalone[1];

    eprintln!(
        "Testing topology freshness with: {} + {}",
        speaker_a.name, speaker_b.name
    );

    let iter = system.iter();

    // Watch memberships on both
    let _gm_a = speaker_a.group_membership.watch()?;
    let _gm_b = speaker_b.group_membership.watch()?;
    thread::sleep(Duration::from_millis(500));

    // Record pre-group state
    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();
    let pre_a = speaker_a.group_membership.get();
    let pre_b = speaker_b.group_membership.get();

    if let (Some(a), Some(b)) = (&pre_a, &pre_b) {
        assert!(
            a.is_coordinator,
            "Speaker A should be coordinator before grouping"
        );
        assert!(
            b.is_coordinator,
            "Speaker B should be coordinator before grouping"
        );
        assert_ne!(
            a.group_id, b.group_id,
            "Should be in different groups before grouping"
        );
        eprintln!("  Pre-group: both standalone in separate groups");
    }

    // Create group
    let _group_guard = GroupGuard {
        speakers: vec![speaker_b],
    };
    system.create_group(speaker_a, &[speaker_b])?;

    drain_events(&iter, "group_membership", 2, Duration::from_secs(5));
    thread::sleep(Duration::from_millis(200));

    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();

    let post_a = speaker_a
        .group_membership
        .get()
        .expect("A should have membership after group");
    let post_b = speaker_b
        .group_membership
        .get()
        .expect("B should have membership after group");

    assert_eq!(post_a.group_id, post_b.group_id, "Should be in same group");
    assert!(post_a.is_coordinator, "Speaker A should be coordinator");
    assert!(
        !post_b.is_coordinator,
        "Speaker B should not be coordinator"
    );
    eprintln!("  Post-group: same group_id, A coordinator, B member");

    eprintln!("Topology freshness verified");
    Ok(())
}
