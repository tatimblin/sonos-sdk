# Project Status

Service completion matrix and development roadmap for the Sonos SDK.

**Last updated:** 2026-02-26

## Service Completion Matrix

Tracks each Sonos UPnP service across the 4-layer SDK architecture (6 checkpoints).

**Status values:** Done | Partial _(see footnotes)_ | Stub _(placeholder, returns error)_ | None _(not started)_

### Active Services

| Service | API | Stream Events | Stream Polling | State Decoder | SDK Handles | SDK Fetch |
|---|---|---|---|---|---|---|
| AVTransport | Done | Done | Done | Done | Done | Done |
| RenderingControl | Done | Done | Done | Done | Done | Done |
| GroupRenderingControl | Done | Done | Done | Done | Done | Done |
| ZoneGroupTopology | Done | Done | Done | Done | Partial [8] | Done |
| GroupManagement | Done | Done | Done [11] | None | None | — |
| DeviceProperties | None | Partial [10] | None | None | None | — |

**Footnotes:**

3. ~~Only `GetVolume`, `SetVolume`, `SetRelativeVolume`~~ — All 11 operations now implemented (Get/Set for Volume, Mute, Bass, Treble, Loudness + SetRelativeVolume)
8. `GroupMembership` on Speaker; `Topology` is system-level with no SDK handle
10. `DevicePropertiesEvent` type exists in stream but no `Service` enum variant; uses `ZoneGroupTopology` as fallback in `service_type()`
11. GroupManagement is action-only (no Get operations); poller returns stable empty state so scheduler never emits spurious change events

### Unstarted Services

These services are known from Sonos device descriptions but have no implementation yet. Documentation links are available in [docs/adding-services.md](adding-services.md).

| Service | API | Stream Events | Stream Polling | State Decoder | SDK Handles | SDK Fetch |
|---|---|---|---|---|---|---|
| AlarmClock | None | None | None | None | None | — |
| AudioIn | None | None | None | None | None | — |
| ConnectionManager | None | None | None | None | None | — |
| ContentDirectory | None | None | None | None | None | — |
| HTControl | None | None | None | None | None | — |
| MusicServices | None | None | None | None | None | — |
| Queue | None | None | None | None | None | — |
| SystemProperties | None | None | None | None | None | — |
| VirtualLineIn | None | None | None | None | None | — |

### Column Reference

| Column | Layer | Crate | What It Means |
|---|---|---|---|
| API | Layer 1 | `sonos-api` | `Service` enum variant, operations module, event parsing |
| Stream Events | Layer 2 | `sonos-stream` | `EventData` variant, event processor case |
| Stream Polling | Layer 2 | `sonos-stream` | `ServicePoller` implementation (fallback when firewall blocks UPnP events) |
| State Decoder | Layer 3 | `sonos-state` | Property structs, decoder functions, `PropertyChange` variants |
| SDK Handles | Layer 4 | `sonos-sdk` | Property handles on `Speaker`/`Group` structs |
| SDK Fetch | Layer 4 | `sonos-sdk` | `Fetchable`/`GroupFetchable` trait impl for on-demand reads |

## Development Roadmap

Prioritized by user-facing impact, not by service or layer.

### Tier 1: Users Can't Get Initial Values

Missing `fetch()` on SDK properties means users must wait for an event before reading a value. This is the most impactful gap.

- [x] Add `GetMute`, `GetBass`, `GetTreble`, `GetLoudness` operations to `sonos-api` RenderingControl service
- [x] Add `fetch()` to Mute, Bass, Treble, Loudness SDK handles
- [x] Add `fetch()` to CurrentTrack handle (uses `GetPositionInfo`)
- [x] Add `fetch()` to GroupMembership handle (uses `GetZoneGroupState` via `FetchableWithContext`)

### Tier 2: Incomplete Existing Services

Services that are started but have gaps across layers.

- [x] GroupRenderingControl decoder: extract `GroupMute` and `GroupVolumeChangeable` from events
- [ ] GroupManagement: add state decoder and SDK handles (API and stream layers are done)
- [x] RenderingControl polling: query mute instead of hardcoding `false`
- [x] AVTransport polling: add `GetPositionInfo` call for position/track data

### Tier 3: Reliability Under Firewall

Polling fallbacks that matter when UPnP events are blocked by firewalls.

- [x] ZoneGroupTopology polling strategy (now delegates to sonos-api `poll()`)
- [x] GroupRenderingControl polling strategy (now delegates to sonos-api `poll()`)
- [x] GroupManagement polling strategy (action-only; returns stable empty state)

### Tier 4: New Service Expansion

Adding entirely new services end-to-end using the [4-layer pattern](adding-services.md).

- [ ] DeviceProperties — phantom event type exists in stream, needs API service and full stack
- [ ] Queue — high user value for playlist management
- [ ] ContentDirectory — browse media libraries
- [ ] AlarmClock, MusicServices, AudioIn, HTControl, ConnectionManager, SystemProperties, VirtualLineIn

### Tier 5: Quality and Testing

- [ ] Fix 2 pre-existing test failures in `sonos-stream` iterator tests (runtime-within-runtime panic)
- [ ] Add integration tests for polling fallback paths
