---
title: watch-after-fetch-event-suppression
date: 2026-03-29
category: logic-errors
tags:
  - property-observer
  - volume
  - event-counting
  - change-detection
  - state-management
  - upnp-notifications
  - debugging
severity: medium
component: sonos-sdk/examples/property_observer.rs
related_components:
  - state-store/PropertyBag
  - sonos-state/event-pipeline
  - sonos-sdk/property/handles
symptom: "Volume property showed count=0 events on all speakers while other RenderingControl properties (Mute, Bass, Treble, Loudness) showed count=1"
---

# fetch() Before watch() Suppresses Initial Events

## Problem

The property observer dashboard (`cargo run --example property_observer`) showed Volume with count=0 events on ALL speakers, while Mute, Bass, Treble, Loudness all showed count=1. Volume was the only property that never received its initial subscription event.

## Investigation

1. Ran `cargo run --example property_observer` against 6 real Sonos speakers for 3+ minutes
2. Observed: Volume count=0 on ALL speakers, Mute/Bass/Treble/Loudness count=1
3. Traced the entire 6-layer event pipeline: callback-server → sonos-stream → sonos-event-manager → sonos-state → sonos-sdk → user
4. Found pipeline code is structurally identical for Volume and Mute — no filtering anywhere
5. Identified that `PropertyBag.set()` returns `false` when value unchanged (change detection working correctly)
6. Found `speaker.volume.fetch()` on line 102 of property_observer.rs used as a reachability probe — this pre-seeds the store
7. The initial UPnP NOTIFY contains the same value → change detection suppresses the event
8. Mute/Bass/Treble/Loudness are never `fetch()`'d before `watch()`, so their NOTIFY values are genuinely new

## Root Cause

The dashboard used `speaker.volume.fetch()` as a reachability probe BEFORE calling `speaker.volume.watch()`. This pre-seeded `Volume(N)` in the PropertyBag. When the UPnP subscription's initial NOTIFY arrived with the same value, `PropertyBag.set()` detected no change (`PartialEq` comparison returned equal) and returned `false`. No ChangeEvent was emitted.

This is not a pipeline bug — the change detection is working as designed. The issue is the usage pattern: `fetch()` before `watch()` creates an implicit suppression of the initial event.

```
fetch() stores Volume(51) in PropertyBag
    ↓
watch() subscribes to RenderingControl
    ↓
Initial NOTIFY arrives: Volume=51
    ↓
PropertyBag.set(Volume(51)) → old == new → returns false
    ↓
No ChangeEvent emitted → count stays at 0
```

## Solution

Replace the `fetch()`-based reachability probe with `watch()` failure detection:

**Before:**
```rust
// Probe reachability
if speaker.volume.fetch().is_err() {
    eprintln!("  Skipping {name} (unreachable)");
    continue;
}
```

**After:**
```rust
// Volume watch serves as reachability probe — skip speaker if it fails
let volume_handle = match speaker.volume.watch() {
    Ok(handle) => handle,
    Err(_) => {
        eprintln!("  Skipping {name} (unreachable)");
        continue;
    }
};
```

## Verification

After the fix, Volume showed count=1 from the initial NOTIFY on all reachable speakers, matching Mute/Bass/Treble/Loudness. The event pipeline delivers all RenderingControl properties uniformly when no prior `fetch()` pre-seeds the store.

## Prevention Guidelines

**The rule:** If you need the initial subscription event, do NOT call `fetch()` before `watch()`.

| Pattern | Safe? | Use when |
|---------|-------|----------|
| `watch()` then `get()` | Yes | You want events AND current value |
| `watch()` only | Yes | You only want events |
| `fetch()` only | Yes | You only want current value, no events |
| `fetch()` then `watch()` | No* | Initial event will be suppressed |

*Acceptable only if you don't care about the initial event (you already have the value from fetch).

## Debugging Tip

`cargo run --example property_observer` is a powerful end-to-end debugging tool for the SDK event pipeline. It watches all 13 properties across every discovered speaker and displays:

- Live property values
- Event counts per property
- Timestamps of last received events
- WatchMode (Events/Polling/CacheOnly)

Use it to verify event delivery when investigating watch() issues. The update count column immediately reveals which properties are receiving events and which are silent.

## Related References

- **Plan:** `docs/plans/2026-03-29-feat-harden-watch-property-reliability-plan.md`
- **Brainstorm:** `docs/brainstorms/2026-03-29-harden-watch-reliability-brainstorm.md`
- **PR #64:** fix(sdk): move EventInitFn to StateManager to fix watch() propagation
- **PR #63:** fix(callback-server): buffer + replay events for unregistered SIDs
- **PR #59:** feat: RAII WatchHandle with 50ms grace period
- **Code:** `state-store/src/store.rs:79` — PropertyBag.set() change detection
- **Code:** `sonos-state/src/event_worker.rs:83-94` — event decoding and PropertyChange application
- **Code:** `sonos-sdk/src/property/handles.rs:466` — PropertyHandle.fetch() stores to PropertyBag
