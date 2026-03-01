---
title: "feat: Group lifecycle management"
type: feat
status: completed
date: 2026-02-28
origin: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md
---

# Phase 6: Group Lifecycle Management

## Enhancement Summary

**Deepened on:** 2026-02-28
**Sections enhanced:** Technical Approach, Files to Modify, Tasks, Edge Cases, Security
**Review agents used:** architecture-strategist, code-simplicity-reviewer, pattern-recognition-specialist, performance-oracle, best-practices-researcher, security-sentinel

### Key Improvements
1. **Store boot_seq on SpeakerInfo** instead of separate `RwLock<HashMap>` — all 6 agents unanimously agreed this co-locates speaker data, eliminates an extra lock, and follows existing patterns
2. **Fix XML injection in AddMember** — security-sentinel found `AddMemberOperation.build_payload()` does NOT escape `member_id`, unlike `RemoveMember` which does. Pre-requisite fix before Phase 6
3. **dissolve() uses remove_speaker()** not `become_standalone()` — pattern-recognition-specialist identified this violates coordinator-targeting pattern. `remove_speaker()` sends to coordinator IP; `become_standalone()` sends to member IP
4. **Keep `_api_client` private** on SonosSystem — `create_group()` delegates through `get_group_for_speaker()` which returns a Group that already has api_client. No rename needed
5. **Add coordinator validation guards** using `SdkError::InvalidOperation` error variant

### New Considerations Discovered
- AddMember XML payload missing `xml_escape()` — CRITICAL security fix needed in sonos-api before Phase 6
- Performance is network-I/O-dominated (5-50ms per SOAP call); all HashMap/String overhead is negligible (<2us per event)
- Sequential operations in dissolve/create_group are acceptable (Sonos devices serialize operations)
- Convenience methods (join_group, leave_group) debated: simplicity reviewer recommends removing, architecture reviewer endorses for ergonomics. **Decision: keep them** — they're one-liners with zero maintenance cost and match the brainstorm's ergonomic API goal

## Overview

Phase 6 transforms the SDK from group-read-only to full group lifecycle management. Users can create groups, add/remove speakers, and dissolve groups through ergonomic methods on `Speaker`, `Group`, and `SonosSystem`.

The topology auto-update pipeline (ZoneGroupTopology events → decoder → state store) is already fully implemented. After any group mutation, the SDK's `system.groups()` automatically reflects the new state via UPnP events.

(see brainstorm: docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md — Phase 6 scope)
(see roadmap: docs/plans/2026-02-24-feat-complete-core-services-roadmap-plan.md — Phase 6 section)

## Problem Statement

The SDK can observe groups but cannot modify them. Users who want to group speakers, add a speaker to an existing group, or dissolve a group must drop to raw UPnP calls. The `GroupManagement` operations (`AddMember`, `RemoveMember`) exist in `sonos-api` but are not exposed through the SDK.

Key challenge: `AddMember` requires a `boot_seq: u32` parameter that is not currently stored anywhere in the SDK state. It's present in ZoneGroupTopology events but gets discarded during processing.

## Proposed Solution

### API Design

Six methods across three structs:

```rust
// Group-centric operations (primary API)
group.add_speaker(&speaker) -> Result<AddMemberResponse, SdkError>
group.remove_speaker(&speaker) -> Result<(), SdkError>
group.dissolve() -> Result<(), SdkError>

// Speaker-centric operations (convenience)
speaker.join_group(&group) -> Result<AddMemberResponse, SdkError>
speaker.leave_group() -> Result<BecomeCoordinatorOfStandaloneGroupResponse, SdkError>

// System-level creation
system.create_group(&coordinator, &[&speakers]) -> Result<(), SdkError>
```

### Design Decisions

1. **Group methods are the primary API** — Group already has `coordinator_ip` and `exec()` for coordinator-targeted operations. Speaker convenience methods delegate to Group.

