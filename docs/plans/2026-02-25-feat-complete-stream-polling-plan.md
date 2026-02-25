---
title: "feat: Complete stream polling for all 5 core services"
type: feat
status: active
date: 2026-02-25
origin: docs/plans/2026-02-24-feat-complete-core-services-roadmap-plan.md
---

# Complete Stream Polling for All 5 Core Services

## Overview

Phase 2 of the core services roadmap: replace all stub and partial `ServicePoller` implementations with real polling that reaches **full parity with UPnP event data**.

**Architectural change:** Introduce a canonical `State` type per service in `sonos-api`. Both UPnP event parsing and polling produce this same type. This eliminates 9 duplicate struct definitions across sonos-api/sonos-stream and simplifies the entire event pipeline.

(see parent plan: docs/plans/2026-02-24-feat-complete-core-services-roadmap-plan.md, Phase 2)

## Problem Statement

Three of five pollers are stubs. The two working pollers have significant gaps vs UPnP events. Additionally, 9 struct definitions are duplicated between sonos-api and sonos-stream, with 135 lines of manual conversion logic.

## Architecture

### Before (3 representations per service)

```
sonos-api/events.rs     → XML serde type (AVTransportEvent)
    ↓ manual conversion in sonos-stream/processor.rs
sonos-stream/types.rs   → flat type (AVTransportEvent) ← DUPLICATE
    ↓ consumed by
sonos-state/decoder.rs  → property extraction
```

Polling would add a 4th representation (snapshot type).

### After (1 canonical type per service)

```
sonos-api/state.rs      → AVTransportState (canonical flat type)
    ↑ produced by                ↑ produced by
events.rs → into_state()     state.rs → poll()
    ↓ used directly by
sonos-stream/types.rs   → EventData::AVTransport(sonos_api::AVTransportState)
    ↓ consumed by
sonos-state/decoder.rs  → property extraction (same fields, different import)
```

**One type, two producers, all consumers.** No conversion. No duplication.

### Per-Service File Layout (sonos-api)

```
sonos-api/src/services/av_transport/
├── operations.rs   — Get/Set operations (unchanged)
├── events.rs       — UPnP XML parser, adds into_state() method
├── state.rs        — AVTransportState type + poll() function (NEW)
└── mod.rs          — exports
```

---

## Implementation Plan

### Task 1: Define State Types (sonos-api)

Create `state.rs` in each service module with a flat, serializable state type. These replace both the sonos-stream flat event types AND the proposed snapshot types.

**`sonos-api/src/services/av_transport/state.rs`:**

```rust
use serde::{Serialize, Deserialize};

/// Complete AVTransport service state.
/// Canonical type used by both UPnP event streaming and polling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AVTransportState {
    pub transport_state: Option<String>,
    pub transport_status: Option<String>,
    pub speed: Option<String>,
    pub current_track_uri: Option<String>,
    pub track_duration: Option<String>,
    pub track_metadata: Option<String>,
    pub rel_time: Option<String>,
    pub abs_time: Option<String>,
    pub rel_count: Option<u32>,
    pub abs_count: Option<u32>,
    pub play_mode: Option<String>,
    pub next_track_uri: Option<String>,
    pub next_track_metadata: Option<String>,
    pub queue_length: Option<u32>,
}
```

**`sonos-api/src/services/rendering_control/state.rs`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderingControlState {
    pub master_volume: Option<String>,
    pub master_mute: Option<String>,
    pub lf_volume: Option<String>,
    pub rf_volume: Option<String>,
    pub lf_mute: Option<String>,
    pub rf_mute: Option<String>,
    pub bass: Option<String>,
    pub treble: Option<String>,
    pub loudness: Option<String>,
    pub balance: Option<String>,
    pub other_channels: std::collections::HashMap<String, String>,
}
```

**`sonos-api/src/services/group_rendering_control/state.rs`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupRenderingControlState {
    pub group_volume: Option<u16>,
    pub group_mute: Option<bool>,
    pub group_volume_changeable: Option<bool>,
}
```

