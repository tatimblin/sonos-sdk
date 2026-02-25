---
title: "feat: Complete stream polling for all 5 core services"
type: feat
status: completed
date: 2026-02-24
origin: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md
---

# Complete Stream Polling for All 5 Core Services

## Overview

Phase 2 of the roadmap: replace all stub and incomplete polling strategies in `sonos-stream` with real implementations. After this work, every service has a functional `ServicePoller` — no stubs remain. Firewall users get degraded but functional state updates for all 5 core services via polling fallback.

(see brainstorm: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md — Key decision: "Polling is must-have for all 5 services")

## Problem Statement

The polling fallback system (used when firewalls block UPnP event callbacks) is incomplete:

- **AVTransport** — only calls `GetTransportInfo`; position, track URI, duration, and metadata are empty strings
- **RenderingControl** — only queries volume; mute is hardcoded `false`, bass/treble/loudness are not polled
- **GroupRenderingControl** — returns `UnsupportedService` error (stub)
- **ZoneGroupTopology** — returns `UnsupportedService` error (stub)
- **GroupManagement** — returns `UnsupportedService` error (stub)

Additionally, a **pre-existing bug** in the polling scheduler means change detection never fires — `last_state` is updated before `parse_state_changes()` is called, so old and new states always compare equal.

The `change_to_event_data()` function in the scheduler only handles AVTransport and RenderingControl — all other services fall through to a wildcard that produces an empty `AVTransportEvent`, silently misrouting events from new pollers.

## Proposed Solution

Fix the scheduler bug, implement all 5 pollers, and update the event conversion to route all service types correctly.

**Files to modify:**
- `sonos-stream/src/polling/strategies.rs` — all poller implementations and state structs
- `sonos-stream/src/polling/scheduler.rs` — fix state comparison bug, extend `change_to_event_data()`

**No new files needed.** All event types already exist in `sonos-stream/src/events/types.rs`.

## Technical Considerations

### Architecture

No architectural changes. All patterns are established. The `ServicePoller` trait, `StateChange` enum, `DeviceStatePoller` coordinator, and `PollingTask` lifecycle are all in place.

### Key Design Decisions

1. **GenericChange for bass/treble/loudness** — use `StateChange::GenericChange { field, old_value, new_value }` with well-defined field names ("bass", "treble", "loudness") rather than adding dedicated variants. The downstream `RenderingControlEvent` already uses `Option<String>` for these fields, making string-based mapping natural.

2. **TopologyChanged variant** — add a new `StateChange::TopologyChanged { event: ZoneGroupTopologyEvent }` variant that carries the fully-parsed topology. This avoids the per-field granularity problem: the downstream `event_worker` expects a single complete `ZoneGroupTopologyEvent` because `apply_topology_changes()` clears all groups before repopulating.

3. **GroupRenderingControlChanged variant** — add a new `StateChange::GroupRenderingControlChanged { event: GroupRenderingControlEvent }` variant for the same reason: group rendering state should arrive as a complete event, not split across multiple sparse events.

4. **GroupManagement no-op returns `Ok`** — return `Ok("{}")` instead of `Err(UnsupportedService)` to avoid triggering error count escalation and poller termination in the scheduler.

5. **ZoneGroupTopology raw string comparison for quick change detection** — compare the raw `zone_group_state` XML string for equality first. Only parse the XML into structured data when a change is detected. This avoids expensive XML parsing on every poll cycle.

6. **Partial API failure = full poll failure** — consistent with existing behavior. If any of the multiple API calls in a poller fails, the entire poll fails and the scheduler handles backoff.

7. **`#[serde(default)]` on new state struct fields** — defensive robustness for any future scenario where old-format state strings are deserialized (e.g., if state persistence is added later). Not strictly needed today since `last_state` resets to `None` on every `PollingTask` creation, but harmless and good practice.

8. **Reuse existing ZoneGroupTopology XML parser** — `sonos-api/src/services/zone_group_topology/events.rs` already has a complete parser (`ZoneGroupTopologyEvent::from_xml()`) that handles HTML-entity-encoded XML, nested zone groups, members, satellites, and network info. The event processor at `sonos-stream/src/events/processor.rs:258-304` already converts sonos-api types to sonos-stream types. The poller should reuse this infrastructure rather than writing a new parser.

