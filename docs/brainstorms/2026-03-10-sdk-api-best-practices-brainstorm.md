# SDK API Best Practices Brainstorm

**Date**: 2026-03-10
**Status**: Draft
**Focus**: Developer experience - making sonos-sdk a best-practice Rust crate

## What We're Building

A set of API improvements to make sonos-sdk follow Rust SDK best practices, inspired by the web DOM API's "everything just works" philosophy. The golden rule: **reading is always free, watching is the opt-in upgrade.**

### The Problem

`SonosSystem::new()` eagerly bootstraps the entire event pipeline (SonosEventManager socket binding, callback server thread, StateManager event wiring) even when the user only wants to `play()` or `fetch()` a volume. This violates the Rust SDK convention of cheap constructors and lazy heavy work.

Additionally, method names are verbose (`get_speaker_by_name`) compared to idiomatic Rust SDKs (reqwest, octocrab, rusb), and the public API lacks some standard Rust crate affordances.

### The Vision

```rust
use sonos_sdk::prelude::*;

let sonos = SonosSystem::new()?;

// Direct SOAP calls - no event infrastructure created
let kitchen = sonos.speaker("Kitchen").unwrap();
kitchen.play()?;
let vol = kitchen.volume.fetch()?;

// Groups via on-demand ZGT query - no event infrastructure needed
let groups = sonos.groups();

// ONLY NOW does the event manager lazily initialize
kitchen.volume.watch()?;
for event in sonos.iter() {
    println!("{:?}", event);
}
```

## Why This Approach

Benchmarked against how top Rust crates handle initialization:

| Crate | Constructor cost | When heavy work happens |
|-------|-----------------|------------------------|
| reqwest | Free fn or cheap `Client::new()` | On first `.send()` |
| rusb | Free fn `rusb::devices()` | Immediately (snapshot) |
| stripe | `Client::new(key)` - infallible | On first API call |
| AWS SDK | `Client::new(&config)` - infallible | On first `.send()` |
| octocrab | `octocrab::instance()` - lazy singleton | On first API call |

The universal rule: **constructors are cheap, heavy work is deferred or opt-in.**

## Key Decisions

### 1. Lazy Event Manager via OnceLock

**Decision**: Store `OnceLock<Arc<SonosEventManager>>` in `SonosSystem`. First `watch()` call triggers transparent initialization.

**Rejected alternatives**:
- Explicit `enable_events()` method - adds ceremony, violates "just works"
- Separate `ReactiveSonosSystem` type - too heavy, two types to learn

**Impact**: `SonosSystem::new()` becomes significantly cheaper. Users who never call `watch()` never pay the event infrastructure cost. The StateManager starts in query-only mode and gets the event manager wired in on first watch.

### 2. Shorter Method Names + Fluent Navigation

**Decision**: Follow Rust naming conventions - drop `get_` prefix, use the shortest name for the primary lookup. Replace cross-entity lookup methods with fluent navigation on the entities themselves.

**System-level lookups** (shortened):

| Current | New |
|---------|-----|
| `get_speaker_by_name("Kitchen")` | `speaker("Kitchen")` |
| `get_speaker_by_id(&id)` | `speaker_by_id(&id)` |
| `get_group_by_name("Living Room")` | `group("Living Room")` |
| `get_group_by_id(&id)` | `group_by_id(&id)` |

**Fluent navigation** (replaces cross-entity methods on SonosSystem):

| Current | New |
|---------|-----|
| `system.get_group_for_speaker(&id)` | `speaker.group()` |
| (none) | `group.speaker("Kitchen")` |

This mirrors the DOM: `element.parentNode` instead of `document.getParentOfElement(element)`. Navigation happens on the entity, not the system.

```rust
// Navigate from speaker to its group
let kitchen = sonos.speaker("Kitchen").unwrap();
let group = kitchen.group().unwrap();

// Navigate from group to its speakers
let group = sonos.group("Living Room").unwrap();
let members = group.members();
let specific = group.speaker("Kitchen");  // None if not in this group
```

The Rust API Guidelines say: getters should not use `get_` prefix. The primary lookup method gets the shortest name.

### 3. Prelude Module

**Decision**: Add `sonos_sdk::prelude` re-exporting the most common types.

```rust
// sonos-sdk/src/prelude.rs
pub use crate::{
    SonosSystem, Speaker, Group, SdkError,
    PlayMode, SeekTarget,
    Volume, PlaybackState, Mute, CurrentTrack,
    SpeakerId, GroupId,
};
```

Follows bevy/tokio convention. Power users import specific items; newcomers use `use sonos_sdk::prelude::*`.

### 4. #[non_exhaustive] on SdkError

**Decision**: Mark `SdkError` as `#[non_exhaustive]` to allow adding new error variants in minor versions without breaking downstream code. Standard practice for published Rust crates.

### 5. StateManager Query-Only Mode

**Decision**: `StateManager` should work without an event manager for pure query operations. The `fetch()` method on PropertyHandle makes direct SOAP calls regardless of event manager state. `get()` returns cached values (populated by `fetch()` or events). `watch()` requires the event manager (triggers lazy init).

## Implementation Notes

### Lazy Init Flow

```
SonosSystem::new()
  1. Discovery (cache-first, as today)
  2. Create StateManager WITHOUT event manager
  3. Create SonosClient
  4. Build Speaker handles
  5. Store OnceLock<Arc<SonosEventManager>> (empty)

First watch() call:
  1. OnceLock::get_or_init() creates SonosEventManager
  2. Wire event manager into StateManager
  3. Register devices with event manager
  4. Proceed with watch subscription
```

### Backward Compatibility

- Method renames: deprecate old names with `#[deprecated]` attributes pointing to new names
- Lazy event manager: transparent to users - same API, different timing
- Prelude: additive change, no breakage

## Open Questions

None - all key decisions resolved through brainstorming.