2. **`speaker.leave_group()` wraps `become_standalone()`** — The underlying UPnP operation already exists on Speaker. `leave_group()` is a semantic alias.

3. **`boot_seq` stored on SpeakerInfo** — Add `boot_seq: u32` field directly to `Speaker` struct in sonos-state (aliased as `SpeakerInfo`). Co-locates all speaker data, avoids a separate `RwLock<HashMap>`, and follows existing patterns. Default to 0 for speakers before first topology event.

4. **No `GroupManagement` properties needed** — GroupManagement events contain operational metadata (transport settings, URIs), not user-facing properties. Group state changes flow through ZoneGroupTopology events, which already work.

5. **Topology auto-update is already done** — After group mutations, ZoneGroupTopology events fire within ~100-500ms and the SDK state updates automatically. No additional work needed.

## Technical Approach

### boot_seq Threading

`boot_seq` exists in the topology event pipeline but is discarded:

```
ZoneGroupMember.boot_seq (sonos-api, events.rs:93)
    ↓ mapped to
ZoneGroupMemberInfo (sonos-api, events.rs:150) — MISSING boot_seq
    ↓ flows through
ZoneGroupTopologyState (sonos-stream)
    ↓ decoded by
decode_topology_event() (sonos-state, decoder.rs:282) — MISSING boot_seq
    ↓ stored as
GroupInfo + GroupMembership — no boot_seq
```

Fix: Add `boot_seq: u32` to `ZoneGroupMemberInfo`, thread it through `decode_topology_event()`, store directly on `SpeakerInfo` (the `Speaker` struct in sonos-state). This co-locates all per-speaker data and avoids a separate lock.

### Cross-IP Execution

`AddMember` and `RemoveMember` must be sent to the **group coordinator's IP**, not the member's. Group already solves this — its `exec()` helper targets `coordinator_ip`. Speaker convenience methods delegate to Group.

### Method Implementations

```rust
// Group
pub fn add_speaker(&self, speaker: &Speaker) -> Result<AddMemberResponse, SdkError> {
    if speaker.id == self.coordinator_id {
        return Err(SdkError::InvalidOperation(
            "Cannot add coordinator to its own group".to_string(),
        ));
    }
    let boot_seq = self.state_manager.get_boot_seq(&speaker.id).unwrap_or(0);
    self.exec(group_management::add_member(
        speaker.id.as_str().to_string(),
        boot_seq,
    ).build())
}

pub fn remove_speaker(&self, speaker: &Speaker) -> Result<(), SdkError> {
    if speaker.id == self.coordinator_id {
        return Err(SdkError::InvalidOperation(
            "Cannot remove coordinator from its own group; use delegate_coordination_to() first".to_string(),
        ));
    }
    self.exec(group_management::remove_member(
        speaker.id.as_str().to_string(),
    ).build())?;
    Ok(())
}

// dissolve() uses remove_speaker() (coordinator-targeted) not become_standalone() (member-targeted)
pub fn dissolve(&self) -> Result<(), SdkError> {
    for member in self.members() {
        if !self.is_coordinator(&member.id) {
            self.remove_speaker(&member)?;
        }
    }
    Ok(())
}

// Speaker
pub fn join_group(&self, group: &Group) -> Result<AddMemberResponse, SdkError> {
    group.add_speaker(self)
}

pub fn leave_group(
    &self,
) -> Result<BecomeCoordinatorOfStandaloneGroupResponse, SdkError> {
    self.become_standalone()
}

// SonosSystem — _api_client stays private (no rename needed)
pub fn create_group(
    &self,
    coordinator: &Speaker,
    members: &[&Speaker],
) -> Result<(), SdkError> {
    let coord_group = self.get_group_for_speaker(&coordinator.id)
        .ok_or_else(|| SdkError::SpeakerNotFound(coordinator.id.as_str().to_string()))?;
    for member in members {
        coord_group.add_speaker(member)?;
    }
    Ok(())
}
```

## Files to Modify

