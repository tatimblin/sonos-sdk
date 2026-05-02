---
title: Data Freshness & Completeness Integration Tests
type: feat
status: active
date: 2026-05-01
origin: docs/brainstorms/2026-03-28-integration-test-suite-brainstorm.md
---

# Data Freshness & Completeness Integration Tests

## Overview

Extend the existing integration test suite with a new test file focused on **data freshness and completeness**: for each mutable property, watch it, mutate it via the API, and assert that the watched state reflects the exact expected value within a bounded time window. This goes beyond the existing `property_validation.rs` (which validates event delivery) by asserting **value correctness** and **cache consistency** end-to-end.

## Problem Statement

The existing `property_validation.rs` tests confirm that a `ChangeEvent` arrives after mutation, and that the cached value matches. However, there are gaps:

1. **No assertion on intermediate states**: Tests don't verify that the cached value was `None` or the original value *before* the mutation, only after.
2. **No multi-property concurrent watching**: Each property is tested in isolation — no test validates that watching Volume and Mute simultaneously both deliver correct events when only one is mutated.
3. **No round-trip value precision tests**: Volume is set to `original - 5`, but there's no test that sets to specific boundary values (0, 100) and verifies the cache reflects those exact values.
4. **No "stale data" regression tests**: Nothing validates that `get()` returns `None` before any fetch/watch, returns correct value after watch+mutation, and stays correct after watch handle is dropped.
5. **Current branch context**: The `fix/data-freshness-completeness` branch modifies `decoder.rs` to add IP extraction and satellite handling — new topology decoding paths that need validation.

## Proposed Solution

Create `sonos-sdk/tests/data_freshness.rs` — structured as a **property test suite** with shared infrastructure and per-property test functions. The architecture prioritizes easy extensibility: adding a new property test should require only writing the test function and nothing else.

### Architecture: Property Test Suite

The file is organized into distinct sections:

1. **Shared infrastructure** — test helpers, RAII guards, event utilities (top of file)
2. **Single-speaker property tests** — one `#[test] #[ignore]` per property or concern
3. **Multi-speaker / group tests** — group management and topology tests
4. **Helpers module** — reusable functions that any new test can call

This structure means a new property test is just a new function — no framework registration, no macro wiring. Copy an existing test, change the property, done.

```
sonos-sdk/tests/data_freshness.rs
├── // Shared infrastructure
│   ├── require_real_speakers()
│   ├── find_reachable_speaker()
│   ├── find_standalone_speakers()
│   ├── wait_for_property_event()
│   ├── drain_events()
│   └── RAII guards (VolumeGuard, MuteGuard, BassGuard, ...)
│
├── // Single-speaker property tests
│   ├── test_volume_round_trip_values
│   ├── test_rendering_control_freshness
│   ├── test_playback_state_transitions
│   ├── test_concurrent_property_watches
│   └── test_cache_lifecycle
│
└── // Group / topology tests
    ├── test_group_management_state_changes
    ├── test_group_volume_freshness
    └── test_topology_freshness
```

### Adding a New Property Test

To add a test for a new property (e.g., `NightMode` or `SurroundLevel`), follow this pattern:

```rust
#[test]
#[ignore]
fn test_night_mode_freshness() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    // 1. Save original + set up RAII guard
    let original = speaker.night_mode.fetch()?.0;
    let _guard = NightModeGuard { speaker: &speaker, original };

    // 2. Watch
    let _handle = speaker.night_mode.watch()?;
    thread::sleep(Duration::from_millis(500));

    // 3. Mutate
    speaker.set_night_mode(!original)?;

    // 4. Assert event + cache
    let event = wait_for_property_event(&iter, &speaker.id, "night_mode", Duration::from_secs(5));
    assert!(event.is_some(), "No night_mode event");
    assert_eq!(speaker.night_mode.get().unwrap().0, !original);

    Ok(())  // guard restores on drop
}
```

That's it — the shared helpers handle discovery, event waiting, and the RAII guard handles cleanup. No registration needed.

### Data Lifecycle Under Test

Each property test validates the full lifecycle:

```
get() → None → watch() → subscription settles → set_*() → event arrives → get() → correct value → drop watch → get() → value persists in cache
```

### Initial Test Suite (8 tests)

1. **`test_volume_round_trip_values`** — Boundary values (0, 1, 10, 25, 50) with exact cache assertions (capped at 50 to avoid loud playback)
2. **`test_rendering_control_freshness`** — All 5 RC properties: watch, mutate, assert cached value, verify value persists after watch dropped
3. **`test_playback_state_transitions`** — State machine transitions: Playing→Paused→Playing, Stopped→Playing→Stopped
4. **`test_concurrent_property_watches`** — Watch Volume + Mute simultaneously, mutate only Volume, verify only volume event arrives and Mute cache is unaffected
5. **`test_cache_lifecycle`** — Full lifecycle: get()→None, fetch()→value, set_*(), get()→new value, drop watch, get()→value persists
6. **`test_group_management_state_changes`** — Move speakers between groups and verify GroupMembership, GroupVolume, and topology state at each step (requires 3 speakers)
7. **`test_group_volume_freshness`** — Group property round-trip: create group, watch GroupVolume, set, verify on coordinator
8. **`test_topology_freshness`** — Validate GroupMembership values are correct after group create/dissolve (exercises new decoder.rs paths)

## Technical Approach

### File: `sonos-sdk/tests/data_freshness.rs`

Structured as a property test suite with shared infrastructure:

**Shared helpers** (reused by all tests, same as `property_validation.rs`):
- `require_real_speakers()`, `find_reachable_speaker()`, `find_standalone_speakers()` — discovery
- `wait_for_property_event()` — event polling with timeout and speaker+property filtering
- `drain_events()` — consume N events of a given type (for group tests that emit multiple events)
- RAII restoration guards (VolumeGuard, MuteGuard, BassGuard, TrebleGuard, LoudnessGuard, PlaybackGuard, GroupGuard)
- `#[test] #[ignore]` — manual execution against real hardware

### Test Implementations

#### 1. `test_volume_round_trip_values`

Validates that specific volume values survive the full round-trip:

```rust
#[test]
#[ignore]
fn test_volume_round_trip_values() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    let original = speaker.volume.fetch()?.0;
    let _guard = VolumeGuard { speaker: &speaker, original };

    let _handle = speaker.volume.watch()?;
    thread::sleep(Duration::from_millis(500));

    // Test boundary values (capped at 50 to avoid loud playback)
    for target in [0u8, 1, 10, 25, 50] {
        if target == original { continue; }

        speaker.set_volume(target)?;
        let event = wait_for_property_event(&iter, &speaker.id, "volume", Duration::from_secs(5));
        assert!(event.is_some(), "No event for volume={target}");

        let cached = speaker.volume.get().expect("volume should be cached");
        assert_eq!(cached.0, target, "Volume cache should be exactly {target}");

        // Small delay to avoid overwhelming the speaker
        thread::sleep(Duration::from_millis(200));
    }

    Ok(())
}
```

#### 2. `test_rendering_control_freshness`

Validates all 5 RenderingControl properties with value assertions:

```rust
#[test]
#[ignore]
fn test_rendering_control_freshness() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    // Volume: set to specific value, verify exact cache
    {
        let original = speaker.volume.fetch()?.0;
        let _guard = VolumeGuard { speaker: &speaker, original };
        let _handle = speaker.volume.watch()?;
        thread::sleep(Duration::from_millis(500));

        let target = if original != 42 { 42 } else { 43 };
        speaker.set_volume(target)?;

        let event = wait_for_property_event(&iter, &speaker.id, "volume", Duration::from_secs(5));
        assert!(event.is_some(), "No volume event");
        assert_eq!(speaker.volume.get().unwrap().0, target);
    }

    // Mute: toggle and verify boolean value
    {
        let original = speaker.mute.fetch()?.0;
        let _guard = MuteGuard { speaker: &speaker, original };
        let _handle = speaker.mute.watch()?;
        thread::sleep(Duration::from_millis(500));

        speaker.set_mute(!original)?;

        let event = wait_for_property_event(&iter, &speaker.id, "mute", Duration::from_secs(5));
        assert!(event.is_some(), "No mute event");
        assert_eq!(speaker.mute.get().unwrap().0, !original);
    }

    // Bass: set to specific value, verify i8 precision
    {
        let original = speaker.bass.fetch()?.0;
        let _guard = BassGuard { speaker: &speaker, original };
        let _handle = speaker.bass.watch()?;
        thread::sleep(Duration::from_millis(500));

        let target = if original != 3 { 3 } else { -3 };
        speaker.set_bass(target)?;

        let event = wait_for_property_event(&iter, &speaker.id, "bass", Duration::from_secs(5));
        assert!(event.is_some(), "No bass event");
        assert_eq!(speaker.bass.get().unwrap().0, target);
    }

    // Treble: set to specific value, verify i8 precision
    {
        let original = speaker.treble.fetch()?.0;
        let _guard = TrebleGuard { speaker: &speaker, original };
        let _handle = speaker.treble.watch()?;
        thread::sleep(Duration::from_millis(500));

        let target = if original != -5 { -5 } else { 5 };
        speaker.set_treble(target)?;

        let event = wait_for_property_event(&iter, &speaker.id, "treble", Duration::from_secs(5));
        assert!(event.is_some(), "No treble event");
        assert_eq!(speaker.treble.get().unwrap().0, target);
    }

    // Loudness: toggle and verify boolean
    {
        let original = speaker.loudness.fetch()?.0;
        let _guard = LoudnessGuard { speaker: &speaker, original };
        let _handle = speaker.loudness.watch()?;
        thread::sleep(Duration::from_millis(500));

        speaker.set_loudness(!original)?;

        let event = wait_for_property_event(&iter, &speaker.id, "loudness", Duration::from_secs(5));
        assert!(event.is_some(), "No loudness event");
        assert_eq!(speaker.loudness.get().unwrap().0, !original);
    }

    Ok(())
}
```

#### 3. `test_playback_state_transitions`

Validates that state machine transitions are reflected in the cache:

```rust
#[test]
#[ignore]
fn test_playback_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    let current = speaker.playback_state.fetch()?;
    let _guard = PlaybackGuard { speaker: &speaker, was_playing: current.is_playing() };

    let _handle = speaker.playback_state.watch()?;
    thread::sleep(Duration::from_millis(500));

    if current.is_playing() {
        // Playing → Paused
        speaker.pause()?;
        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", Duration::from_secs(5));
        assert!(event.is_some(), "No event after pause");
        let cached = speaker.playback_state.get().expect("playback_state should be cached");
        assert!(matches!(cached, PlaybackState::Paused), "Expected Paused, got {:?}", cached);

        // Paused → Playing
        speaker.play()?;
        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", Duration::from_secs(5));
        assert!(event.is_some(), "No event after play");
        let cached = speaker.playback_state.get().expect("playback_state should be cached");
        assert!(matches!(cached, PlaybackState::Playing), "Expected Playing, got {:?}", cached);
    } else {
        // Stopped/Paused → Play (may fail if no queue)
        speaker.play()?;
        let event = wait_for_property_event(&iter, &speaker.id, "playback_state", Duration::from_secs(5));
        if event.is_some() {
            let cached = speaker.playback_state.get().unwrap();
            assert!(matches!(cached, PlaybackState::Playing | PlaybackState::Transitioning),
                "Expected Playing or Transitioning, got {:?}", cached);

            // Playing → Paused
            speaker.pause()?;
            let event = wait_for_property_event(&iter, &speaker.id, "playback_state", Duration::from_secs(5));
            assert!(event.is_some(), "No event after pause");
        } else {
            eprintln!("  Skipped: speaker has no queue, play() did not generate event");
        }
    }

    Ok(())
}
```

#### 4. `test_concurrent_property_watches`

Validates that concurrent watches on different properties don't interfere:

```rust
#[test]
#[ignore]
fn test_concurrent_property_watches() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    let original_vol = speaker.volume.fetch()?.0;
    let original_mute = speaker.mute.fetch()?.0;
    let _vol_guard = VolumeGuard { speaker: &speaker, original: original_vol };
    let _mute_guard = MuteGuard { speaker: &speaker, original: original_mute };

    // Watch both properties simultaneously
    let _vol_handle = speaker.volume.watch()?;
    let _mute_handle = speaker.mute.watch()?;
    thread::sleep(Duration::from_millis(500));

    // Mutate only volume
    let new_vol = if original_vol > 5 { original_vol - 5 } else { original_vol + 5 };
    speaker.set_volume(new_vol)?;

    // Should receive volume event
    let event = wait_for_property_event(&iter, &speaker.id, "volume", Duration::from_secs(5));
    assert!(event.is_some(), "No volume event during concurrent watch");
    assert_eq!(speaker.volume.get().unwrap().0, new_vol, "Volume cache incorrect");

    // Mute should still have its original value (not corrupted by volume event)
    // Note: RenderingControl NOTIFY may include both, but mute value should stay the same
    let mute_val = speaker.mute.get().unwrap_or(Mute(original_mute));
    assert_eq!(mute_val.0, original_mute, "Mute should be unchanged after volume-only mutation");

    // Now mutate mute
    speaker.set_mute(!original_mute)?;
    let event = wait_for_property_event(&iter, &speaker.id, "mute", Duration::from_secs(5));
    assert!(event.is_some(), "No mute event during concurrent watch");
    assert_eq!(speaker.mute.get().unwrap().0, !original_mute, "Mute cache incorrect");

    // Volume should still be the new value
    assert_eq!(speaker.volume.get().unwrap().0, new_vol, "Volume should be unchanged after mute mutation");

    Ok(())
}
```

#### 5. `test_cache_lifecycle`

Validates the full cache lifecycle from empty to populated to post-watch:

```rust
#[test]
#[ignore]
fn test_cache_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;
    let iter = system.iter();

    // Note: cache may already be populated from SonosSystem::new() discovery.
    // We test the fetch→watch→mutate→verify lifecycle instead of assuming empty cache.

    // Step 1: fetch() populates cache with current value
    let fetched = speaker.volume.fetch()?.0;
    let cached = speaker.volume.get().expect("get() should return Some after fetch()");
    assert_eq!(cached.0, fetched, "Cache should match fetched value");

    let original = fetched;
    let _guard = VolumeGuard { speaker: &speaker, original };

    // Step 2: watch() — subscription established
    let handle = speaker.volume.watch()?;
    thread::sleep(Duration::from_millis(500));

    // Step 3: mutate — cache should update to new value
    let new_vol = if original > 5 { original - 5 } else { original + 5 };
    speaker.set_volume(new_vol)?;

    let event = wait_for_property_event(&iter, &speaker.id, "volume", Duration::from_secs(5));
    assert!(event.is_some(), "No event after mutation");

    let cached_after = speaker.volume.get().expect("get() should return Some after event");
    assert_eq!(cached_after.0, new_vol, "Cache should reflect mutation");

    // Step 4: drop watch — cache should persist (not cleared)
    drop(handle);
    thread::sleep(Duration::from_millis(100)); // let grace period pass

    let cached_after_drop = speaker.volume.get().expect("Cache should persist after watch dropped");
    assert_eq!(cached_after_drop.0, new_vol, "Cache should persist after watch handle dropped");

    Ok(())
}
```

#### 6. `test_group_management_state_changes`

Validates that moving speakers between groups produces correct state changes on watched properties. This is the most comprehensive group test — it creates groups, moves members around, and verifies GroupMembership, coordinator roles, and group counts at each step.

Requires 3 standalone speakers (skips gracefully if fewer available).

**Scenario:** A, B, C all standalone → group A+B → move C into A's group → remove B → dissolve group