**`sonos-api/src/services/zone_group_topology/state.rs`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneGroupTopologyState {
    pub zone_groups: Vec<ZoneGroupInfo>,
    pub vanished_devices: Vec<String>,
}
// Reuses existing public ZoneGroupInfo, ZoneGroupMemberInfo, NetworkInfo, SatelliteInfo from events.rs
```

**`sonos-api/src/services/group_management/state.rs`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroupManagementState {
    pub group_coordinator_is_local: Option<bool>,
    pub local_group_uuid: Option<String>,
    pub reset_volume_after: Option<bool>,
    pub virtual_line_in_group_id: Option<String>,
    pub volume_av_transport_uri: Option<String>,
}
```

**Update each `mod.rs`** to add `pub mod state;` and re-export the state type.

---

### Task 2: Add `into_state()` to Event Types (sonos-api)

Each XML event type gets a method that produces the canonical state type. This replaces the 135-line conversion in `sonos-stream/src/events/processor.rs`.

**`sonos-api/src/services/av_transport/events.rs`:**

```rust
impl AVTransportEvent {
    // ... existing methods ...

    /// Convert parsed UPnP event to canonical state representation.
    pub fn into_state(&self) -> super::state::AVTransportState {
        super::state::AVTransportState {
            transport_state: self.transport_state(),
            transport_status: self.transport_status(),
            speed: self.speed(),
            current_track_uri: self.current_track_uri(),
            track_duration: self.track_duration(),
            track_metadata: self.track_metadata(),
            rel_time: self.rel_time(),
            abs_time: self.abs_time(),
            rel_count: self.rel_count(),
            abs_count: self.abs_count(),
            play_mode: self.play_mode(),
            next_track_uri: self.next_track_uri(),
            next_track_metadata: self.next_track_metadata(),
            queue_length: self.queue_length(),
        }
    }
}
```

Same pattern for all 5 services. Each `into_state()` calls the existing public accessor methods.

---

### Task 3: Add `poll()` Functions (sonos-api)

Each `state.rs` gets a `poll()` function that calls Get operations and returns the state type.

**`sonos-api/src/services/av_transport/state.rs` — `poll()`:**

Calls 4 operations: GetTransportInfo (required) + GetPositionInfo, GetTransportSettings, GetMediaInfo (optional, fallback to None on failure).

```rust
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<AVTransportState> {
    let transport = client.execute_enhanced(ip,
        super::get_transport_info_operation().build()?)?;

    let position = super::get_position_info_operation().build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());
    let settings = super::get_transport_settings_operation().build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());
    let media = super::get_media_info_operation().build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());

    Ok(AVTransportState {
        transport_state: Some(transport.current_transport_state),
        transport_status: Some(transport.current_transport_status),
        speed: Some(transport.current_speed),
        current_track_uri: position.as_ref().map(|p| p.track_uri.clone()),
        track_duration: position.as_ref().map(|p| p.track_duration.clone()),
        track_metadata: position.as_ref().map(|p| p.track_meta_data.clone()),
        rel_time: position.as_ref().map(|p| p.rel_time.clone()),
        abs_time: position.as_ref().map(|p| p.abs_time.clone()),
        rel_count: position.as_ref().map(|p| p.rel_count as u32),
        abs_count: position.as_ref().map(|p| p.abs_count as u32),
        play_mode: settings.map(|s| s.play_mode),
        next_track_uri: media.as_ref().map(|m| m.next_uri.clone()),
        next_track_metadata: media.as_ref().map(|m| m.next_uri_meta_data.clone()),
        queue_length: media.map(|m| m.nr_tracks),
    })
}
```

**`rendering_control/state.rs` — `poll()`:**

Calls 5 operations: GetVolume (required) + GetMute, GetBass, GetTreble, GetLoudness (optional).

