---
title: "feat: Harden watch() Property Reliability"
type: feat
status: active
date: 2026-03-29
origin: docs/brainstorms/2026-03-29-harden-watch-reliability-brainstorm.md
---

# feat: Harden watch() Property Reliability

## Overview

The `watch()` system doesn't reliably surface every property update. Position is notably flaky, and other properties miss events intermittently. This plan implements a diagnostic-first approach: build observability tooling to understand what's failing, codify expectations into automated tests, then fix root causes based on evidence.

Three phases (see brainstorm: `docs/brainstorms/2026-03-29-harden-watch-reliability-brainstorm.md`):
1. Property Observer Dashboard — live visibility into all 13 properties
2. Automated Property Validation Tests — assert every property propagates through `watch()`
3. Targeted Root Cause Fixes — based on findings from Phases 1-2

## Problem Statement / Motivation

When a user changes volume, playback state, or track on the Sonos app, the `watch()` pipeline should deliver that change to the Rust SDK consumer. Currently it doesn't do this reliably for all properties. The 6-layer event pipeline (callback-server → sonos-stream → sonos-event-manager → sonos-state → sonos-sdk → user) has multiple async/sync bridges where events can silently drop.

Without systematic observability, debugging is guesswork. The existing integration tests only validate Volume events end-to-end — the other 12 properties have no real-speaker coverage.

## Proposed Solution

### Phase 1: Property Observer Dashboard

A new example binary at `sonos-sdk/examples/property_observer.rs` that:
- Discovers all speakers via `SonosSystem::new()`
- Watches all 9 speaker-scoped properties on every reachable speaker
- Watches all 3 group-scoped properties on each group coordinator
- Monitors system-scoped Topology changes
- Displays a live table with: speaker name, property, current value, last updated timestamp, update count, watch mode (Events/Polling/CacheOnly)
- Shows time-since-last-update to identify stale properties
- Runs until Ctrl+C

**Architecture note:** The dashboard has three sections because properties have different scopes (see brainstorm). Speaker properties (9) use `speaker.{property}.watch()`. Group properties (3) use `group.{property}.watch()` on the coordinator and require topology to be populated first. Topology itself uses the lower-level `StateManager` API since there is no `system.topology.watch()` in the SDK.

**Dashboard sections:**

| Section | Properties | Source Handle |
|---------|-----------|---------------|
| Per-Speaker | Volume, Mute, Bass, Treble, Loudness, PlaybackState, Position, CurrentTrack, GroupMembership | `speaker.{prop}.watch()` |
| Per-Group | GroupVolume, GroupMute, GroupVolumeChangeable | `group.{prop}.watch()` on coordinator |
| System | Topology | `state_manager.watch_property_with_subscription::<Topology>()` |

**Initialization flow:**
1. `SonosSystem::new()` — discover speakers
2. For each speaker: call `fetch()` on all speaker properties, then `watch()` — this seeds the cache and starts subscriptions
3. Wait for topology to populate (poll `system.groups()` until non-empty, up to 5s)
4. For each group: `watch()` group properties on the coordinator
5. Enter event loop: consume `system.iter()`, update display on each event

**Display format (printed to terminal, one section per property group):**

```
=== Speaker Properties ===
Speaker         Property        Value                   Updated         Count   Mode
Kitchen         Volume          45                      2s ago          3       Events
Kitchen         Mute            false                   5s ago          1       Events
Kitchen         PlaybackState   Playing                 12s ago         2       Events
Kitchen         Position        1:23 / 3:45             1s ago          15      Polling
Kitchen         CurrentTrack    "Song Title" - Artist   30s ago         1       Events
Kitchen         Bass            0                       45s ago         1       Events
...

=== Group Properties ===
Group           Property              Value    Updated    Count   Mode
Kitchen+Bath    GroupVolume            42       3s ago     2       Events
Kitchen+Bath    GroupMute              false    3s ago     1       Events
Kitchen+Bath    GroupVolumeChangeable  true     3s ago     1       Events

=== Topology ===
Groups: 2 | Speakers: 4 | Last update: 5s ago | Count: 1
```