### Layer 0: sonos-api (fix XML injection — BLOCKING pre-requisite)
- `sonos-api/src/services/group_management/operations.rs` — Add `xml_escape()` to `member_id` in `AddMemberOperation.build_payload()` (line 60). Currently unescaped, unlike RemoveMember which properly escapes.

### Layer 1: sonos-api (thread boot_seq)
- `sonos-api/src/services/zone_group_topology/events.rs` — Add `boot_seq` to `ZoneGroupMemberInfo`, populate from `ZoneGroupMember`

### Layer 2: sonos-state (store boot_seq on SpeakerInfo)
- `sonos-state/src/model/speaker.rs` — Add `pub boot_seq: u32` field to `Speaker` struct (default 0)
- `sonos-state/src/state.rs` — Add `get_boot_seq()` and `set_boot_seq()` methods on `StateManager` that read/write the SpeakerInfo's boot_seq field
- `sonos-state/src/decoder.rs` — Update `decode_topology_event()` to extract boot_seq per member, return it in `TopologyChanges`
- `sonos-state/src/event_worker.rs` — Apply boot_seq updates when processing topology changes

### Layer 3: sonos-sdk (group lifecycle methods)
- `sonos-sdk/src/error.rs` — Add `InvalidOperation(String)` variant to `SdkError`
- `sonos-sdk/src/group.rs` — Add `add_speaker()`, `remove_speaker()`, `dissolve()` methods + imports + validation guards
- `sonos-sdk/src/speaker.rs` — Add `join_group()`, `leave_group()` methods
- `sonos-sdk/src/system.rs` — Add `create_group()` method (no `_api_client` rename needed)
- `sonos-sdk/src/lib.rs` — Re-export `AddMemberResponse` from sonos-api

## Tasks

### Task 0: Fix XML injection in AddMember (BLOCKING pre-requisite)

**sonos-api/src/services/group_management/operations.rs:**

- [x] Add `crate::operation::xml_escape(&request.member_id)` in `AddMemberOperation.build_payload()` (line 60) — currently `member_id` is interpolated without escaping, unlike RemoveMember which properly escapes
- [x] Add test: `test_add_member_xml_special_chars()` — verify `<`, `>`, `&` in member_id are escaped in payload

### Task 1: Thread boot_seq through ZoneGroupMemberInfo

**sonos-api/src/services/zone_group_topology/events.rs:**

- [x] Add `pub boot_seq: u32` field to `ZoneGroupMemberInfo` struct
- [x] In the `ZoneGroupMember` → `ZoneGroupMemberInfo` mapping (line ~209), populate `boot_seq` by parsing `member.boot_seq` from `Option<String>` to `u32` (default 0)
- [x] Update all test helpers that construct `ZoneGroupMemberInfo` to include `boot_seq: 0`

### Task 2: Store boot_seq on SpeakerInfo

**sonos-state/src/model/speaker.rs:**

- [x] Add `pub boot_seq: u32` field to `Speaker` struct (default 0)
- [x] Update `Speaker` construction sites (discovery device mapping) to include `boot_seq: 0`

**sonos-state/src/state.rs:**

- [x] Add `pub fn get_boot_seq(&self, speaker_id: &SpeakerId) -> Option<u32>` — reads from SpeakerInfo
- [x] Add `pub(crate) fn set_boot_seq(&self, speaker_id: &SpeakerId, boot_seq: u32)` — updates SpeakerInfo's boot_seq field

### Task 3: Extract boot_seq in topology decoder

**sonos-state/src/decoder.rs:**

- [x] Add `pub boot_seqs: Vec<(SpeakerId, u32)>` field to `TopologyChanges` struct
- [x] In `decode_topology_event()`, extract boot_seq for each member and add to `boot_seqs` vec

**sonos-state/src/event_worker.rs:**

- [x] In `apply_topology_changes()`, iterate over `changes.boot_seqs` and call `state_manager.set_boot_seq()` for each

### Task 4: Add InvalidOperation error variant + group lifecycle methods on Group

