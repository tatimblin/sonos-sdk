---
title: "feat: Complete 5 core services across all SDK layers"
type: feat
status: active
date: 2026-02-24
origin: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md
---

# Complete 5 Core Services Across All SDK Layers

## Overview

A layer-by-layer roadmap to bring AVTransport, RenderingControl, GroupRenderingControl, GroupManagement, and ZoneGroupTopology to full completion. The end state is a feature-complete, documented SDK where users can stream events from all 5 services, watch/get/fetch any property, execute any operation via methods on Speaker/Group, and manage group lifecycle.

Every aspect has a proof of concept but nothing is fully implemented for every service. This plan closes every gap.

(see brainstorm: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md)

## Problem Statement

The SDK has a 4-layer architecture (API → Stream → State → SDK) with 6 checkpoints per service. Of the 5 core services, only AVTransport approaches completeness — and even it has gaps. The SDK is entirely read-only: none of the 44 API operations are exposed for execution. Polling is stubbed for 3 of 5 services. Group management exists only as topology observation.

Users hit these pain points:
- Cannot get initial property values for 6 of 10 properties (no `fetch()`)
- Cannot execute any operations through the SDK (play, pause, set volume)
- Cannot manage groups (create, add/remove speakers, dissolve)
- Firewall users lose 3 of 5 service streams (polling stubs)

## Proposed Solution

Layer-by-layer completion: finish each layer across all 5 services before moving up. This matches the dependency chain and the existing Claude skills.

**7 phases:**
1. Complete API operations (RenderingControl gaps)
2. Complete stream polling (all 5 services)
3. Complete state decoders (GroupRenderingControl + GroupManagement)
4. Complete SDK properties (all fetch() + missing handles)
5. Add SDK operation methods (Speaker + Group)
6. Add group lifecycle management
7. Documentation

## Technical Approach

### Architecture

No architectural changes needed. All patterns are established and proven. Each phase fills gaps within existing abstractions.

```
Phase 1 → sonos-api (operations)
Phase 2 → sonos-stream (polling)
Phase 3 → sonos-state (decoders + properties)
Phase 4 → sonos-sdk (property handles + fetch)
Phase 5 → sonos-sdk (operation methods)
Phase 6 → sonos-sdk (group lifecycle)
Phase 7 → docs (rustdoc + examples + guide)
```

### Implementation Phases

---

#### Phase 1: Complete API Operations

**Goal:** Every Get/Set operation exists for the 5 core services.

**Only RenderingControl has gaps.** All other services are complete.

**Files to modify:**
- `sonos-api/src/services/rendering_control/operations.rs`

**Tasks:**

- [x] Add `GetMuteOperation` using `define_operation_with_response!` macro
  - Request: `{ channel: String }` (auto-adds instance_id)
  - Response: `GetMuteResponse { current_mute: bool }`
  - XML mapping: `{ current_mute: "CurrentMute" }`
  - Note: bool fields may need manual parse (see `AddMemberOperation` for bool pattern — Sonos returns "0"/"1")
- [x] Add `SetMuteOperation` using `define_upnp_operation!` macro
  - Request: `{ channel: String, desired_mute: bool }`
  - Payload: format with `<DesiredMute>{}</DesiredMute>` using "0"/"1" for bool
  - Validation: channel must be "Master", "LF", or "RF"
- [x] Add `GetBassOperation` using `define_operation_with_response!`
  - Response: `GetBassResponse { current_bass: i8 }`
  - XML mapping: `{ current_bass: "CurrentBass" }`
- [x] Add `SetBassOperation` using `define_upnp_operation!`
  - Request: `{ desired_bass: i8 }`
  - Validation: range -10 to +10
- [x] Add `GetTrebleOperation` using `define_operation_with_response!`
  - Response: `GetTrebleResponse { current_treble: i8 }`
  - XML mapping: `{ current_treble: "CurrentTreble" }`