**Dependencies:** `chrono` (timestamps), `ctrlc` (Ctrl+C handling) — both already in dev-dependencies.

### Phase 2: Automated Property Validation Tests

New integration test file at `sonos-sdk/tests/property_validation.rs` with `#[test] #[ignore]` tests requiring real speakers. Run with:

```bash
cargo test --package sonos-sdk --test property_validation -- --ignored --nocapture
```

**Test organization by property group:**

#### Single-Speaker Tests (no group required)

**`test_rendering_control_properties`** — Tests Volume, Mute, Bass, Treble, Loudness:
1. Find reachable speaker (reuse `find_reachable_speaker()` helper pattern)
2. For each property: save original → `watch()` → wait for subscription (200ms) → set new value via API → wait for change event via `system.iter()` (5s timeout) → assert cached value matches → restore original → wait for restore event
3. Uses RAII restoration guards so cleanup happens even on panic

**`test_playback_state_property`** — Tests PlaybackState:
1. Find reachable speaker, save current transport state
2. `watch()` PlaybackState
3. If playing: `pause()` → assert Paused event → `play()` → assert Playing event
4. If paused/stopped: `play()` → assert Playing event → `pause()` → assert Paused event
5. Restore original state

**`test_position_property`** — Tests Position:
1. Find reachable speaker, ensure it's playing (if stopped, skip with message)
2. `watch()` Position
3. `fetch()` Position to get baseline
4. Wait up to 10 seconds for any Position change event
5. Assert: new `position_ms` differs from baseline (tolerance: position advanced or a new track started)
6. **Note:** This test is inherently timing-dependent. Position updates depend on AVTransport NOTIFY frequency. If no event arrives, this is diagnostic data for Phase 3.

**`test_current_track_property`** — Tests CurrentTrack:
1. Find reachable speaker, ensure it's playing with a queue (if stopped or no queue, skip with message)
2. `watch()` CurrentTrack
3. `fetch()` CurrentTrack to get baseline (save current track metadata)
4. Call `next()` to advance to the next track in the queue
5. Wait for CurrentTrack change event (5s timeout)
6. Assert new track metadata differs from baseline
7. Call `previous()` to restore the original track
8. **Note:** Requires the speaker to have a queue with 2+ tracks. If `next()` fails or no queue is available, skip and log diagnostic info.

#### Group Tests (shared fixture, requires 2+ standalone speakers)

**`test_group_properties`** — Tests GroupVolume, GroupMute, GroupVolumeChangeable:
1. Find 2+ standalone speakers (reuse `find_standalone_speakers()` pattern)
2. Create group with `system.create_group()`
3. Wait for topology propagation (poll `system.groups()` until group appears, up to 5s)
4. Get group handle, save original group volume/mute
5. `watch()` GroupVolume, GroupMute, GroupVolumeChangeable on the group
6. **GroupVolumeChangeable:** Assert initial value received (expected: `true`). This is event-only with no setter — we can only verify it's delivered on subscription, not that changes propagate.
7. **GroupVolume:** Set group volume to original+5, wait for event (5s), assert cached value matches
8. **GroupMute:** Set group mute to `true`, wait for event (5s), assert cached value matches
9. Restore group volume/mute
10. Dissolve group via `leave_group()` on all members
11. Wait for topology update confirming speakers are standalone

#### Topology Tests

**`test_topology_properties`** — Tests GroupMembership and Topology:
1. Find 2+ standalone speakers
2. `watch()` GroupMembership on both speakers
3. Bootstrap topology (wait for initial GroupMembership values)
4. Assert both speakers show `is_coordinator: true` (standalone)
5. Create group → wait for GroupMembership change events on both speakers
6. Assert coordinator speaker shows `is_coordinator: true`, member shows `is_coordinator: false`
7. Assert `group_id` matches on both speakers
8. Leave group → wait for GroupMembership change events
9. Assert both speakers return to `is_coordinator: true` with different `group_id`s
10. Dissolve group, restore standalone state