**sonos-sdk/src/error.rs:**

- [x] Add `#[error("Invalid operation: {0}")] InvalidOperation(String)` variant to `SdkError`

**sonos-sdk/src/group.rs:**

- [x] Add `use sonos_api::services::group_management` import
- [x] Add `add_speaker(&self, speaker: &Speaker) -> Result<AddMemberResponse, SdkError>` method with coordinator self-add guard
- [x] Add `remove_speaker(&self, speaker: &Speaker) -> Result<(), SdkError>` method with coordinator removal guard
- [x] Add `dissolve(&self) -> Result<(), SdkError>` method — iterates non-coordinator members, calls `self.remove_speaker()` on each (coordinator-targeted, not `become_standalone()`)

### Task 5: Add convenience methods on Speaker

**sonos-sdk/src/speaker.rs:**

- [x] Add `join_group(&self, group: &Group) -> Result<AddMemberResponse, SdkError>` — delegates to `group.add_speaker(self)`
- [x] Add `leave_group(&self) -> Result<BecomeCoordinatorOfStandaloneGroupResponse, SdkError>` — wraps `self.become_standalone()`

### Task 6: Add create_group on SonosSystem

**sonos-sdk/src/system.rs:**

- [x] Add `create_group(&self, coordinator: &Speaker, members: &[&Speaker]) -> Result<(), SdkError>` method
  - Looks up coordinator's current group via `get_group_for_speaker()`
  - Calls `group.add_speaker()` for each member
  - No `_api_client` rename needed — method delegates through Group

### Task 7: Re-export response types

**sonos-sdk/src/lib.rs:**

- [x] Add `pub use sonos_api::services::group_management::AddMemberResponse` to the existing response re-exports block

### Task 8: Tests

**sonos-api/src/services/group_management/operations.rs (tests):**

- [x] Test `add_member` escapes XML special characters in member_id

**sonos-sdk/src/group.rs (tests):**

- [x] Test `add_speaker` method exists and compiles (signature assertion)
- [x] Test `remove_speaker` method exists and compiles
- [x] Test `dissolve` method exists and compiles
- [x] Test `add_speaker` rejects coordinator self-add with `InvalidOperation`
- [x] Test `remove_speaker` rejects coordinator removal with `InvalidOperation`

**sonos-sdk/src/speaker.rs (tests):**

- [x] Test `join_group` method exists and compiles
- [x] Test `leave_group` method exists and compiles

**sonos-sdk/src/system.rs (tests):**

- [x] Test `create_group` method exists and compiles

**sonos-state/src/state.rs (tests):**

- [x] Test `get_boot_seq` returns `None` for unknown speaker
- [x] Test `set_boot_seq` + `get_boot_seq` round-trip
- [x] Test boot_seq defaults to 0 for newly discovered speaker (before topology event)

**sonos-state/src/decoder.rs (tests):**

- [x] Test `decode_topology_event` extracts boot_seq values
- [x] Test boot_seq defaults to 0 when missing from topology XML

### Task 9: Update roadmap plan

- [x] Check off Phase 6 tasks in `docs/plans/2026-02-24-feat-complete-core-services-roadmap-plan.md`

## Edge Cases & Error Handling

### Coordinator self-add
`group.add_speaker(&coordinator)` — trying to add the coordinator to its own group. SDK validates: if `speaker.id == self.coordinator_id`, return `Err(SdkError::InvalidOperation("Cannot add coordinator to its own group"))`. Prevents wasted UPnP call.

### Coordinator removal guard
`group.remove_speaker(&coordinator)` — must not remove the coordinator via RemoveMember. SDK validates: if `speaker.id == self.coordinator_id`, return `Err(SdkError::InvalidOperation(...))`. To change coordinator, use `speaker.delegate_coordination_to()` first.

### Speaker already in group
Calling `group.add_speaker()` on a speaker that's already in another group. Sonos handles this by removing the speaker from its current group first. The topology events will reflect both changes. No SDK-level guard needed.