- [x] Add `SetTrebleOperation` using `define_upnp_operation!`
  - Request: `{ desired_treble: i8 }`
  - Validation: range -10 to +10
- [x] Add `GetLoudnessOperation` using `define_operation_with_response!`
  - Request: `{ channel: String }`
  - Response: `GetLoudnessResponse { current_loudness: bool }`
  - Note: bool parse — Sonos returns "0"/"1"
- [x] Add `SetLoudnessOperation` using `define_upnp_operation!`
  - Request: `{ channel: String, desired_loudness: bool }`
  - Validation: channel must be "Master", "LF", or "RF"
- [x] Add unit tests for all 8 new operations (payload construction + response parsing)
- [x] Update `pub use` exports and convenience function aliases

**Pattern reference:** `GetVolumeOperation` at `sonos-api/src/services/rendering_control/operations.rs` for Get pattern. `SetVolumeOperation` for Set pattern. `AddMemberOperation` at `sonos-api/src/services/group_management/operations.rs` for manual bool handling.

**Success criteria:** `cargo test -p sonos-api` passes. All 8 operations have tests.

---

#### Phase 2: Complete Stream Polling

**Goal:** Every service has a real `ServicePoller` implementation (no stubs).

**Files to modify:**
- `sonos-stream/src/polling/strategies.rs`

**Tasks:**

- [ ] **AVTransport poller** — add `GetPositionInfo` call
  - Current: only calls `get_transport_info_operation()`, position/track data are TODOs (empty strings)
  - Add: call `get_position_info_operation()` and populate `current_track_uri`, `track_duration`, `track_metadata`, `rel_time` in `AVTransportState`
  - Update `parse_for_changes()` to detect track/position changes from the new data
  - Pattern: follow existing `get_transport_info_operation()` call pattern at `strategies.rs:73-100`

- [ ] **RenderingControl poller** — add mute, bass, treble, loudness queries
  - Current: only calls `get_volume_operation("Master")`, mute hardcoded `false`
  - Add: call `get_mute_operation("Master")` (from Phase 1)
  - Add: call `get_bass_operation()`, `get_treble_operation()`, `get_loudness_operation("Master")` (from Phase 1)
  - Extend `RenderingControlState` struct with `mute: bool`, `bass: i8`, `treble: i8`, `loudness: bool`
  - Update `parse_for_changes()` to detect changes for all fields
  - Pattern: follow existing volume call at `strategies.rs:213-236`

- [ ] **GroupRenderingControl poller** — replace stub with real implementation
  - Replace `UnsupportedService` error at `strategies.rs:340`
  - Call `get_group_volume()` and `get_group_mute()` operations
  - Define `GroupRenderingControlState { group_volume: u16, group_mute: bool }`
  - Implement `parse_for_changes()` to detect volume and mute changes

- [ ] **ZoneGroupTopology poller** — replace stub with real implementation
  - Replace `UnsupportedService` error at `strategies.rs:286`
  - Call `get_zone_group_state_operation()` (exists in sonos-api)
  - Define `ZoneGroupTopologyState` with serialized group state
  - Implement `parse_for_changes()` to detect topology changes
  - Note: `GetZoneGroupState` response is XML-heavy; need to serialize/compare zone groups

- [ ] **GroupManagement poller** — determine strategy
  - Current stub comment says "GroupManagement doesn't currently emit events"
  - GroupManagement has no Get operations that return current state — it's action-only (AddMember, RemoveMember, etc.)
  - Decision: implement as a minimal poller that returns a stable state (no polling needed since group changes are reflected via ZoneGroupTopology events). Replace error with a no-op poller that returns unchanged state.

- [ ] Add/update tests for all modified pollers

**Success criteria:** `cargo test -p sonos-stream` passes. No `UnsupportedService` stubs remain (all pollers either poll real state or return stable no-op state).

---

#### Phase 3: Complete State Layer

**Goal:** Every event type decodes all its properties. All property types are defined.

**Files to modify:**
- `sonos-state/src/property.rs` — new property structs
- `sonos-state/src/decoder.rs` — decoder functions + `PropertyChange` enum