```rust
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<RenderingControlState> {
    let volume = client.execute_enhanced(ip,
        super::get_volume_operation("Master".to_string()).build()?)?;
    let mute = super::get_mute_operation("Master".to_string()).build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());
    let bass = super::get_bass_operation().build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());
    let treble = super::get_treble_operation().build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());
    let loudness = super::get_loudness_operation("Master".to_string()).build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());

    Ok(RenderingControlState {
        master_volume: Some(volume.current_volume.to_string()),
        master_mute: mute.map(|m| if m.current_mute { "1" } else { "0" }.to_string()),
        bass: bass.map(|b| b.current_bass.to_string()),
        treble: treble.map(|t| t.current_treble.to_string()),
        loudness: loudness.map(|l| if l.current_loudness { "1" } else { "0" }.to_string()),
        lf_volume: None,   // Not polled — decoder only reads Master
        rf_volume: None,
        lf_mute: None,
        rf_mute: None,
        balance: None,      // No GetBalance operation exists
        other_channels: std::collections::HashMap::new(),
    })
}
```

**`group_rendering_control/state.rs` — `poll()`:**

```rust
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<GroupRenderingControlState> {
    let volume = client.execute_enhanced(ip,
        super::get_group_volume_operation().build()?)?;
    let mute = super::get_group_mute_operation().build()
        .ok().and_then(|op| client.execute_enhanced(ip, op).ok());

    Ok(GroupRenderingControlState {
        group_volume: Some(volume.current_volume),
        group_mute: mute.map(|m| m.current_mute),
        group_volume_changeable: None, // No Get operation exists
    })
}
```

**`zone_group_topology/state.rs` — `poll()`:**

```rust
pub fn poll(client: &SonosClient, ip: &str) -> crate::Result<ZoneGroupTopologyState> {
    let response = client.execute_enhanced(ip,
        super::get_zone_group_state_operation().build()?)?;
    let zone_groups = super::events::parse_zone_group_state_xml(&response.zone_group_state)?;

    Ok(ZoneGroupTopologyState {
        zone_groups,
        vanished_devices: vec![],
    })
}
```

Requires extracting a `parse_zone_group_state_xml()` helper from `ZoneGroupTopologyEvent::zone_groups()` in events.rs (shared by both event path and polling path).

**GroupManagement — no `poll()` function.** Action-only service, no Get operations.

---

### Task 4: Expose Shared ZoneGroupTopology Parser (sonos-api)

**File:** `sonos-api/src/services/zone_group_topology/events.rs`

Extract the XML→ZoneGroupInfo parsing from `ZoneGroupTopologyEvent::zone_groups()` into a public function:

```rust
/// Parse raw ZoneGroupState XML into ZoneGroupInfo structs.
/// Shared by UPnP event processing and polling for parity.
pub fn parse_zone_group_state_xml(raw_xml: &str) -> crate::Result<Vec<ZoneGroupInfo>> {
    let clean_xml = xml_utils::strip_namespaces(raw_xml);
    let state: ZoneGroupState = quick_xml::de::from_str(&clean_xml)
        .map_err(|e| ApiError::ParseError(format!("ZoneGroupState parse error: {}", e)))?;
    Ok(/* ... same mapping as zone_groups() lines 199-228 ... */)
}
```

Then `ZoneGroupTopologyEvent::zone_groups()` calls this function internally (dedup).

---

### Task 5: Update sonos-stream EventData to Use sonos-api State Types

**File:** `sonos-stream/src/events/types.rs`

Replace the duplicate flat structs with references to sonos-api state types:

```rust
pub enum EventData {
    AVTransport(sonos_api::services::av_transport::state::AVTransportState),
    RenderingControl(sonos_api::services::rendering_control::state::RenderingControlState),
    GroupRenderingControl(sonos_api::services::group_rendering_control::state::GroupRenderingControlState),
    ZoneGroupTopology(sonos_api::services::zone_group_topology::state::ZoneGroupTopologyState),
    GroupManagement(sonos_api::services::group_management::state::GroupManagementState),
    DeviceProperties(DevicePropertiesEvent), // Keep — no state type yet
}
```

**Remove from types.rs:**
- `AVTransportEvent` struct (replaced by `sonos_api::AVTransportState`)
- `RenderingControlEvent` struct
- `GroupRenderingControlEvent` struct
- `ZoneGroupTopologyEvent` struct + `ZoneGroupInfo` + `ZoneGroupMemberInfo` + `NetworkInfo` + `SatelliteInfo`
- `GroupManagementEvent` struct