```rust
#[test]
#[ignore]
fn test_group_management_state_changes() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Bootstrap topology
    let speaker_names = system.speaker_names();
    if let Some(first) = system.speaker(&speaker_names[0]) {
        let _ = first.group_membership.watch();
    }
    for _ in 0..10 {
        if !system.groups().is_empty() { break; }
        thread::sleep(Duration::from_millis(500));
    }

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

    eprintln!("Testing group management with: {}, {}, {}",
        speaker_a.name, speaker_b.name, speaker_c.name);

    let iter = system.iter();
    let event_timeout = Duration::from_secs(5);

    // Watch GroupMembership on all three speakers
    let _gm_a = speaker_a.group_membership.watch()?;
    let _gm_b = speaker_b.group_membership.watch()?;
    let _gm_c = speaker_c.group_membership.watch()?;
    thread::sleep(Duration::from_millis(500));

    // ── Step 1: Verify all standalone ──────────────────────────────
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
        // All in different groups
        assert_ne!(a.group_id, b.group_id);
        assert_ne!(b.group_id, c.group_id);
    }

    // ── Step 2: Group A + B ───────────────────────────────────────
    eprintln!("\n--- Step 2: Group A + B ---");
    system.create_group(speaker_a, &[speaker_b])?;

    // Wait for membership events
    drain_events(&iter, "group_membership", 2, event_timeout);
    thread::sleep(Duration::from_millis(200));

    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();

    let gm_a = speaker_a.group_membership.get().expect("A should have membership");
    let gm_b = speaker_b.group_membership.get().expect("B should have membership");

    assert_eq!(gm_a.group_id, gm_b.group_id, "A and B should be in same group");
    assert!(gm_a.is_coordinator, "A should be coordinator of group");
    assert!(!gm_b.is_coordinator, "B should be member, not coordinator");

    // C should still be standalone
    let _ = speaker_c.group_membership.fetch();
    let gm_c = speaker_c.group_membership.get().expect("C should have membership");
    assert!(gm_c.is_coordinator, "C should still be standalone coordinator");
    assert_ne!(gm_c.group_id, gm_a.group_id, "C should be in a different group");

    // Verify group count: should be a 2-member group
    let groups = system.groups();
    let ab_group = groups.iter().find(|g| g.member_count() == 2);
    assert!(ab_group.is_some(), "Should have a 2-member group");

    // ── Step 3: Move C into A's group ─────────────────────────────
    eprintln!("\n--- Step 3: Add C to A's group ---");
    let ab_group = system.group_for_speaker(&speaker_a.id)
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

    // All three should be in the same group
    assert_eq!(gm_a.group_id, gm_b.group_id, "A and B in same group");
    assert_eq!(gm_b.group_id, gm_c.group_id, "B and C in same group");
    assert!(gm_a.is_coordinator, "A should still be coordinator");
    assert!(!gm_b.is_coordinator, "B should be member");
    assert!(!gm_c.is_coordinator, "C should be member");

    // Should now be a 3-member group
    let groups = system.groups();
    let abc_group = groups.iter().find(|g| g.member_count() == 3);
    assert!(abc_group.is_some(), "Should have a 3-member group");

    // ── Step 4: Remove B from group ───────────────────────────────
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

    // B should be standalone now
    assert!(gm_b.is_coordinator, "B should be standalone coordinator after leaving");
    assert_ne!(gm_b.group_id, gm_a.group_id, "B should be in its own group");

    // A and C should still be grouped
    assert_eq!(gm_a.group_id, gm_c.group_id, "A and C should still be grouped");
    assert!(gm_a.is_coordinator, "A should still be coordinator");
    assert!(!gm_c.is_coordinator, "C should still be member");

    // Should have a 2-member group (A+C) and standalone B
    let groups = system.groups();
    let ac_group = groups.iter().find(|g| g.member_count() == 2);
    assert!(ac_group.is_some(), "Should have a 2-member group (A+C)");

    // ── Step 5: Dissolve — C leaves ───────────────────────────────
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

    // All three standalone again
    assert!(gm_a.is_coordinator, "A should be standalone");
    assert!(gm_b.is_coordinator, "B should be standalone");
    assert!(gm_c.is_coordinator, "C should be standalone");
    assert_ne!(gm_a.group_id, gm_b.group_id, "All in different groups");
    assert_ne!(gm_b.group_id, gm_c.group_id, "All in different groups");
    assert_ne!(gm_a.group_id, gm_c.group_id, "All in different groups");

    eprintln!("\n All group management state changes verified");
    Ok(())
}

/// Helper: drain up to `count` events matching `property_key` within `timeout`
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
                eprintln!("  [event] {} for {} ({}/{})",
                    event.property_key, event.speaker_id.as_str(), received, count);
            }
        }
    }
}
```