**Tasks:**

- [ ] **Define `GroupMute` property** in `property.rs`
  - Struct: `GroupMute(pub bool)`
  - `Property::KEY = "group_mute"`
  - `SonosProperty::SCOPE = Scope::Group`
  - `SonosProperty::SERVICE = Service::GroupRenderingControl`
  - Pattern: follow `GroupVolume` at `property.rs:186-207`

- [ ] **Define `GroupVolumeChangeable` property** in `property.rs`
  - Struct: `GroupVolumeChangeable(pub bool)`
  - `Property::KEY = "group_volume_changeable"`
  - `SonosProperty::SCOPE = Scope::Group`
  - `SonosProperty::SERVICE = Service::GroupRenderingControl`

- [ ] **Add `PropertyChange` variants** in `decoder.rs`
  - Add `GroupMute(GroupMute)` and `GroupVolumeChangeable(GroupVolumeChangeable)` to enum at `decoder.rs:42-54`

- [ ] **Update `decode_group_rendering_control()`** in `decoder.rs`
  - Current: only extracts `group_volume` at `decoder.rs:232-240`
  - Add: extract `group_mute` from `event.group_mute`
  - Add: extract `group_volume_changeable` from `event.group_volume_changeable`

- [ ] **GroupManagement state** — assess what properties are meaningful
  - GroupManagementEvent has fields like `group_coordinates_uri`, `local_group_uuid`, `reset_volume_after`, `volume_av_transport_uri`
  - These are operational metadata, not user-facing properties
  - Decision: GroupManagement changes are reflected via ZoneGroupTopology (topology events). No user-facing properties needed for GroupManagement.
  - Update decoder to remain as empty vec but add a comment explaining the intentional decision

- [ ] Export new property types from `sonos-state/src/lib.rs`
- [ ] Add tests for updated decoders

**Success criteria:** `cargo test -p sonos-state` passes. GroupRenderingControl decoder extracts all 3 fields.

---

#### Phase 4: Complete SDK Properties

**Goal:** Every property has `fetch()` support. All missing handles exist.

**Files to modify:**
- `sonos-sdk/src/property/handles.rs` — Fetchable impls
- `sonos-sdk/src/speaker.rs` — new property handle fields (if any)
- `sonos-sdk/src/group.rs` — new GroupMute handle

**Tasks:**

- [ ] **Add `Fetchable` impl for `Mute`**
  - Operation: `GetMuteOperation` (from Phase 1)
  - `build_operation()`: call `rendering_control::get_mute_operation("Master")`
  - `from_response()`: `Mute(response.current_mute)`
  - Pattern: follow Volume Fetchable at `handles.rs:412-424`

- [ ] **Add `Fetchable` impl for `Bass`**
  - Operation: `GetBassOperation` (from Phase 1)
  - `from_response()`: `Bass(response.current_bass)`

- [ ] **Add `Fetchable` impl for `Treble`**
  - Operation: `GetTrebleOperation` (from Phase 1)
  - `from_response()`: `Treble(response.current_treble)`

- [ ] **Add `Fetchable` impl for `Loudness`**
  - Operation: `GetLoudnessOperation` (from Phase 1)
  - `build_operation()`: call `rendering_control::get_loudness_operation("Master")`
  - `from_response()`: `Loudness(response.current_loudness)`

- [ ] **Add `Fetchable` impl for `CurrentTrack`**
  - Operation: `GetPositionInfoOperation` (exists)
  - `from_response()`: extract title, artist, album, album_art_uri, uri from response
  - Note: GetPositionInfo returns `track_uri`, `track_metadata` (DIDL-Lite XML) — needs metadata parsing like the decoder does at `decoder.rs:200-219`

- [ ] **Add `Fetchable` impl for `GroupMembership`**
  - Operation: `GetZoneGroupStateOperation` (exists)
  - `from_response()`: parse zone groups to find the speaker's group and coordinator status
  - Note: this is more complex — response contains full topology, need to extract the relevant speaker's membership