## Implementation Phases

---

### Phase 0: Fix Scheduler State Comparison Bug

**Priority:** Must be done first — without this fix, no poller (existing or new) will ever emit events.

**File:** `sonos-stream/src/polling/scheduler.rs`

**Bug:** At lines 146-173, `last_state` is updated (line 151) before `parse_state_changes()` is called (line 172). The code at line 170 clones the already-updated `last_state`, so `previous_state == current_state` always, and zero changes are detected.

**Fix:** Save the old state before updating `last_state`:

```rust
// Before (buggy):
let state_changed = {
    let previous_state = last_state.clone();
    if let Some(ref previous) = previous_state {
        if previous != &current_state {
            last_state = Some(current_state.clone());
            true
        } else { false }
    } else {
        last_state = Some(current_state.clone());
        true
    }
};
if state_changed {
    let previous_state = last_state.clone(); // BUG: already updated!
    // ...
}

// After (fixed):
let previous_state = last_state.clone();
let state_changed = if let Some(ref previous) = previous_state {
    previous != &current_state
} else {
    true // First poll
};
if state_changed {
    last_state = Some(current_state.clone());
    // Use previous_state (saved before update) for change detection
}
```

**Tasks:**
- [x] Fix state comparison logic to save old state before updating
- [x] Add unit test verifying change detection works correctly across sequential polls

**Success criteria:** A state transition from `{"volume":50}` to `{"volume":75}` produces a `VolumeChanged` event.

---

### Phase 1: Extend `change_to_event_data()` for All Services

**Priority:** Must be done before implementing new pollers — otherwise events from new pollers get silently misrouted as empty `AVTransportEvent`.

**File:** `sonos-stream/src/polling/scheduler.rs`

**Tasks:**