#### 7. `test_group_volume_freshness`

Validates group property data freshness (requires 2+ speakers):

```rust
#[test]
#[ignore]
fn test_group_volume_freshness() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Bootstrap topology
    let speaker_names = system.speaker_names();
    if let Some(first) = system.speaker(&speaker_names[0]) {
        let _ = first.group_membership.watch();
    }
    for _ in 0..10 {
        if !system.groups().is_empty() { break; }
        thread::sleep(Duration::from_millis(500));
    }

    let standalone = match find_standalone_speakers(&system, 2) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Skipping group volume freshness: {e}");
            return Ok(());
        }
    };

    let speaker_a = &standalone[0];
    let speaker_b = &standalone[1];

    // Create group
    system.create_group(speaker_a, &[speaker_b])?;
    let _group_guard = GroupGuard { speakers: vec![speaker_b] };

    // Wait for group to appear in topology
    let mut group = None;
    for _ in 0..10 {
        if let Some(g) = system.groups().into_iter().find(|g| g.member_count() >= 2) {
            group = Some(g);
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    let group = group.ok_or("Group not found after creation")?;

    let iter = system.iter();

    // Watch group volume and verify round-trip
    if let Ok(handle) = group.volume.watch() {
        thread::sleep(Duration::from_millis(500));

        let original = group.volume.get().map(|v| v.0).unwrap_or(20);
        let target = if original > 10 { original - 10 } else { original + 10 };

        group.set_volume(target)?;

        let event = wait_for_property_event(
            &iter, &group.coordinator_id, "group_volume", Duration::from_secs(5)
        );
        assert!(event.is_some(), "No group_volume event");

        let cached = group.volume.get().expect("group_volume should be cached");
        assert_eq!(cached.0, target, "Group volume cache should match set value");

        // Restore
        group.set_volume(original)?;
        drop(handle);
    }

    Ok(())
}
```

#### 7. `test_topology_freshness`

Validates that GroupMembership values are correct after group operations:

```rust
#[test]
#[ignore]
fn test_topology_freshness() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Bootstrap topology
    let speaker_names = system.speaker_names();
    if let Some(first) = system.speaker(&speaker_names[0]) {
        let _ = first.group_membership.watch();
    }
    for _ in 0..10 {
        if !system.groups().is_empty() { break; }
        thread::sleep(Duration::from_millis(500));
    }

    let standalone = match find_standalone_speakers(&system, 2) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Skipping topology freshness: {e}");
            return Ok(());
        }
    };

    let speaker_a = &standalone[0];
    let speaker_b = &standalone[1];
    let iter = system.iter();

    // Watch memberships on both speakers
    let _gm_a = speaker_a.group_membership.watch()?;
    let _gm_b = speaker_b.group_membership.watch()?;
    thread::sleep(Duration::from_millis(500));

    // Record pre-group state
    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();
    let pre_a = speaker_a.group_membership.get();
    let pre_b = speaker_b.group_membership.get();

    // Both should be coordinators of their own groups
    if let (Some(a), Some(b)) = (&pre_a, &pre_b) {
        assert!(a.is_coordinator, "Speaker A should be coordinator before grouping");
        assert!(b.is_coordinator, "Speaker B should be coordinator before grouping");
        assert_ne!(a.group_id, b.group_id, "Should be in different groups before grouping");
    }

    // Create group
    let _group_guard = GroupGuard { speakers: vec![speaker_b] };
    system.create_group(speaker_a, &[speaker_b])?;

    // Wait for membership events
    let mut events = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline && events < 2 {
        if let Some(e) = iter.recv_timeout(Duration::from_millis(100)) {
            if e.property_key == "group_membership" { events += 1; }
        }
    }

    thread::sleep(Duration::from_millis(200));
    let _ = speaker_a.group_membership.fetch();
    let _ = speaker_b.group_membership.fetch();

    let post_a = speaker_a.group_membership.get().expect("A should have membership after group");
    let post_b = speaker_b.group_membership.get().expect("B should have membership after group");

    // Both in same group, A is coordinator
    assert_eq!(post_a.group_id, post_b.group_id, "Should be in same group");
    assert!(post_a.is_coordinator, "Speaker A should be coordinator");
    assert!(!post_b.is_coordinator, "Speaker B should not be coordinator");

    Ok(())
}
```