- [ ] **Add `GroupMuteHandle` to Group struct**
  - Define `GroupMuteHandle = GroupPropertyHandle<GroupMute>` type alias in `handles.rs`
  - Add `pub mute: GroupMuteHandle` field to `Group` struct at `group.rs:36-54`
  - Initialize in `Group::from_info()` at `group.rs:61-84`

- [ ] **Add `GroupFetchable` impl for `GroupMute`**
  - Operation: `GetGroupMuteOperation` (exists in sonos-api)
  - `from_response()`: `GroupMute(response.current_mute)`

- [ ] **Add `GroupVolumeChangeableHandle` to Group struct** (optional — consider if users need this)
  - If yes: same pattern as GroupMuteHandle

- [ ] Remove event-only comments for properties that now have `fetch()` at `handles.rs:461-481`
- [ ] Add tests for all new Fetchable/GroupFetchable implementations

**Success criteria:** `cargo test -p sonos-sdk` passes. Every property handle supports `get()`, `watch()`, and `fetch()`.

---

#### Phase 5: SDK Operation Methods

**Goal:** Users can execute any operation via methods on Speaker/Group.

This is the largest phase — the SDK is currently entirely read-only.

**Files to modify:**
- `sonos-sdk/src/speaker.rs` — methods for AVTransport, RenderingControl, ZoneGroupTopology ops
- `sonos-sdk/src/group.rs` — methods for GroupRenderingControl, GroupManagement ops

**Design decision:** Methods on Speaker/Group, not generic execute (see brainstorm). Each method wraps the corresponding `SonosClient::execute()` call.

**Pattern for operation methods:**
```rust
impl Speaker {
    pub fn play(&self) -> Result<(), SdkError> {
        let op = av_transport::play_operation("1");
        self.context.api_client.execute_at(&self.ip, op)?;
        Ok(())
    }
}
```

**Tasks — AVTransport methods on Speaker:**

- [ ] `play()` — PlayOperation
- [ ] `pause()` — PauseOperation
- [ ] `stop()` — StopOperation
- [ ] `next()` — NextOperation
- [ ] `previous()` — PreviousOperation
- [ ] `seek(target: &str, unit: SeekUnit)` — SeekOperation
- [ ] `set_av_transport_uri(uri: &str, metadata: &str)` — SetAVTransportURIOperation
- [ ] `set_next_av_transport_uri(uri: &str, metadata: &str)` — SetNextAVTransportURIOperation
- [ ] `get_media_info()` — GetMediaInfoOperation
- [ ] `get_transport_settings()` — GetTransportSettingsOperation
- [ ] `get_current_transport_actions()` — GetCurrentTransportActionsOperation
- [ ] `set_play_mode(mode: PlayMode)` — SetPlayModeOperation
- [ ] `get_crossfade_mode()` / `set_crossfade_mode(enabled: bool)` — Get/SetCrossfadeModeOperation
- [ ] `configure_sleep_timer(duration: &str)` / `get_remaining_sleep_timer()` — Sleep timer ops
- [ ] `add_uri_to_queue(uri: &str, metadata: &str, position: u32)` — AddURIToQueueOperation
- [ ] `remove_track_from_queue(track: u32)` — RemoveTrackFromQueueOperation
- [ ] `remove_all_tracks_from_queue()` — RemoveAllTracksFromQueueOperation
- [ ] `save_queue(title: &str)` / `create_saved_queue(title: &str)` — Queue save ops
- [ ] `become_standalone()` — BecomeCoordinatorOfStandaloneGroupOperation
- [ ] `delegate_coordination_to(new_coordinator_id: &str)` — DelegateGroupCoordinationToOperation

**Tasks — RenderingControl methods on Speaker:**