**Keep in types.rs:** `EnrichedEvent`, `EventSource`, `DevicePropertiesEvent` (no state type for this service yet).

**Update `EventData::service_type()`** to match new variant names.

---

### Task 6: Simplify sonos-stream Event Processor

**File:** `sonos-stream/src/events/processor.rs`

The `convert_api_event_data()` function (lines 189-321, 135 lines of manual field mapping) simplifies dramatically:

```rust
fn convert_api_event_data(
    &self, service: &Service, api_event_data: Box<dyn Any + Send + Sync>,
) -> EventProcessingResult<EventData> {
    match service {
        Service::AVTransport => {
            let event = api_event_data.downcast::<sonos_api::services::av_transport::AVTransportEvent>()
                .map_err(|_| EventProcessingError::Parsing("downcast failed".into()))?;
            Ok(EventData::AVTransport(event.into_state()))
        }
        Service::RenderingControl => {
            let event = api_event_data.downcast::<sonos_api::services::rendering_control::RenderingControlEvent>()
                .map_err(|_| EventProcessingError::Parsing("downcast failed".into()))?;
            Ok(EventData::RenderingControl(event.into_state()))
        }
        // ... same pattern for all services
    }
}
```

Each match arm is 4 lines (downcast + `into_state()`). No manual field mapping.

---

### Task 7: Simplify sonos-stream Pollers

**File:** `sonos-stream/src/polling/strategies.rs`

Pollers become thin adapters calling sonos-api `poll()` functions:

```rust
pub struct AVTransportPoller;

#[async_trait]
impl ServicePoller for AVTransportPoller {
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String> {
        let state = sonos_api::services::av_transport::state::poll(client, &pair.speaker_ip.to_string())
            .map_err(|e| PollingError::Network(e.to_string()))?;
        serde_json::to_string(&state)
            .map_err(|e| PollingError::StateParsing(e.to_string()))
    }

    fn service_type(&self) -> Service { Service::AVTransport }
}
```

Same ~8-line pattern for RenderingControl, GroupRenderingControl, ZoneGroupTopology.

**GroupManagement — stable no-op:**

```rust
pub struct GroupManagementPoller;

#[async_trait]
impl ServicePoller for GroupManagementPoller {
    async fn poll_state(&self, _client: &SonosClient, _pair: &SpeakerServicePair) -> PollingResult<String> {
        Ok("{}".to_string()) // No Get ops exist; state via ZoneGroupTopology
    }

    fn service_type(&self) -> Service { Service::GroupManagement }
}
```

**Simplify `ServicePoller` trait** — remove `parse_for_changes()`:

```rust
#[async_trait]
pub trait ServicePoller: Send + Sync {
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String>;
    fn service_type(&self) -> Service;
}
```

**Remove:** `StateChange` enum, all `parse_for_changes()` implementations, all intermediate state structs (`AVTransportState`, `RenderingControlState` — the old sonos-stream ones), `DeviceStatePoller.parse_state_changes()`.

---

### Task 8: Simplify Scheduler Event Emission

**File:** `sonos-stream/src/polling/scheduler.rs`

Replace `change_to_event_data()` (lines 269-341) with `state_to_event_data()`:

```rust
fn state_to_event_data(state_json: &str, service: &Service) -> Option<EventData> {
    match service {
        Service::AVTransport => {
            let s: sonos_api::services::av_transport::state::AVTransportState =
                serde_json::from_str(state_json).ok()?;
            Some(EventData::AVTransport(s))
        }
        Service::RenderingControl => {
            let s: sonos_api::services::rendering_control::state::RenderingControlState =
                serde_json::from_str(state_json).ok()?;
            Some(EventData::RenderingControl(s))
        }
        Service::GroupRenderingControl => {
            let s: sonos_api::services::group_rendering_control::state::GroupRenderingControlState =
                serde_json::from_str(state_json).ok()?;
            Some(EventData::GroupRenderingControl(s))
        }
        Service::ZoneGroupTopology => {
            let s: sonos_api::services::zone_group_topology::state::ZoneGroupTopologyState =
                serde_json::from_str(state_json).ok()?;
            Some(EventData::ZoneGroupTopology(s))
        }
        _ => None, // GroupManagement: no-op
    }
}
```