### Dissolve with single member
`group.dissolve()` on a standalone group (1 member). The loop body never executes since the only member is the coordinator (skipped by `is_coordinator` check). Returns `Ok(())` — this is the correct no-op.

### Stale boot_seq
If `boot_seq` is 0 (no topology event received yet), `AddMember` may fail. In practice, topology events arrive within seconds of discovery, so this window is very small. If it does fail, users can retry after the first topology event. `get_boot_seq()` returns `Option<u32>` — callers use `unwrap_or(0)`.

### Stale Group handle
After group mutations, the `Group` struct's `member_ids` field is stale (it's a snapshot, not a live reference). Users should re-fetch via `system.groups()` or `system.get_group_by_id()` after mutations. Document this behavior.

### Partial failure in create_group
If `create_group()` adds 2 of 5 speakers then fails on the 3rd, a partial group exists. The method returns the error immediately — no automatic rollback. The user can inspect the current group state and decide whether to continue or undo manually. Document this explicitly.

### Concurrent modifications
Multiple clients modifying the same group simultaneously. Sonos serializes operations at the device level. The SDK does not add its own synchronization. Topology events may arrive interleaved but the final state will be consistent.

## Acceptance Criteria

- [ ] AddMember XML payload escapes `member_id` with `xml_escape()` (security fix)
- [ ] `group.add_speaker(&speaker)` sends AddMember to coordinator with correct boot_seq
- [ ] `group.add_speaker(&coordinator)` returns `Err(SdkError::InvalidOperation)` (self-add guard)
- [ ] `group.remove_speaker(&speaker)` sends RemoveMember to coordinator
- [ ] `group.remove_speaker(&coordinator)` returns `Err(SdkError::InvalidOperation)` (removal guard)
- [ ] `group.dissolve()` removes all non-coordinator members via coordinator-targeted `remove_speaker()`
- [ ] `speaker.join_group(&group)` delegates to group.add_speaker
- [ ] `speaker.leave_group()` wraps become_standalone
- [ ] `system.create_group()` adds all members to coordinator's group
- [ ] boot_seq stored on SpeakerInfo, flows from topology events through to AddMember calls
- [ ] `cargo test -p sonos-api` passes (XML escape test)
- [ ] `cargo test -p sonos-sdk` passes
- [ ] `cargo test -p sonos-state` passes
- [ ] `cargo clippy` passes with no warnings

## Verification

```bash
cargo build -p sonos-sdk       # Compiles
cargo test -p sonos-state      # State tests pass
cargo test -p sonos-sdk        # SDK tests pass
cargo clippy -p sonos-sdk      # No warnings
cargo build                    # Full workspace
```

Then live validation: `group.add_speaker()`, `speaker.leave_group()`, `group.dissolve()`, `system.create_group()`.

## Sources & References

### Origin

- **Brainstorm:** [docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md](../brainstorms/2026-02-24-product-roadmap-brainstorm.md) — Key decisions: full group lifecycle, methods on Speaker/Group
- **Roadmap:** [docs/plans/2026-02-24-feat-complete-core-services-roadmap-plan.md](2026-02-24-feat-complete-core-services-roadmap-plan.md) — Phase 6 section

### Internal References

- GroupManagement operations: `sonos-api/src/services/group_management/operations.rs`
- AddMember requires boot_seq: `operations.rs:27` (`pub boot_seq: u32`)
- boot_seq in topology XML: `zone_group_topology/events.rs:93` (`@BootSeq`)
- ZoneGroupMemberInfo (missing boot_seq): `zone_group_topology/events.rs:150`
- Group exec() helper: `sonos-sdk/src/group.rs:206-214`
- become_standalone(): `sonos-sdk/src/speaker.rs:525-529`
- Topology decoder: `sonos-state/src/decoder.rs:282-315`
- Topology event worker: `sonos-state/src/event_worker.rs`
- SonosSystem group methods: `sonos-sdk/src/system.rs:198-251`
