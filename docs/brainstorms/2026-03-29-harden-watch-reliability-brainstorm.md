# Brainstorm: Harden watch() Property Reliability

**Date:** 2026-03-29
**Status:** Draft

## What We're Building

A two-phase approach to make `watch()` deliver every property update reliably:

1. **Property Observer Dashboard** — A live example binary that watches all 13 properties on every discovered speaker, showing real-time values, update counts, timestamps, and staleness indicators. This lets us observe exactly which properties are updating and which are not as we interact with the Sonos app.

2. **Automated Property Validation Tests** — Integration tests that programmatically change every property via the Sonos API and assert that `watch()` delivers the update within a bounded timeout. Covers all services: RenderingControl, AVTransport, GroupRenderingControl, and ZoneGroupTopology.

3. **Targeted Root Cause Fixes** — Based on findings from Steps 1 and 2, fix the specific pipeline issues causing missed events.

## Why This Approach

- **Observe before fixing** — The pipeline has 6 layers with async/sync bridges. Guessing at fixes risks solving symptoms, not causes.
- **Position is a known pain point** — It's time-dependent and may not be included in every AVTransport NOTIFY event. The dashboard will reveal the actual update pattern.
- **Other properties are intermittent** — Need systematic visibility across all 13 properties to identify patterns.
- **Dashboard becomes a permanent dev tool** — Useful for ongoing development, not just this debugging effort.

## Key Decisions

1. **Goal: Never miss events** — Fix root causes in the pipeline rather than adding eventual-consistency safety nets (polling resync). If a property changes on the speaker, `watch()` must deliver it.

2. **All 13 properties covered** — Not just API-controllable ones. CurrentTrack and Position will use creative test approaches (playing known URIs, verifying position advances during playback).

3. **Dashboard as example binary** — `cargo run -p sonos-sdk --example property_observer`. Follows existing patterns (live_dashboard, reactive_dashboard).

4. **Dashboard first, then tests** — Manual observation to understand the landscape, then codify expectations into automated tests.

## Scope

### Properties to Observe and Test

**RenderingControl (Speaker-scoped):**
- Volume (u8, 0-100) — set via API
- Mute (bool) — set via API
- Bass (i8, -10 to +10) — set via API
- Treble (i8, -10 to +10) — set via API
- Loudness (bool) — set via API

**AVTransport (Speaker-scoped):**
- PlaybackState (Playing/Paused/Stopped/Transitioning) — set via Play/Pause/Stop API
- Position ({position_ms, duration_ms}) — verify advances during playback
- CurrentTrack ({title, artist, album, album_art_uri, uri}) — trigger `next()`/`previous()` to change track

**GroupRenderingControl (Group-scoped):**
- GroupVolume (u16, 0-100) — set via API after grouping
- GroupMute (bool) — set via API after grouping
- GroupVolumeChangeable (bool) — event-only, observe during group changes

**ZoneGroupTopology (Speaker/System-scoped):**
- GroupMembership ({group_id, is_coordinator}) — group/ungroup speakers
- Topology ({speakers, groups}) — group/ungroup speakers

### Phase 1: Property Observer Dashboard

**Output format:** Live terminal table refreshed on each event, showing:
| Speaker | Property | Value | Updated At | Count | Latency |
|---------|----------|-------|------------|-------|---------|

Plus a raw event log showing the pipeline layer where each event was processed.

**Behavior:**
- Discovers all speakers automatically
- Watches all 13 properties on every speaker
- Highlights stale properties (no update in configurable timeout)
- Shows update count to detect duplicate or missing events
- Runs indefinitely until Ctrl+C

### Phase 2: Automated Property Validation Tests

**Test structure:** One test per property group, all `#[ignore]` (require real speakers):

- `test_rendering_control_properties` — Set volume/mute/bass/treble/loudness, assert watch delivers each change within 5 seconds
- `test_av_transport_properties` — Play/pause/stop, assert PlaybackState updates; play URI, assert CurrentTrack; verify Position advances
- `test_group_rendering_properties` — Create group, set group volume/mute, assert GroupVolume/GroupMute/GroupVolumeChangeable
- `test_topology_properties` — Group/ungroup speakers, assert GroupMembership and Topology update

Each test reports which specific property failed and at which pipeline layer if possible.

### Phase 3: Root Cause Fixes

Based on findings. Known suspects from code review:

1. **Position not in every AVTransport NOTIFY** — May need supplemental polling or explicit position query after transport state changes
2. **`let _ = event_tx.send(...)` silently drops** — If receiver is gone, events vanish with only a log
3. **Group property race** — GroupRenderingControl events arriving before ZoneGroupTopology establishes group mapping causes silent drops
4. **IP-to-speaker lookup failures** — Events from speakers not in the `ip_to_speaker` map are logged and dropped

## Open Questions

_None — all questions resolved during brainstorming._

## Resolved Questions

1. **Which properties fail?** — Position is notably flaky, others intermittent. Need systematic observation.
2. **Dashboard or tests first?** — Dashboard first for exploration, then automated tests.
3. **Test scope?** — All 13 properties, including ones requiring external interaction.
4. **Reliability guarantee?** — Never miss events (fix root causes, not safety nets).
5. **Dashboard format?** — Example binary in sonos-sdk, following existing patterns.