Each arm: deserialize → wrap in EventData. 3 lines. No field mapping.

**Scheduler polling loop:**
```rust
if new_state != last_state {
    if let Some(event_data) = Self::state_to_event_data(&new_state, &pair.service) {
        let enriched = EnrichedEvent::new(
            reg_id, pair.speaker_ip, pair.service,
            EventSource::PollingDetection { poll_interval: interval },
            event_data,
        );
        event_sender.send(enriched)?;
    }
    last_state = Some(new_state);
}
```

**Remove:** `change_to_event_data()`, the `StateChange` iteration loop.

---

### Task 9: Update sonos-state Decoder Imports

**File:** `sonos-state/src/decoder.rs`

Update imports from sonos-stream types to sonos-api state types:

```rust
// Before:
use sonos_stream::events::{
    AVTransportEvent, EnrichedEvent, EventData, GroupRenderingControlEvent,
    RenderingControlEvent, ZoneGroupTopologyEvent,
};

// After:
use sonos_stream::events::{EnrichedEvent, EventData};
use sonos_api::services::av_transport::state::AVTransportState;
use sonos_api::services::rendering_control::state::RenderingControlState;
use sonos_api::services::group_rendering_control::state::GroupRenderingControlState;
use sonos_api::services::zone_group_topology::state::ZoneGroupTopologyState;
```

**Field access stays the same.** The state types have identical field names to the old sonos-stream types. The decoder match arms update variant names:

```rust
// Before: EventData::AVTransportEvent(event) => decode_av_transport(event)
// After:  EventData::AVTransport(state) => decode_av_transport(state)
```

The `decode_*` functions take the new state types. Field access like `state.transport_state`, `state.master_volume` etc. is unchanged.

**Also update:** `sonos-state/src/lib.rs` and any other files that reference sonos-stream event types.

---

### Task 10: Tests

**sonos-api tests** (in each `state.rs`):

1. `test_av_transport_state_serialization` — round-trip JSON
2. `test_rendering_control_state_serialization` — round-trip JSON
3. `test_group_rendering_control_state_serialization` — round-trip JSON
4. `test_zone_group_topology_state_serialization` — round-trip with zone groups
5. `test_parse_zone_group_state_xml` — shared XML parser with real Sonos samples

**sonos-api tests** (in each `events.rs`):

6. `test_av_transport_event_into_state` — verify UPnP event → state conversion
7. `test_rendering_control_event_into_state` — verify conversion
8. `test_group_rendering_control_event_into_state` — verify conversion
9. `test_zone_group_topology_event_into_state` — verify topology conversion

**sonos-stream tests:**

10. `test_group_management_poller_returns_ok` — verify `Ok("{}")` not error
11. `test_state_to_event_data` — verify JSON → EventData for each service
12. Update `test_device_poller_creation` — remove stub references

**Remove:** `test_av_transport_change_detection`, `test_rendering_control_change_detection`, `test_zone_group_topology_poller_stub`, `test_group_management_poller_stub`, `test_service_poller_types` — these test removed code.

---

### Task 11: Update STATUS.md and Exports

**`docs/STATUS.md`:**
- AVTransport Stream Polling: `Partial [1]` → `Done`
- RenderingControl Stream Polling: `Partial [4]` → `Done`
- GroupRenderingControl Stream Polling: `Stub` → `Done`
- ZoneGroupTopology Stream Polling: `Stub` → `Done`
- GroupManagement Stream Polling: `Stub` → `Done [no-op]`

**`sonos-stream/src/polling/mod.rs`:** Update exports (remove `StateChange`).

**`sonos-stream/src/events/mod.rs`:** Update exports (remove deleted types, re-export from sonos-api if needed for backwards compat).

---

## What Gets Deleted