**RAII Restoration Guards:**

Each test wraps mutations in a guard struct that restores the original value on `Drop`:

```rust
struct VolumeGuard<'a> {
    speaker: &'a Speaker,
    original: u8,
}

impl Drop for VolumeGuard<'_> {
    fn drop(&mut self) {
        let _ = self.speaker.set_volume(self.original);
    }
}
```

Similar guards for Mute, Bass, Treble, Loudness, PlaybackState, GroupVolume, GroupMute, and a GroupGuard that dissolves the group.

**Event wait helper:**

```rust
fn wait_for_property_event(
    iter: &ChangeIterator,
    speaker_id: &SpeakerId,
    property_key: &str,
    timeout: Duration,
) -> Option<ChangeEvent> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(event) = iter.recv_timeout(Duration::from_millis(100)) {
            if event.speaker_id == *speaker_id && event.property_key == property_key {
                return Some(event);
            }
        }
    }
    None
}
```

### Phase 3: Root Cause Fixes

Based on findings from Phases 1-2. Known suspects from code review:

1. **Position not in every AVTransport NOTIFY** — Sonos speakers may not include `rel_time` in every AVTransport `LastChange` event. If the event only contains `TransportState`, the decoder produces a `PlaybackState` change but no `Position` change. Fix: supplement with explicit `GetPositionInfo` fetch after any AVTransport event that doesn't include position data.

2. **`let _ = event_tx.send(...)` silently drops events** (`sonos-state/src/event_worker.rs:163,197`) — If the `ChangeIterator` receiver is dropped, events are silently lost. This is by design for shutdown, but could mask issues during normal operation. Fix: log at `warn` level when send fails outside of shutdown.

3. **Group property race** — `GroupRenderingControl` events arriving before `ZoneGroupTopology` establishes group mapping cause `speaker_to_group` lookup failure, silently returning `false`. Fix: buffer group events and replay them after topology is established.

4. **IP-to-speaker lookup failures** — Events from speakers not in the `ip_to_speaker` map are logged and dropped (`event_worker.rs:56-75`). This can happen if a speaker joins the network after initial discovery. Fix: trigger re-discovery or add the speaker dynamically.

5. **Out-of-order events during polling/UPnP transition** — During fallback transition, a poll result could arrive after a UPnP event for the same change, causing a stale value to overwrite a fresh one. Fix: add timestamp comparison in `StateStore::set()` to implement last-write-wins.

Phase 3 scope will be refined based on actual findings. Some suspects may turn out to be non-issues; new issues may surface.

## Technical Considerations

**Architecture impact:** Phases 1-2 add no new public APIs. Phase 3 may require changes to `StateStore`, `event_worker`, and `sonos-stream` internals.

**Performance:** The dashboard watches all 13 properties on all speakers simultaneously. This means one UPnP subscription per (speaker, service) pair — roughly 4 subscriptions per speaker (RenderingControl, AVTransport, GroupRenderingControl, ZoneGroupTopology). With 5 speakers, that's 20 concurrent subscriptions. The existing architecture handles this via the reference-counted subscription manager.

**Testing environment:** All Phase 2 tests require real Sonos hardware. Tests are `#[ignore]` by default. The `a queue with 2+ tracks on the test speaker` environment variable provides an optional playable URI for CurrentTrack testing.

## System-Wide Impact

- **Interaction graph:** `watch()` → lazy EventManager init → UPnP subscription → callback-server → event processing → state update → ChangeEvent. All layers are already wired. No new cross-layer interactions.
- **Error propagation:** Events silently drop at multiple points (IP lookup failure, group mapping failure, channel send failure). Phase 3 will add visibility into these silent drops.
- **State lifecycle risks:** Group property events arriving before topology can orphan state updates. Phase 3 addresses this with event buffering.
- **API surface parity:** No new public APIs in Phases 1-2. Phase 3 changes are internal.

## Acceptance Criteria