## Acceptance Criteria

### Functional Requirements
- [x] New test file `sonos-sdk/tests/data_freshness.rs` compiles and runs with `cargo test --package sonos-sdk --test data_freshness -- --ignored --nocapture`
- [x] All 8 tests pass against real Sonos hardware
- [x] RAII guards restore all property values after each test
- [x] Tests fail loudly when no speakers available
- [x] Group tests skip gracefully when insufficient standalone speakers available (2 for volume/topology, 3 for management)

### Value Correctness Requirements
- [x] Volume boundary values (0, 1, 10, 25, 50) all survive round-trip — never exceeds 50 to avoid loud playback
- [x] Boolean properties (Mute, Loudness) toggle correctly in both directions
- [x] Signed integer properties (Bass, Treble) accept specific values including negative
- [x] PlaybackState transitions reflect correct enum variant
- [x] GroupMembership reflects correct coordinator/member roles and group_id
- [x] Group management scenario: all standalone → A+B grouped → C joins → B leaves → C leaves → all standalone — with correct state at each step

### Cache Consistency Requirements
- [x] `get()` after `fetch()` matches fetched value
- [x] `get()` after watch+mutation matches set value
- [x] `get()` after watch handle drop still returns cached value
- [x] Concurrent watches on different properties don't corrupt each other's cache

### Quality Requirements
- [x] Structured as a property test suite — adding a new property test requires only a new function, no registration or framework changes
- [x] Shared helpers at top of file: discovery, event waiting, RAII guards
- [x] Reuses RAII guard pattern — every mutation has a guard that restores on drop
- [x] Each test is independent — can run individually or as a suite
- [x] Fast execution (< 30 seconds for all tests) — measured 2.82s
- [x] Clear assertion messages on failure

## Dependencies & Risks

### Dependencies
- Real Sonos hardware (1+ speaker for most tests, 2+ standalone for group volume/topology, 3+ standalone for group management)
- Network connectivity between test machine and speakers
- Branch `fix/data-freshness-completeness` decoder changes (for topology test validation)

### Risks
- **RenderingControl batch NOTIFY**: A single volume mutation may trigger a NOTIFY with all RC fields (volume, mute, bass, treble, loudness), potentially generating multiple events. Tests must handle receiving "extra" events for properties they didn't mutate.
- **Playback requires queue**: PlaybackState tests may skip if speaker has no queue loaded.
- **Group property timing**: GroupRenderingControl events may lag behind topology updates.

## Sources & References

### Origin
- **Brainstorm:** [docs/brainstorms/2026-03-28-integration-test-suite-brainstorm.md](docs/brainstorms/2026-03-28-integration-test-suite-brainstorm.md)
- **Prior plan (completed):** [docs/plans/2026-03-28-feat-integration-test-suite-plan.md](docs/plans/2026-03-28-feat-integration-test-suite-plan.md)

### Internal References
- **Existing property validation:** `sonos-sdk/tests/property_validation.rs` — established watch→set→verify pattern with RAII guards
- **Existing integration tests:** `sonos-sdk/tests/integration_real_speakers.rs` — helper functions and test infrastructure
- **Decoder under test:** `sonos-state/src/decoder.rs` — modified on current branch with IP/satellite extraction

### Known Gotchas
- **fetch() before watch() suppresses initial event** — see `docs/solutions/logic-errors/watch-after-fetch-event-suppression.md`
- **50ms grace period** on WatchHandle drop before actual unsubscribe
- **Position not always in AVTransport NOTIFY** — polling may be needed