| Location | What | Lines | Reason |
|---|---|---|---|
| `sonos-stream/events/types.rs` | 5 flat event structs + 4 topology structs | ~230 lines | Replaced by sonos-api state types |
| `sonos-stream/events/processor.rs` | `convert_api_event_data()` field mapping | ~135 lines | Replaced by `into_state()` calls |
| `sonos-stream/polling/strategies.rs` | `StateChange` enum, `parse_for_changes()`, intermediate state structs | ~200 lines | Replaced by sonos-api poll functions |
| `sonos-stream/polling/scheduler.rs` | `change_to_event_data()` | ~75 lines | Replaced by `state_to_event_data()` |
| **Total removed** | | **~640 lines** | |

## What Gets Added

| Location | What | Lines |
|---|---|---|
| `sonos-api/services/*/state.rs` (5 files) | State types + poll functions | ~250 lines |
| `sonos-api/services/*/events.rs` (5 files) | `into_state()` methods | ~75 lines |
| `sonos-api/services/zone_group_topology/events.rs` | `parse_zone_group_state_xml()` helper | ~30 lines |
| `sonos-stream/polling/scheduler.rs` | `state_to_event_data()` | ~30 lines |
| **Total added** | | **~385 lines** |

**Net: ~255 fewer lines** with better architecture.

---

## Parity Matrix

| Service | UPnP Event Fields | Polled Fields | Gap |
|---|---|---|---|
| AVTransport | 14 | 14 | Full parity |
| RenderingControl | 10 | 5 (Master + EQ) | LF/RF: not consumed downstream. Balance: no Get op |
| GroupRenderingControl | 3 | 2 | `group_volume_changeable`: no Get op |
| ZoneGroupTopology | Full topology | Full topology | Full parity |
| GroupManagement | 5 | 0 | Action-only service, no Get ops |

---

## Acceptance Criteria

- [ ] Each service in sonos-api has `state.rs` with canonical State type
- [ ] Each service has `poll()` function (except GroupManagement)
- [ ] Each event type has `into_state()` method
- [ ] `EventData` variants reference sonos-api state types directly
- [ ] Duplicate structs removed from sonos-stream/events/types.rs
- [ ] processor.rs uses `into_state()` (no manual field mapping)
- [ ] strategies.rs pollers are thin adapters (~8 lines each)
- [ ] sonos-state decoder uses sonos-api state types
- [ ] No `UnsupportedService` stubs remain
- [ ] `cargo test` passes across entire workspace
- [ ] `cargo clippy` passes across entire workspace
- [ ] STATUS.md updated

## Dependencies & Prerequisites

- Phase 1 is complete — all RenderingControl Get operations exist
- All other API operations already existed

## Risk Analysis

| Risk | Impact | Mitigation |
|------|--------|------------|
| Cross-crate type changes touch 3 crates | Risk of breaking something | Each crate has tests; run full `cargo test` after each task |
| EventData variant name changes | Breaking change for sonos-state/sonos-sdk | Internal crates only — no public API impact |
| Decoder field access changes | Could break property extraction | Field names are identical — only imports and match arm names change |
| GroupRenderingControl ops fail on non-coordinators | Polling errors | Error propagates as PollingError → scheduler backs off |
| 4 blocking calls per AVTransport poll | Could starve tokio threads | P2 improvement: wrap in `spawn_blocking` |

## Sources & References

### Origin

- **Parent plan:** [docs/plans/2026-02-24-feat-complete-core-services-roadmap-plan.md](2026-02-24-feat-complete-core-services-roadmap-plan.md) — Phase 2
- **Brainstorm:** [docs/brainstorms/2026-02-24-product-roadmap-brainstorm.md](../brainstorms/2026-02-24-product-roadmap-brainstorm.md)

### Internal References

- Current pollers: `sonos-stream/src/polling/strategies.rs:62-353`
- Current event types (to remove): `sonos-stream/src/events/types.rs:74-362`
- Current conversion (to simplify): `sonos-stream/src/events/processor.rs:189-321`
- Scheduler emission: `sonos-stream/src/polling/scheduler.rs:172-341`
- State decoder: `sonos-state/src/decoder.rs:122-245`
- ZoneGroupTopology parser: `sonos-api/src/services/zone_group_topology/events.rs:191-244`
- API operations: `sonos-api/src/services/*/operations.rs`
- Status tracking: `docs/STATUS.md`