### Phase 1: Property Observer Dashboard
- [x] `cargo run -p sonos-sdk --example property_observer` discovers speakers and displays all 9 speaker-scoped properties per speaker
- [x] Dashboard displays group properties on coordinators after topology populates
- [x] Dashboard shows WatchMode (Events/Polling/CacheOnly) per property
- [x] Dashboard shows update count and time-since-last-update per property
- [x] Dashboard updates in real-time when interacting with Sonos app (volume change on phone shows in dashboard within 2 seconds)
- [x] Dashboard runs until Ctrl+C with clean shutdown

### Phase 2: Automated Property Validation Tests
- [x] `test_rendering_control_properties` passes: Volume, Mute, Bass, Treble, Loudness all deliver watch events within 5 seconds of API mutation
- [x] `test_playback_state_property` passes: PlaybackState updates on play/pause/stop
- [x] `test_position_property` provides diagnostic output about Position event frequency during playback
- [x] `test_current_track_property` validates track metadata changes (when `a queue with 2+ tracks on the test speaker` is set) or verifies fetch works during playback
- [x] `test_group_properties` passes: GroupVolume, GroupMute deliver events; GroupVolumeChangeable receives initial value
- [x] `test_topology_properties` passes: GroupMembership updates on group/ungroup; both speakers get correct coordinator status
- [x] All tests restore speaker state on success and on panic (RAII guards)
- [x] All tests run with `cargo test --package sonos-sdk --test property_validation -- --ignored`

### Phase 3: Root Cause Fixes
- [ ] All Phase 2 tests pass reliably (no flaky failures across 5 consecutive runs)
- [ ] Position property updates during active playback
- [ ] No silent event drops in the pipeline (logged at warn level minimum)

## Success Metrics

- All 13 properties deliver `watch()` updates when changed via the Sonos app
- Phase 2 test suite passes 5/5 consecutive runs on real speakers
- Dashboard shows live updates for all properties within 2 seconds of changes

## Dependencies & Risks

**Dependencies:**
- Real Sonos speakers for all testing (at least 2 for group tests)
- Network configuration allowing UPnP callbacks (or polling fallback)
- Optional: `a queue with 2+ tracks on the test speaker` environment variable for CurrentTrack test

**Risks:**
- Position may be fundamentally limited by Sonos firmware (no `rel_time` in NOTIFY events). Mitigation: supplement with explicit fetch after transport state changes.
- GroupVolumeChangeable has no API setter — test is limited to verifying initial value delivery.
- Network timing variability may cause test flakiness. Mitigation: generous timeouts (5s for most, 10s for Position).

## Implementation Files

### New Files
- `sonos-sdk/examples/property_observer.rs` — Phase 1 dashboard
- `sonos-sdk/tests/property_validation.rs` — Phase 2 test suite

### Potentially Modified Files (Phase 3, based on findings)
- `sonos-state/src/event_worker.rs` — Silent event drop logging, group event buffering
- `state-store/src/lib.rs` — Timestamp-based last-write-wins in `StateStore::set()`
- `sonos-stream/src/events/processor.rs` — Position supplemental fetch after AVTransport events

## Sources & References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-03-29-harden-watch-reliability-brainstorm.md](docs/brainstorms/2026-03-29-harden-watch-reliability-brainstorm.md) — Key decisions carried forward: never-miss-events guarantee, all 13 properties, dashboard-first approach.

### Internal References

- Existing integration tests: `sonos-sdk/tests/integration_real_speakers.rs`
- Existing dashboard example: `sonos-sdk/examples/smart_dashboard.rs`
- Property definitions: `sonos-state/src/property.rs`
- Event worker pipeline: `sonos-state/src/event_worker.rs`
- Speaker property handles: `sonos-sdk/src/property/handles.rs`
- Event router race fix: `docs/plans/2026-03-28-fix-event-router-registration-race-plan.md`
- WatchHandle RAII design: `docs/plans/2026-03-25-feat-watch-grace-period-raii-guard-plan.md`

### Related Work

- PR #64: Fix EventRouter registration race (buffer + replay)
- PR #61: Integration test suite
- PR #59: WatchHandle RAII with grace period