- [ ] `set_volume(volume: u8)` — SetVolumeOperation
- [ ] `set_relative_volume(adjustment: i8)` — SetRelativeVolumeOperation
- [ ] `set_mute(muted: bool)` — SetMuteOperation (from Phase 1)
- [ ] `set_bass(level: i8)` — SetBassOperation (from Phase 1)
- [ ] `set_treble(level: i8)` — SetTrebleOperation (from Phase 1)
- [ ] `set_loudness(enabled: bool)` — SetLoudnessOperation (from Phase 1)

**Tasks — GroupRenderingControl methods on Group:**

- [ ] `set_volume(volume: u16)` — SetGroupVolumeOperation
- [ ] `set_relative_volume(adjustment: i16)` — SetRelativeGroupVolumeOperation
- [ ] `set_mute(muted: bool)` — SetGroupMuteOperation
- [ ] `snapshot_volume()` — SnapshotGroupVolumeOperation

**Tasks — GroupManagement methods on Group:**

- [ ] `add_member(member_id: &str)` — AddMemberOperation
- [ ] `remove_member(member_id: &str)` — RemoveMemberOperation

**Tasks — Testing:**
- [ ] Tests for each operation method category (don't need per-method tests if the pattern is uniform, test representative samples)

**Success criteria:** `cargo test -p sonos-sdk` passes. Users can call every operation from the 5 core services via Speaker/Group methods.

---

#### Phase 6: Group Lifecycle Management

**Goal:** Full group create/join/leave/dissolve through the SDK.

**Files to modify:**
- `sonos-sdk/src/system.rs` — group creation/dissolution methods on SonosSystem
- `sonos-sdk/src/group.rs` — group modification methods
- `sonos-sdk/src/speaker.rs` — speaker group methods

**Tasks:**

- [ ] **`speaker.join_group(group: &Group)`** — calls AddMemberOperation on the group coordinator
- [ ] **`speaker.leave_group()`** — calls BecomeCoordinatorOfStandaloneGroupOperation (makes speaker standalone)
- [ ] **`group.add_speaker(speaker: &Speaker)`** — calls AddMemberOperation on coordinator
- [ ] **`group.remove_speaker(speaker: &Speaker)`** — calls RemoveMemberOperation on coordinator
- [ ] **`system.create_group(coordinator: &Speaker, members: &[&Speaker])`** — adds each member to coordinator's group
- [ ] **`group.dissolve()`** — removes all non-coordinator members, each becomes standalone
- [ ] **Live topology updates** — verify that after group operations, ZoneGroupTopology events fire and the SDK's group list auto-updates
  - The event pipeline already handles this (ZoneGroupTopology → decoder → state store → topology)
  - Verify that `system.groups()` reflects changes after operations
- [ ] Integration-style tests verifying group lifecycle operations compose correctly

**Success criteria:** Users can create groups, add/remove speakers, and dissolve groups. Topology auto-updates after mutations.

---

#### Phase 7: Documentation

**Goal:** Feature complete + documented. Rustdoc, examples, getting-started guide.

**Files to create/modify:**
- Rustdoc comments across all public APIs
- Example files in each crate
- Getting-started guide
- Updated SPEC files

**Tasks:**

- [ ] **Rustdoc for sonos-sdk public API**
  - `SonosSystem` — all methods with examples
  - `Speaker` — all property handles and operation methods
  - `Group` — all property handles, operation methods, lifecycle methods
  - Property handle types — document get/watch/fetch pattern

- [ ] **Rustdoc for sonos-api public API**
  - All new RenderingControl operations
  - Ensure existing operations have doc comments

- [ ] **Update examples**
  - Update `sonos-state/examples/reactive_dashboard.rs` to show all properties
  - Create example showing operation execution (play/pause/volume)
  - Create example showing group management lifecycle

- [ ] **Getting-started guide** in `docs/`
  - Discovery → System creation → Speaker access → Property watching → Operation execution → Group management
  - Code examples for each step

- [ ] **Update SPEC files** to reflect final state
  - `docs/specs/sonos-api.md` — new operations
  - `docs/specs/sonos-stream.md` — completed pollers
  - `docs/specs/sonos-state.md` — new properties
  - `docs/specs/sonos-sdk.md` — operation methods, group lifecycle

- [ ] **Update `docs/STATUS.md`** — mark all 5 core services as Done across all columns

**Success criteria:** `cargo doc --no-deps` builds cleanly. All public items have doc comments. Examples compile and demonstrate key workflows.

---

## Acceptance Criteria

### Functional Requirements

- [ ] All 5 services have complete API operations (44+ total)
- [ ] All 5 services stream events with real polling fallback
- [ ] All properties support get(), watch(), and fetch()
- [ ] All operations are callable via methods on Speaker/Group
- [ ] Groups can be created, modified (add/remove speakers), and dissolved
- [ ] Live topology updates reflect group changes automatically
- [ ] `docs/STATUS.md` shows Done for all 5 services across all columns

### Quality Gates

- [ ] `cargo test` passes across entire workspace
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo doc --no-deps` builds cleanly
- [ ] All public APIs have rustdoc comments
- [ ] Getting-started guide exists with working code examples

## Dependencies & Prerequisites

- **Phase 1 has no dependencies** — can start immediately
- **Phase 2 depends on Phase 1** — RenderingControl poller needs new Get operations
- **Phase 3 has no dependency on Phase 2** — decoders work on events, not polling
- **Phase 4 depends on Phase 1** — Fetchable impls need Get operations
- **Phase 5 depends on Phase 1** — operation methods need all operations to exist
- **Phase 6 depends on Phase 5** — group lifecycle uses operation methods
- **Phase 7 can start partially during Phases 4-6** — doc comments can be written as code is added

**Parallelism opportunity:** Phases 3 and 4 can run concurrently with Phase 2 (state decoders don't depend on polling).

```
Phase 1 (API) ──┬── Phase 2 (Polling) ──────────────────┐
                ├── Phase 3 (State) ──── Phase 4 (SDK) ──┼── Phase 5 (Operations) ── Phase 6 (Groups) ── Phase 7 (Docs)
                └── Phase 4 (SDK) ───────────────────────┘
```

## Risk Analysis & Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| Bool fields in UPnP operations use "0"/"1" not "true"/"false" | Get operations return wrong values | Follow AddMemberOperation pattern for manual bool parsing; test against real speakers |
| GroupManagement poller has no meaningful state to poll | Incomplete polling layer | Accept no-op poller — group changes come via ZoneGroupTopology events |
| CurrentTrack fetch requires DIDL-Lite XML parsing | Complex response parsing | Reuse `parse_track_metadata()` from decoder.rs |
| GroupMembership fetch returns full topology, not single speaker | Complex response extraction | Parse GetZoneGroupState and filter to target speaker |
| Group lifecycle operations may have timing issues | Operations succeed but state doesn't update immediately | Rely on ZoneGroupTopology event pipeline; add brief wait-for-update if needed |

## Sources & References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md](../brainstorms/2026-02-24-product-roadmap-brainstorm.md) — Key decisions: layer-by-layer structure, methods on Speaker/Group for operations, polling must-have, full group lifecycle, feature complete + documented

### Internal References

- Operation macro patterns: `sonos-api/src/operation/macros.rs`
- Bool handling pattern: `sonos-api/src/services/group_management/operations.rs` (AddMemberOperation)
- ServicePoller trait: `sonos-stream/src/polling/strategies.rs:49-60`
- Real poller example: `sonos-stream/src/polling/strategies.rs:63-189` (AVTransport)
- Fetchable trait: `sonos-sdk/src/property/handles.rs:146-155`
- Existing Fetchable impls: `sonos-sdk/src/property/handles.rs:412-459`
- Property definitions: `sonos-state/src/property.rs`
- Decoder pattern: `sonos-state/src/decoder.rs:122-133`
- Group access: `sonos-sdk/src/system.rs:198-251`
- Status tracking: `docs/STATUS.md`
- Service addition guide: `docs/adding-services.md`