- [x] Add `StateChange::TopologyChanged` variant to strategies.rs (carries `ZoneGroupTopologyEvent`)
- [x] Add `StateChange::GroupRenderingControlChanged` variant to strategies.rs (carries `GroupRenderingControlEvent`)
- [x] Add match arm for `Service::RenderingControl` to handle `GenericChange` with fields "bass", "treble", "loudness" — map to `RenderingControlEvent.bass`, `.treble`, `.loudness`
- [x] Add match arm for `Service::GroupRenderingControl` — extract `GroupRenderingControlEvent` from `StateChange::GroupRenderingControlChanged` and wrap in `EventData::GroupRenderingControlEvent`
- [x] Add match arm for `Service::ZoneGroupTopology` — extract `ZoneGroupTopologyEvent` from `StateChange::TopologyChanged` and wrap in `EventData::ZoneGroupTopologyEvent`
- [x] Add match arm for `Service::GroupManagement` — return `EventData::GroupManagementEvent` with default/empty values (no-op poller won't produce changes, but the match arm should exist for completeness)
- [x] Remove the wildcard `_` fallback that silently produces `AVTransportEvent` — replace with explicit match arms or a panic for truly unexpected services

---

### Phase 2: Complete RenderingControlPoller

**File:** `sonos-stream/src/polling/strategies.rs`

**Current state:** Only calls `get_volume_operation("Master")`, mute hardcoded `false`.

**Tasks:**

- [x] Extend `RenderingControlState` struct with `bass: i8`, `treble: i8`, `loudness: bool` fields (with `#[serde(default)]`)
- [x] Add `get_mute_operation("Master".to_string())` call in `poll_state()` — populates `mute` field
- [x] Add `get_bass_operation()` call — populates `bass` field
- [x] Add `get_treble_operation()` call — populates `treble` field
- [x] Add `get_loudness_operation("Master".to_string())` call — populates `loudness` field
- [x] Update `parse_for_changes()` to detect bass, treble, loudness changes using `StateChange::GenericChange`
- [x] Update test `test_rendering_control_change_detection` to cover new fields

**Pattern reference:** Existing `get_volume_operation` call at `strategies.rs:215-221`. Repeat the same build/execute/map_err pattern for each new operation.

---

### Phase 3: Complete AVTransportPoller

**File:** `sonos-stream/src/polling/strategies.rs`

**Current state:** Only calls `get_transport_info_operation()`; 7 fields are TODOs with empty strings/zeroes.

**Tasks:**

- [x] Add `get_position_info_operation()` call in `poll_state()`
- [x] Populate `current_track_uri` from `position_info.track_uri`
- [x] Populate `track_duration` from `position_info.track_duration`
- [x] Populate `track_metadata` from `position_info.track_meta_data`
- [x] Populate `rel_time` from `position_info.rel_time`
- [x] Populate `abs_time` from `position_info.abs_time`
- [x] Populate `rel_count` from `position_info.rel_count as u32`
- [x] Populate `abs_count` from `position_info.abs_count as u32`
- [x] Remove all TODO comments from the state construction
- [x] Update test `test_av_transport_change_detection` to verify position/track change detection works

**Pattern reference:** Existing `get_transport_info_operation()` call at `strategies.rs:71-77`.

---

### Phase 4: Implement GroupRenderingControlPoller

**File:** `sonos-stream/src/polling/strategies.rs`

**Current state:** Returns `Err(PollingError::UnsupportedService)`.

**Tasks:**

- [x] Define `GroupRenderingControlState` struct: `{ group_volume: u16, group_mute: bool }`
- [x] Implement `poll_state()` — call `get_group_volume_operation()` and `get_group_mute_operation()`
- [x] Implement `parse_for_changes()` — compare old/new, emit `StateChange::GroupRenderingControlChanged` carrying a `GroupRenderingControlEvent` with the new values
- [x] Add unit test for GroupRenderingControl change detection
- [x] Remove stub comment from struct doc

**Note:** GroupRenderingControl operations must be sent to the group coordinator. The poller does not validate this — it's the registration layer's responsibility. If the target speaker is not a coordinator, the SOAP call will fail and the scheduler will handle the error via normal backoff.

---

### Phase 5: Implement ZoneGroupTopologyPoller

**File:** `sonos-stream/src/polling/strategies.rs`

**Current state:** Returns `Err(PollingError::UnsupportedService)`.

This poller is more complex than the others because `GetZoneGroupState` returns raw XML. However, **the XML parsing infrastructure already exists** in `sonos-api/src/services/zone_group_topology/events.rs` — the `ZoneGroupTopologyEvent` type has `from_xml()` and `zone_groups()` methods that handle HTML-entity-decoded XML, nested zone groups, members, satellites, and network info. The event processor at `sonos-stream/src/events/processor.rs:258-304` already converts from sonos-api types to sonos-stream types.

**Tasks:**

- [x] Define `ZoneGroupTopologyState` struct: `{ zone_group_state_xml: String }` — stores the raw XML for fast string comparison
- [x] Implement `poll_state()` — call `get_zone_group_state_operation()`, store the raw `zone_group_state` string
- [x] Implement `parse_for_changes()`:
  1. Compare old and new `zone_group_state_xml` strings directly (fast path)
  2. If equal, return empty vec (no changes)
  3. If different, parse the new XML and build a `ZoneGroupTopologyEvent`
  4. Emit a single `StateChange::TopologyChanged { event }` carrying the full parsed topology
- [x] Implement `parse_topology_xml()` helper method on `ZoneGroupTopologyPoller`:
  - Wrap the raw `zone_group_state` string in a UPnP propertyset envelope
  - Call `sonos_api::services::zone_group_topology::ZoneGroupTopologyEvent::from_xml()` to parse
  - Convert from sonos-api types to sonos-stream types (follow the pattern at `processor.rs:264-297`)
  - On parse failure, emit `StateChange::GenericChange` with the raw string so the change isn't silently lost
- [x] Add unit test with sample topology XML fixture (reuse fixtures from `sonos-api` topology event tests)
- [x] Remove stub comment from struct doc

---

### Phase 6: Implement GroupManagement No-Op Poller

**File:** `sonos-stream/src/polling/strategies.rs`

**Current state:** Returns `Err(PollingError::UnsupportedService)`, which triggers error escalation and eventual poller termination after 5 failures.

**Decision:** GroupManagement has no Get operations — it's action-only (AddMember, RemoveMember). Group state changes are reflected via ZoneGroupTopology events. The poller should be a stable no-op that never triggers errors.

**Tasks:**

- [x] Change `poll_state()` to return `Ok("{}")` (empty stable JSON)
- [x] Keep `parse_for_changes()` returning empty vec (no changes from identical strings)
- [x] Update struct doc comment to explain the intentional no-op design
- [x] Update test `test_group_management_poller_stub` to verify it returns `Ok` instead of `Err`

---

### Cross-Cutting: Update Existing Tests

After all phases are complete, update the pre-existing tests that reference stubs:

- [x] Update `test_device_poller_creation` comment to note pollers are now real (not stubs)
- [x] Add `test_change_to_event_data_all_services` — verify `change_to_event_data()` produces correct `EventData` variant for each service

---

## Acceptance Criteria

- [x] `cargo test -p sonos-stream` passes (49 pass, 2 pre-existing iterator failures)
- [x] `cargo clippy -p sonos-stream` passes with no warnings (only pre-existing `uninlined_format_args` style warnings)
- [x] No `UnsupportedService` stubs remain (all pollers return `Ok`)
- [x] Scheduler state comparison bug is fixed and tested
- [x] `change_to_event_data()` has explicit match arms for all 5 services
- [x] AVTransport poller populates all position/track fields
- [x] RenderingControl poller queries volume, mute, bass, treble, loudness
- [x] GroupRenderingControl poller queries group volume and group mute
- [x] ZoneGroupTopology poller queries and parses zone group state
- [x] GroupManagement poller returns stable no-op state
- [x] All new code has unit tests

## Dependencies & Risks

**Dependencies:**
- Phase 1 of the roadmap (API operations) is complete — all Get operations exist in sonos-api
- No dependency on sonos-state or sonos-sdk changes

**Risks:**

| Risk | Impact | Mitigation |
|------|--------|------------|
| ZoneGroupTopology XML parsing complexity | Most complex poller — double-encoded XML, nested structures | Reuse existing `ZoneGroupTopologyEvent::from_xml()` parser; compare raw strings first, parse only on change; add fixture-based tests |
| RenderingControl makes 5 blocking HTTP calls per poll | Could block tokio thread pool under high concurrency | Acceptable for typical use (few speakers); document as known limitation |
| GroupRenderingControl must target group coordinator | Non-coordinator speakers return SOAP errors | Document as registration-layer responsibility; poller handles errors via normal backoff |
| Pre-existing bug in scheduler is a silent failure | All polling change detection is currently broken | Fix first (Phase 0) before implementing new pollers |

**Downstream gaps (out of scope, tracked separately):**
- sonos-state `decode_group_rendering_control()` does not extract `group_mute` or `group_volume_changeable` — this is Phase 3 of the roadmap
- `GroupMute` property type does not exist in sonos-state yet — also Phase 3

**Related gap (separate plan):**
- Firewall detection has 3 integration bugs: `EventDetector` not connected to `EventBroker`, polling request channel sender dropped immediately (`_` prefix at `broker.rs:277`), and event timeout monitoring is stubbed (only prints debug message). First-subscription detection works, but mid-stream event loss does not trigger polling fallback. See separate firewall detection plan.

## Sources & References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md](../brainstorms/2026-02-24-product-roadmap-brainstorm.md) — Key decisions: polling must-have for all 5 services, layer-by-layer structure
- **Roadmap plan:** [docs/plans/2026-02-24-feat-complete-core-services-roadmap-plan.md](2026-02-24-feat-complete-core-services-roadmap-plan.md) — Phase 2 section

### Internal References

- ServicePoller trait: `sonos-stream/src/polling/strategies.rs:49-60`
- AVTransport poller (pattern reference): `sonos-stream/src/polling/strategies.rs:62-189`
- RenderingControl poller: `sonos-stream/src/polling/strategies.rs:206-275`
- Stub pollers: `sonos-stream/src/polling/strategies.rs:277-353`
- Scheduler polling loop: `sonos-stream/src/polling/scheduler.rs:96-245`
- change_to_event_data: `sonos-stream/src/polling/scheduler.rs:268-341`
- Event types: `sonos-stream/src/events/types.rs`
- RenderingControl operations: `sonos-api/src/services/rendering_control/operations.rs`
- GroupRenderingControl operations: `sonos-api/src/services/group_rendering_control/operations.rs`
- ZoneGroupTopology operations: `sonos-api/src/services/zone_group_topology/operations.rs`
- Status tracking: `docs/STATUS.md`
