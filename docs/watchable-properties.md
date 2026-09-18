# Watchable Properties

Reference for the properties the SDK exposes, and how reading, fetching and watching each
one behaves.

Regenerate the coverage view from the tree with:

```bash
python .claude/skills/implement-service-sdk/scripts/analyze_handles.py --coverage
```

## Property Reference

Thirteen properties are defined in `sonos-state/src/property.rs`. Twelve are reachable
through a handle on `Speaker` or `Group`; `Topology` is system-scoped and read off
`SonosSystem`.

| Property | Key | Service | Exposed as | `fetch()` | Emits change events |
|---|---|---|---|---|---|
| `Volume` | `volume` | RenderingControl | `speaker.volume` | yes (`GetVolume`) | yes |
| `Mute` | `mute` | RenderingControl | `speaker.mute` | yes (`GetMute`) | yes |
| `Bass` | `bass` | RenderingControl | `speaker.bass` | yes (`GetBass`) | yes |
| `Treble` | `treble` | RenderingControl | `speaker.treble` | yes (`GetTreble`) | yes |
| `Loudness` | `loudness` | RenderingControl | `speaker.loudness` | yes (`GetLoudness`) | yes |
| `PlaybackState` | `playback_state` | AVTransport | `speaker.playback_state` | yes (`GetTransportInfo`) | yes |
| `Position` | `position` | AVTransport | `speaker.position` | yes (`GetPositionInfo`) | yes |
| `CurrentTrack` | `current_track` | AVTransport | `speaker.current_track` | yes (`GetPositionInfo`) | yes |
| `GroupMembership` | `group_membership` | ZoneGroupTopology | `speaker.group_membership` | yes (`GetZoneGroupState`) | yes |
| `GroupVolume` | `group_volume` | GroupRenderingControl | `group.volume` | yes (`GetGroupVolume`) | yes |
| `GroupMute` | `group_mute` | GroupRenderingControl | `group.mute` | yes (`GetGroupMute`) | yes |
| `GroupVolumeChangeable` | `group_volume_changeable` | GroupRenderingControl | `group.volume_changeable` | **no** | yes |
| `Topology` | `topology` | ZoneGroupTopology | `SonosSystem` | — | **no** |

Two rows are the exceptions worth knowing:

- **`GroupVolumeChangeable` has no `fetch()`.** GroupRenderingControl exposes no
  `GetGroupVolumeChangeable`, so the property implements none of the fetch traits. `get()`
  and `watch()` work; its value arrives only from an event. Polling leaves it `None`.
- **`Topology` emits no change events.** It is the one property whose `to_change()` is not
  overridden, because it is written wholesale by `initialize()` rather than per property.
  It updates the store but cannot ride in a `ChangeEvent`, so it is not watchable.

`to_change()` defaults to `None`. A new property that does not override it will store
values correctly and emit nothing — the symptom is a `watch()` that never fires.

## The Three Access Methods

Every handle offers the same three methods.

```rust
use sonos_sdk::prelude::*;
use sonos_sdk::SonosSystem;

let system = SonosSystem::new()?;
let speaker = system.speaker("Living Room").ok_or("no speaker")?;

// Cached read. No network call, no subscription. `None` until something populates it.
let cached = speaker.volume.get();

// Fresh read. Blocking SOAP call, and it updates the cache.
let fresh = speaker.volume.fetch()?;

// Reactive. Registers the watch and opens the UPnP subscription if this is the
// first holder.
let volume = speaker.volume.watch()?;
```

`watch_or_fetch()` is a fourth option: it watches, and additionally does one `fetch()` to
prime the cache so `value()` is populated before the first event arrives.

### Hold the watch handle

`WatchHandle` is `#[must_use]`, and dropping it starts a 50 ms grace period after which
the subscription is torn down. Acquire it **once, outside** any render or event loop:

```rust
// Correct: one handle, held across the whole loop.
let volume = speaker.volume.watch()?;

for event in system.iter() {
    if event.property_key() == "volume" {
        // `value()` re-reads the store on each call, so the handle stays current.
        println!("volume now {:?}", volume.value());
    }
}
```

Re-acquiring inside the loop makes each iteration drop the previous handle, which
schedules a teardown that the next iteration then has to cancel. The 50 ms grace period
exists to absorb exactly that churn, but it is a safety net, not a licence.

`WatchHandle` is a live view, not a snapshot: it holds a read closure, so `value()`
reflects the store at call time. There is no `Deref` — use `value()`, `has_value()`,
`mode()` and `has_realtime_events()`.

### Watch modes

`WatchHandle::mode()` reports how updates are actually arriving:

| Mode | Meaning |
|---|---|
| `Events` | Real-time UPnP NOTIFY callbacks |
| `Polling` | Callbacks are blocked (firewall); the value is polled on an interval |
| `CacheOnly` | Neither is available; the handle reads the store and nothing refreshes it |

## How a Change Reaches You

```
Device NOTIFY
    │
    ▼
callback-server                     routes by SID
    │
    ▼
sonos-stream                        enriches into EnrichedEvent { service, data: EventData }
    │
    ▼
sonos-state decoder                 EventData -> Vec<PropertyChange>
    │
    ▼
sonos-state event worker            writes the store, then, for each change:
    │                               is (speaker_id, property_key) in the watched set?
    ▼
EventFanout                         if watched, clone the ChangeEvent into every
    │                               subscriber's own unbounded mpsc queue
    ▼
manager.iter() / system.iter()      blocks on recv()
```

Three consequences follow from this shape:

- **Storage is unconditional, emission is not.** Values are written whether or not anyone
  is watching, so `get()` works without a `watch()`. Only pairs in the watched set produce
  a `ChangeEvent`, so nothing fans out work nobody asked for.
- **Each `iter()` is independent.** `EventFanout` gives every subscriber its own queue, so
  two loops each see every event rather than splitting the stream, and a slow consumer
  cannot starve a fast one. The cost is that a subscriber which never drains grows its
  queue without bound.
- **Identical values are not re-emitted.** The store reports whether a write actually
  changed the value, and an unchanged write emits nothing.

`ChangeEvent` carries `speaker_id`, `change` (the typed `PropertyChange`, new value
included), `source` and `timestamp`. The property key and service are **methods**, derived
from `change`:

```rust
for event in system.iter() {
    match &event.change {
        PropertyChange::Volume(v) => println!("volume {}", v.value()),
        PropertyChange::Mute(m) => println!("mute {:?}", m),
        _ => {}
    }

    // Also available:
    let _key: &'static str = event.property_key();
    let _service = event.service();
}
```

Matching on `event.change` is preferred over comparing `event.property_key()`: the new
value is already in hand, so no follow-up `get()` is needed, and a burst of queued events
shows every value rather than the latest one repeated.

## Related

- [Project Status](STATUS.md) — per-service coverage across all four layers
- [Adding Services](adding-services.md) — how to add a new property end to end
- `docs/specs/sonos-state.md` — the store, the watched set and `EventFanout` in detail
