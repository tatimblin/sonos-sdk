# Project Visibility: Status Matrix & Impact Roadmap

**Date:** 2026-02-24
**Status:** Brainstorm complete

## What We're Building

Two documents to make project status visible at a glance:

1. **`docs/STATUS.md`** — A living status matrix tracking every known Sonos service across all 4 SDK layers (API, Stream, State, SDK), plus sub-columns for granular completion (polling, decoder, fetch, etc.)

2. **Impact-prioritized roadmap** (section within STATUS.md) — Incomplete work ranked by user-facing value, not by service or layer.

Both documents will be referenced from CLAUDE.md and AGENTS.md so AI agents always check status before working on any service.

## Why This Approach

- **The problem**: 4-layer architecture with 17+ possible services means ~68+ completion checkpoints. No single view exists to see what's done, partial, or missing. Easy to get lost.
- **Status matrix**: A markdown table is simple, readable in GitHub, and doesn't require tooling. All 17+ known services get a row, even unstarted ones — this shows the full scope of what's possible.
- **Impact-prioritized roadmap**: Organizing by user-facing value (not by service or layer) ensures work that matters most gets done first. Example: missing `fetch()` implementations block users from getting initial property values — that's higher priority than polling stubs that only matter when firewalls block events.
- **AI references**: Adding a pointer in CLAUDE.md and AGENTS.md means any AI agent will check the status matrix before starting work, reducing wasted effort and accidental duplication.

## Key Decisions

1. **Location**: `docs/STATUS.md` — standalone file, not embedded in CLAUDE.md or adding-services.md
2. **Scope**: All 17+ known Sonos services, not just the 6 already started
3. **Roadmap organization**: By impact (user-facing value), not by service or layer
4. **AI awareness**: CLAUDE.md and AGENTS.md will reference STATUS.md
5. **Maintenance**: Manual updates — no auto-generation script (keeps it simple, YAGNI)

## Status Matrix Design

### Columns

The matrix tracks 6 completion checkpoints per service, matching the 4-layer architecture:

| Column | Layer | What It Means |
|--------|-------|---------------|
| API | sonos-api | Service enum variant, operations module, event parsing |
| Stream Events | sonos-stream | EventData variant, event processor case |
| Stream Polling | sonos-stream | ServicePoller implementation (fallback for firewalls) |
| State Decoder | sonos-state | Property structs, decoder functions, PropertyChange variants |
| SDK Handles | sonos-sdk | Property handles on Speaker/Group structs |
| SDK Fetch | sonos-sdk | Fetchable trait impl for on-demand property reads |

### Status Values

- **Done** — Fully implemented and tested
- **Partial** — Implemented but incomplete (with notes on what's missing)
- **Stub** — Placeholder exists but returns error/empty
- **None** — Not started
- Blank — Not applicable or not yet planned

### Services to Track

**Started (6):**
- AVTransport
- RenderingControl
- GroupRenderingControl
- ZoneGroupTopology
- GroupManagement
- DeviceProperties

**Unstarted (12+):**
- AlarmClock
- AudioIn
- ConnectionManager
- ContentDirectory
- HTControl
- MusicServices
- Queue
- SystemProperties
- VirtualLineIn
- GroupManagement (additional operations)
- Plus any others discovered from device descriptions

## Roadmap Priorities (By Impact)

### Tier 1: Users Can't Get Initial Values
Missing `fetch()` on SDK properties means users must wait for an event before reading a value. This is the most impactful gap.

- Add `fetch()` to Mute, Bass, Treble, Loudness handles (RenderingControl Get operations exist)
- Add `fetch()` to CurrentTrack handle (GetPositionInfo exists)
- Add `fetch()` to GroupVolume handle (GroupRenderingControl GetGroupVolume exists)
- Add `fetch()` to GroupMembership handle (ZoneGroupTopology GetZoneGroupState exists)

### Tier 2: Incomplete Existing Services
Services that are started but have gaps across layers.

- GroupRenderingControl decoder: add GroupMute, GroupVolumeChangeable extraction
- GroupManagement: add state decoder and SDK handles (API and stream are done)
- RenderingControl polling: currently hardcodes mute to false
- AVTransport polling: missing GetPositionInfo for position/track data

### Tier 3: Reliability Under Firewall
Polling fallbacks matter when UPnP events are blocked.

- ZoneGroupTopology polling strategy (currently stub)
- GroupRenderingControl polling strategy (currently stub)
- GroupManagement polling strategy (currently stub)

### Tier 4: New Service Expansion
Adding entirely new services end-to-end.

- DeviceProperties (phantom type exists in stream, nothing else)
- Queue (high user value for playlist management)
- ContentDirectory (browse media libraries)
- AlarmClock, MusicServices, etc.

### Tier 5: Quality & Testing
- Fix 2 pre-existing test failures in sonos-stream iterator tests
- Add integration tests for polling fallback paths

## Open Questions

_None — all questions resolved during brainstorming._

## Next Steps

Create `docs/STATUS.md` with the matrix and roadmap, then add references to CLAUDE.md and AGENTS.md.
