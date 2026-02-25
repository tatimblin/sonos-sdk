# Product Roadmap: Complete the 5 Core Services

**Date:** 2026-02-24
**Status:** Brainstorm complete

## What We're Building

A long-running product roadmap to bring 5 core Sonos services (AVTransport, RenderingControl, GroupRenderingControl, GroupManagement, ZoneGroupTopology) to full completion across all 4 SDK layers. The end state is a feature-complete, documented SDK where users can:

- **Stream events** from all 5 services (with polling fallback for all 5)
- **Watch, get, or fetch** any property from any service via the DOM-like API
- **Execute any operation** from any of the 5 services via methods on Speaker/Group
- **Manage groups** with a full lifecycle: create, add/remove speakers, dissolve
- **Observe live group updates** as speakers join and leave

Every aspect of what's needed has a proof of concept, but nothing is fully implemented for every service. This roadmap closes every gap.

## Why This Approach

- **Layer-by-layer structure**: Complete each layer across all 5 services before moving up. This matches the codebase's dependency chain (API → Stream → State → SDK) and the existing Claude skills, which are organized by layer.
- **Bottom-up ordering**: Each layer is stable and tested before the next layer builds on it. No revisiting lower layers when you discover gaps from above.
- **Operations as first-class methods**: `speaker.play()`, `speaker.set_volume(50)`, `group.set_volume(80)` — idiomatic Rust, discoverable, wraps every operation.
- **Polling is must-have**: All 5 services get real polling implementations. Users behind firewalls get degraded but functional experience.
- **Full group lifecycle**: Create, join, leave, dissolve groups through the SDK, not just read topology.
- **Definition of done**: Feature complete + documented. Rustdoc for public APIs, usage examples per service, getting-started guide.

## Key Decisions

1. **Roadmap structure**: Layer-by-layer (not service-by-service or milestone-based)
2. **Operation execution style**: Methods on Speaker/Group (`speaker.play()`, not `speaker.execute::<PlayOperation>()`)
3. **Polling completeness**: Must-have for all 5 services (not nice-to-have)
4. **Group management scope**: Full lifecycle (create, add, remove, dissolve)
5. **Definition of done**: Feature complete + documented (rustdoc, examples, getting-started guide)
6. **Services in scope**: AVTransport, RenderingControl, GroupRenderingControl, GroupManagement, ZoneGroupTopology — no others

## Current State (Gaps by Layer)

### Layer 1: sonos-api (Operations + Events)

| Service | Operations Status | What's Missing |
|---|---|---|
| AVTransport | 30 operations | Nothing — complete |
| RenderingControl | 3 of ~11 operations | GetMute, SetMute, GetBass, SetBass, GetTreble, SetTreble, GetLoudness, SetLoudness |
| GroupRenderingControl | 6 operations | Nothing — complete |
| GroupManagement | 4 operations | Nothing — complete |
| ZoneGroupTopology | 1 operation | Nothing for current needs — complete |

**Events**: All 5 services have event parsing. Complete at this layer.

### Layer 2: sonos-stream (Event Processing + Polling)

| Service | Events | Polling |
|---|---|---|
| AVTransport | Done | Partial — only GetTransportInfo, no GetPositionInfo for position/track |
| RenderingControl | Done | Partial — volume only, mute hardcoded false |
| GroupRenderingControl | Done | Stub — returns UnsupportedService |
| GroupManagement | Done | Stub — returns UnsupportedService |
| ZoneGroupTopology | Done | Stub — returns UnsupportedService |

### Layer 3: sonos-state (Properties + Decoders)

| Service | Properties | Decoder |
|---|---|---|
| AVTransport | PlaybackState, Position, CurrentTrack | Done |
| RenderingControl | Volume, Mute, Bass, Treble, Loudness | Done |
| GroupRenderingControl | GroupVolume only | Partial — GroupMute, GroupVolumeChangeable not decoded |
| GroupManagement | None | Returns empty vec |
| ZoneGroupTopology | GroupMembership, Topology | Done (special path) |

### Layer 4: sonos-sdk (Handles + Fetch + Operations)

| Service | Handles | Fetch | Operation Methods |
|---|---|---|---|
| AVTransport | 3 (playback_state, position, current_track) | 2 of 3 (no CurrentTrack fetch) | None |
| RenderingControl | 5 (volume, mute, bass, treble, loudness) | 1 of 5 (volume only) | None |
| GroupRenderingControl | 1 (group_volume) | 1 of 1 | None |
| GroupManagement | 0 | N/A | None |
| ZoneGroupTopology | 1 (group_membership) | 0 of 1 | None |

**Cross-cutting**: SDK is entirely read-only. No operations exposed. Group management is read-only (topology events work, but can't create/modify groups).

## Roadmap Layers

### Layer 1: Complete API Operations

Close the gaps in sonos-api. Only RenderingControl has missing operations.

- Add GetMute, SetMute operations
- Add GetBass, SetBass operations
- Add GetTreble, SetTreble operations
- Add GetLoudness, SetLoudness operations
- Tests for all new operations

### Layer 2: Complete Stream Polling

Make every service's polling strategy real (not stubbed).

- AVTransport: add GetPositionInfo call for position/track data in poller
- RenderingControl: add GetMute query instead of hardcoding false; add Bass, Treble, Loudness queries if available via polling
- GroupRenderingControl: implement real poller (GetGroupVolume, GetGroupMute)
- GroupManagement: implement real poller or determine appropriate polling strategy
- ZoneGroupTopology: implement real poller (GetZoneGroupState)

### Layer 3: Complete State Layer

Ensure every event type decodes all its properties.

- GroupRenderingControl decoder: extract GroupMute, GroupVolumeChangeable
- GroupManagement: define properties, implement decoder
- Ensure all property types are exported

### Layer 4: Complete SDK — Properties

Close all fetch() gaps and add missing handles.

- Add fetch() to: Mute, Bass, Treble, Loudness (requires Layer 1 operations)
- Add fetch() to: CurrentTrack (use GetPositionInfo)
- Add fetch() to: GroupMembership (use GetZoneGroupState)
- Add GroupMute handle and any GroupManagement handles
- Add Topology handle if appropriate for system-level access

### Layer 5: SDK — Operation Methods

Add execution methods to Speaker and Group structs.

- AVTransport methods on Speaker: play(), pause(), stop(), next(), previous(), seek(), set_av_transport_uri(), etc.
- RenderingControl methods on Speaker: set_volume(), set_mute(), set_bass(), set_treble(), set_loudness(), set_relative_volume()
- GroupRenderingControl methods on Group: set_group_volume(), set_group_mute(), snapshot_group_volume()
- GroupManagement methods on Group: add_member(), remove_member()
- ZoneGroupTopology: any needed methods for group creation/dissolution

### Layer 6: Group Lifecycle

Full group management through the SDK.

- Create group from speakers
- Add speaker to existing group
- Remove speaker from group
- Dissolve group (all speakers become standalone)
- Live topology updates automatically reflect group changes in SDK state

### Layer 7: Documentation

- Rustdoc for all public APIs (sonos-api, sonos-discovery, sonos-state, sonos-sdk)
- Usage examples per service
- Getting-started guide
- Update all SPEC files to reflect final state
- Update STATUS.md to reflect completion

## Open Questions

_None — all questions resolved during brainstorming._

## Next Steps

Create a detailed implementation plan from this roadmap with specific tasks per layer.
