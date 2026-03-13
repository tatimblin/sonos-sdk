---
title: "refactor: SDK API best practices — lazy events, fluent navigation, ergonomic naming"
type: refactor
status: completed
date: 2026-03-11
origin: docs/brainstorms/2026-03-10-sdk-api-best-practices-brainstorm.md
---

# refactor: SDK API best practices — lazy events, fluent navigation, ergonomic naming

## Overview

Make `sonos-sdk` follow Rust SDK best practices: cheap constructor, lazy heavy work, short method names, fluent entity navigation, and standard crate affordances. The guiding principle is the web DOM API — **reading is always free, watching is the opt-in upgrade.**

(see brainstorm: docs/brainstorms/2026-03-10-sdk-api-best-practices-brainstorm.md)

## Problem Statement / Motivation

`SonosSystem::new()` eagerly bootstraps the entire event pipeline (SonosEventManager socket binding, tokio runtime, callback server thread, event worker thread) even when the user only wants to `play()` or `fetch()`. This violates the universal Rust SDK convention of cheap constructors (reqwest, stripe, AWS SDK, octocrab).

Additionally:
- Method names are verbose (`get_speaker_by_name`) vs idiomatic Rust (`speaker()`)
- Cross-entity navigation requires the system object (`system.get_group_for_speaker(&id)`) instead of fluent patterns (`speaker.group()`)
- No prelude module for common imports
- `SdkError` is exhaustive, preventing non-breaking error variant additions

## Proposed Solution

Five coordinated changes across two crates (`sonos-sdk`, `sonos-state`), implemented in four phases:

1. **Lazy event manager** — defer `SonosEventManager` creation to first `watch()` call
2. **Shorter method names** — drop `get_` prefix per Rust API Guidelines
3. **Fluent navigation** — `speaker.group()` and `group.speaker("name")` replace system-level cross-entity lookups
4. **Prelude module** — `use sonos_sdk::prelude::*` for common imports
5. **`#[non_exhaustive]` on `SdkError`** — standard extensibility practice

### Target API

```rust
use sonos_sdk::prelude::*;

let sonos = SonosSystem::new()?;                // Cheap: discovery + API client only

let kitchen = sonos.speaker("Kitchen").unwrap(); // Short name
kitchen.play()?;                                 // Direct SOAP call, no event infra
let vol = kitchen.volume.fetch()?;               // Direct SOAP call

let group = kitchen.group().unwrap();            // Fluent: speaker -> group
let members = group.members();                   // Already exists
let specific = group.speaker("Kitchen");         // Fluent: group -> speaker

kitchen.volume.watch()?;                         // NOW event manager lazily initializes
for event in sonos.iter() { /* ... */ }
```

## Technical Considerations

### Architecture: Lazy Event Manager

**Current eager initialization chain** (`system.rs:130-158`):
```
SonosEventManager::new()           → tokio runtime + network socket + callback server
StateManager::builder()
  .with_event_manager(em).build()  → event worker thread
state_manager.add_devices()        → registers with event manager
```

**Proposed lazy initialization**:

`SonosSystem::new()` creates StateManager without event manager (query-only mode). The event manager is stored in a `Mutex<Option<Arc<SonosEventManager>>>` on SonosSystem. First `watch()` call triggers initialization.

**Why `Mutex<Option<...>>` instead of `OnceLock`**: MSRV is 1.80; `OnceLock::get_or_try_init` requires 1.82. `SonosEventManager::new()` is fallible (network socket binding). `Mutex<Option<...>>` supports retry on transient failures. If MSRV is bumped to 1.82+ later, this can be simplified to `OnceLock` with `get_or_try_init`.

**StateManager changes**: The `event_manager` field (`sonos-state/src/state.rs:256`) changes from `Option<Arc<SonosEventManager>>` to `OnceLock<Arc<SonosEventManager>>`. New `set_event_manager(&self, em: Arc<SonosEventManager>)` method added. The event worker thread (`_worker` field) changes from `Option<JoinHandle<()>>` to `Mutex<Option<JoinHandle<()>>>` to support lazy spawning.

**Device registration replay**: When the event manager is lazily created, all devices already in the StateManager's `ip_to_speaker` map must be registered with it. The lazy init code retrieves `speaker_infos()` and calls `em.add_devices()`.

### Architecture: Fluent Navigation

**`speaker.group()`**: Topology must be managed through sonos-state's reactive layer — not a separate one-shot cache bypass. External controllers (iPhone Sonos app, other SDKs) can change group membership at any time. A one-shot `ensure_topology()` with a "already loaded" guard would go stale permanently.

**How it should work**: Topology is essential system state — not an opt-in property. ZoneGroupTopology should be automatically subscribed whenever the event manager initializes, keeping topology fresh from events (including changes from external controllers like the iPhone Sonos app).

**Auto-subscription rule**: Any action that triggers event manager lazy init also subscribes to ZoneGroupTopology:
- `speaker.volume.watch()` → event init → auto-subscribe ZGT + requested property
- `system.groups()` → event init → auto-subscribe ZGT + fetch initial topology via SOAP → return groups

After either of these calls, `speaker.group()` reads from the state store and returns live data kept fresh by the event stream.

**`speaker.group()` itself does NOT trigger event init** — it reads from the state store. If topology hasn't been loaded (no `system.groups()` call, no `watch()` active), it returns `None`. This avoids hidden network calls from a getter and keeps the pattern consistent: getters read, explicit calls fetch.

**Implementation**: When the lazy event init closure fires (see Phase 1b), after creating the event manager and wiring it into StateManager, also subscribe to ZoneGroupTopology on all known speakers:
```rust
// Inside the event init closure, after set_event_manager:
for speaker_ip in sm.all_speaker_ips() {
    let _ = em.ensure_service_subscribed(speaker_ip, Service::ZoneGroupTopology);
}
```

`system.groups()` triggers event init (via the same lazy init mechanism), then fetches initial topology via the existing `ensure_topology()` SOAP call to seed the state store. From that point forward, events keep it fresh.

**`group.speaker("name")`**: Iterates `member_ids`, looks up `SpeakerInfo` by ID in the state manager, matches by name. Returns `Option<Speaker>`. Case-sensitive, matching existing `speaker()` behavior.

### Performance implications

- `SonosSystem::new()` becomes ~10x faster (no tokio runtime, no socket bind, no threads)
- Users who never call `watch()` never pay event infrastructure cost
- First `watch()` has a one-time initialization cost (~50-100ms for socket bind + thread spawn)
- `speaker.group()` triggers a one-shot topology fetch on first call (~200ms SOAP call), then cached

### Security considerations

None — all changes are internal refactoring with no new network surface.

## System-Wide Impact

- **Interaction graph**: `PropertyHandle::watch()` → check Mutex → create `SonosEventManager` → wire into `StateManager` → spawn event worker → create UPnP subscription. All existing downstream behavior unchanged.
- **Error propagation**: Lazy init failure propagates as `SdkError::EventManager(String)` from `watch()`. This is a new failure mode for `watch()` — previously it could only fail from the subscription step.
- **State lifecycle risks**: Between `SonosSystem::new()` and first `watch()`, the system operates in query-only mode. `get()` returns `None` (no cache populated), `fetch()` works (direct SOAP). After first `watch()`, events flow and `get()` returns cached values. No inconsistent state possible.
- **API surface parity**: Deprecated methods delegate to new implementations. `get_group_for_speaker` on SonosSystem deprecated in favor of `speaker.group()`.

## Acceptance Criteria

### Phase 1: Lazy Event Manager
- [ ] `SonosSystem::new()` does NOT create `SonosEventManager` or spawn event threads — `sonos-sdk/src/system.rs`
- [ ] `StateManager` supports `set_event_manager()` post-construction — `sonos-state/src/state.rs`
- [ ] `PropertyHandle::watch()` triggers lazy event manager creation — `sonos-sdk/src/system.rs`, `sonos-sdk/src/property/handles.rs`
- [ ] Device registration replayed during lazy init — `sonos-sdk/src/system.rs`
- [ ] Event worker thread spawned during lazy init — `sonos-state/src/state.rs`
- [ ] `fetch()` works without event manager — already true, verify with test
- [ ] `get()` returns `None` before any `fetch()` or event — already true, verify with test
- [ ] Concurrent `watch()` calls handled safely (only one init) — `sonos-sdk/src/system.rs`
- [ ] Failed lazy init returns `Err(SdkError::EventManager(...))` — `sonos-sdk/src/system.rs`
- [ ] Subsequent `watch()` after failed init retries initialization — `sonos-sdk/src/system.rs`
- [ ] Event manager init auto-subscribes ZoneGroupTopology on all speakers — `sonos-sdk/src/system.rs`

### Phase 2: Method Renames + Fluent Navigation
- [ ] `speaker("name")` added to `SonosSystem` — `sonos-sdk/src/system.rs`
- [ ] `speaker_by_id(&id)` added to `SonosSystem` — `sonos-sdk/src/system.rs`
- [ ] `group("name")` added to `SonosSystem` — `sonos-sdk/src/system.rs`
- [ ] `group_by_id(&id)` added to `SonosSystem` — `sonos-sdk/src/system.rs`
- [ ] Old method names deprecated with `#[deprecated(since = "...", note = "...")]` — `sonos-sdk/src/system.rs`
- [ ] `speaker.group()` returns `Option<Group>` from state store (no hidden network call) — `sonos-sdk/src/speaker.rs`
- [ ] `group.speaker("name")` returns `Option<Speaker>` — `sonos-sdk/src/group.rs`
- [ ] `system.get_group_for_speaker()` deprecated — `sonos-sdk/src/system.rs`
- [ ] `system.groups()` triggers lazy event init + ZGT subscription + initial topology fetch — `sonos-sdk/src/system.rs`
- [ ] ZoneGroupTopology auto-subscribed on event manager init (alongside any explicit watch) — `sonos-sdk/src/system.rs`
- [ ] External topology changes (iPhone app regrouping) reflected in state store when events flowing
- [ ] Existing tests updated for new method names

### Phase 3: Prelude + Error Polish
- [ ] `sonos_sdk::prelude` module created — `sonos-sdk/src/prelude.rs`, `sonos-sdk/src/lib.rs`
- [ ] Prelude includes: `SonosSystem`, `Speaker`, `Group`, `GroupChangeResult`, `SdkError`, `PlayMode`, `SeekTarget`, `Volume`, `PlaybackState`, `Mute`, `CurrentTrack`, `SpeakerId`, `GroupId`, `WatchStatus`, `WatchMode`
- [ ] `#[non_exhaustive]` added to `SdkError` — `sonos-sdk/src/error.rs`
- [ ] Existing tests with exhaustive `match` on `SdkError` updated with wildcard arm

### Phase 4: Documentation + Cleanup
- [ ] `docs/specs/sonos-sdk.md` updated with new API surface
- [ ] `docs/STATUS.md` updated
- [ ] Crate-level doc examples in `lib.rs` updated to use new API
- [ ] Speaker/Group/SonosSystem rustdoc updated

## Implementation Phases

### Phase 1: Lazy Event Manager (core architectural change)

This is the foundational change — everything else can be done independently, but this must be correct first.

#### 1a. StateManager: support post-construction event manager wiring

**File: `sonos-state/src/state.rs`**

Changes to `StateManager` struct (line 241):
- `event_manager: Option<Arc<SonosEventManager>>` → `event_manager: OnceLock<Arc<SonosEventManager>>`
- `_worker: Option<JoinHandle<()>>` → `_worker: Mutex<Option<JoinHandle<()>>>`

New method:
```rust
/// Wire an event manager into this StateManager after construction.
///
/// Spawns the event worker thread and registers all known devices.
/// Can only be called once — subsequent calls are no-ops.
pub fn set_event_manager(&self, em: Arc<SonosEventManager>) -> Result<()> {
    if self.event_manager.set(Arc::clone(&em)).is_err() {
        return Ok(()); // Already set — no-op
    }

    // Register all known devices with the event manager
    let devices = self.registered_devices(); // need to expose this
    em.add_devices(devices)?;

    // Spawn event worker thread
    let worker = spawn_state_event_worker(
        em, self.store.clone(), self.watched.clone(),
        self.event_tx.clone(), self.ip_to_speaker.clone(),
    );
    *self._worker.lock().unwrap() = Some(worker);

    Ok(())
}
```

Update `event_manager()` accessor (line 608):
```rust
pub fn event_manager(&self) -> Option<&Arc<SonosEventManager>> {
    self.event_manager.get()  // OnceLock::get() returns Option<&T>
}
```

Update `StateManagerBuilder::build()` (line 670+):
- If `event_manager` is `Some`, call `set_event_manager()` after construction
- If `None`, leave the `OnceLock` empty

Update all internal references from `&self.event_manager` / `self.event_manager.as_ref()` to `self.event_manager.get()`.

#### 1b. SonosSystem: lazy event manager with init closure

**File: `sonos-sdk/src/system.rs`**

Change `_event_manager` field:
```rust
event_manager: Mutex<Option<Arc<SonosEventManager>>>,
```

`Mutex<Option<...>>` (not `OnceLock`) because: (a) `SonosEventManager::new()` is fallible, (b) retry on failure is desired, (c) MSRV 1.80 lacks `OnceLock::get_or_try_init`.

**File: `sonos-sdk/src/property/handles.rs`**

Add an init closure to `SpeakerContext` so `PropertyHandle::watch()` can trigger lazy event manager creation without referencing `SonosSystem`:

```rust
pub struct SpeakerContext {
    pub(crate) speaker_id: SpeakerId,
    pub(crate) speaker_ip: IpAddr,
    pub(crate) state_manager: Arc<StateManager>,
    pub(crate) api_client: SonosClient,
    pub(crate) event_init: Option<Arc<dyn Fn() -> Result<(), SdkError> + Send + Sync>>,
}
```

**Construction ordering**: The init closure captures `Arc`s to the Mutex and StateManager. These must be created *before* `build_speakers()` so the closure can be passed into each `SpeakerContext`. Then the struct is assembled from the same `Arc`s:

```rust
fn from_devices_inner(devices: Vec<Device>) -> Result<Self, SdkError> {
    // 1. Create shared state FIRST
    let state_manager = Arc::new(StateManager::new().map_err(SdkError::StateError)?);
    state_manager.add_devices(devices.clone()).map_err(SdkError::StateError)?;
    let api_client = SonosClient::new();
    let event_manager = Arc::new(Mutex::new(None));

    // 2. Build init closure from the shared Arcs
    let init_fn = {
        let em_mutex = Arc::clone(&event_manager);
        let sm = Arc::clone(&state_manager);
        Arc::new(move || {
            let mut guard = em_mutex.lock().map_err(|_| SdkError::LockPoisoned)?;
            if guard.is_some() { return Ok(()); }
            let em = Arc::new(
                SonosEventManager::new()
                    .map_err(|e| SdkError::EventManager(e.to_string()))?
            );
            sm.set_event_manager(Arc::clone(&em)).map_err(SdkError::StateError)?;
            *guard = Some(em);
            Ok(())
        }) as Arc<dyn Fn() -> Result<(), SdkError> + Send + Sync>
    };

    // 3. Build speakers WITH the init closure
    let speakers = Self::build_speakers_with_init(
        &devices, &state_manager, &api_client, Some(init_fn),
    )?;

    // 4. Assemble struct from the SAME Arcs
    Ok(Self {
        state_manager,
        event_manager: Arc::try_unwrap(event_manager)
            .unwrap_or_else(|arc| Mutex::new(arc.lock().unwrap().clone())),
        api_client,
        speakers: RwLock::new(speakers),
        last_rediscovery: AtomicU64::new(0),
    })
}
```

Note: `build_speakers_with_init` is a renamed `build_speakers` that accepts an optional init closure to pass through to `SpeakerContext::new()`.

In `PropertyHandle::watch()`:
```rust
pub fn watch(&self) -> Result<WatchStatus<P>, SdkError> {
    self.context.state_manager.register_watch(&self.context.speaker_id, P::KEY);

    // Trigger lazy event manager init if needed
    if self.context.state_manager.event_manager().is_none() {
        if let Some(ref init) = self.context.event_init {
            init()?;
        }
    }

    let mode = if let Some(em) = self.context.state_manager.event_manager() {
        // ... existing subscription logic
    } else {
        WatchMode::CacheOnly
    };

    Ok(WatchStatus::new(self.get(), mode))
}
```

**Test mode** (`with_speakers()`): Pass `event_init: None`. `watch()` returns `CacheOnly` — same as current behavior.

**`iter()` before `watch()` note**: If a user calls `system.iter()` before any `watch()`, the iterator blocks forever (no events, no shutdown signal). This is existing behavior but becomes a more likely pitfall with lazy init. Document this in the `iter()` rustdoc.

### Phase 2: Method Renames + Fluent Navigation

#### 2a. System-level method renames

**File: `sonos-sdk/src/system.rs`**

Add new methods, deprecate old ones:

```rust
// New primary methods
pub fn speaker(&self, name: &str) -> Option<Speaker> { ... }
pub fn speaker_by_id(&self, speaker_id: &SpeakerId) -> Option<Speaker> { ... }
pub fn group(&self, name: &str) -> Option<Group> { ... }
pub fn group_by_id(&self, group_id: &GroupId) -> Option<Group> { ... }

// Deprecated old names — delegate to new methods
#[deprecated(since = "0.2.0", note = "renamed to `speaker()`")]
pub fn get_speaker_by_name(&self, name: &str) -> Option<Speaker> { self.speaker(name) }

#[deprecated(since = "0.2.0", note = "renamed to `speaker_by_id()`")]
pub fn get_speaker_by_id(&self, speaker_id: &SpeakerId) -> Option<Speaker> { self.speaker_by_id(speaker_id) }

#[deprecated(since = "0.2.0", note = "use `speaker.group()` or `group()` instead")]
pub fn get_group_for_speaker(&self, speaker_id: &SpeakerId) -> Option<Group> { ... }

#[deprecated(since = "0.2.0", note = "renamed to `group()`")]
pub fn get_group_by_name(&self, name: &str) -> Option<Group> { self.group(name) }

#[deprecated(since = "0.2.0", note = "renamed to `group_by_id()`")]
pub fn get_group_by_id(&self, group_id: &GroupId) -> Option<Group> { self.group_by_id(group_id) }
```

#### 2b. Speaker.group() — fluent navigation

**File: `sonos-sdk/src/speaker.rs`**

`speaker.group()` reads from the state store. Returns `None` if topology hasn't been loaded yet. No hidden network calls.

Once `system.groups()` or any `watch()` call has been made, topology is live — ZGT events (including from external controllers) keep it fresh.

```rust
/// Get the group this speaker belongs to (sync)
///
/// Reads from the state store's topology data. Returns `None` if topology
/// has not been loaded yet. To load topology:
/// - Call `system.groups()` (fetches topology + starts event subscription)
/// - Or call `watch()` on any property (starts events, auto-subscribes ZGT)
///
/// Once events are flowing, topology stays fresh — including changes from
/// external controllers like the Sonos mobile app.
///
/// # Example
///
/// ```rust,ignore
/// let groups = system.groups(); // seeds topology + starts ZGT events
/// let kitchen = system.speaker("Kitchen").unwrap();
/// if let Some(group) = kitchen.group() {
///     println!("In group with {} speakers", group.member_count());
/// }
/// ```
pub fn group(&self) -> Option<Group> {
    let info = self.context.state_manager.get_group_for_speaker(&self.id)?;
    Group::from_info(
        info,
        Arc::clone(&self.context.state_manager),
        self.context.api_client.clone(),
    )
}
```

#### 2c. system.groups() triggers event init

**File: `sonos-sdk/src/system.rs`**

`system.groups()` now triggers lazy event init (if not already initialized) before fetching topology. This ensures ZGT events flow from this point forward:

```rust
pub fn groups(&self) -> Vec<Group> {
    // Trigger event init + ZGT auto-subscription
    if let Err(e) = self.ensure_event_manager_and_topology() {
        tracing::warn!("Failed to initialize events for groups: {}", e);
    }
    self.ensure_topology(); // existing: seeds state store via SOAP if empty
    // ... existing group construction from state store
}
```

`SonosSystem::ensure_topology()` remains unchanged — it handles the one-shot ZGT SOAP fetch for initial seeding. After that, the ZGT event subscription keeps topology fresh.

#### 2d. Group.speaker("name") — fluent navigation

**File: `sonos-sdk/src/group.rs`**

```rust
/// Get a speaker in this group by name (sync)
///
/// Returns `None` if no member has the given name. Case-sensitive.
pub fn speaker(&self, name: &str) -> Option<Speaker> {
    self.members().into_iter().find(|s| s.name == name)
}
```

### Phase 3: Prelude + Error Polish

#### 3a. Prelude module

**New file: `sonos-sdk/src/prelude.rs`**

```rust
//! Convenience re-exports for common types.
//!
//! ```rust,ignore
//! use sonos_sdk::prelude::*;
//! ```

pub use crate::{
    Group, GroupChangeResult, PlayMode, SdkError, SeekTarget, SonosSystem, Speaker,
};
pub use crate::{WatchMode, WatchStatus};
pub use sonos_state::{
    CurrentTrack, GroupId, GroupMute, GroupVolume, Mute, PlaybackState, SpeakerId, Volume,
};
```

**File: `sonos-sdk/src/lib.rs`** — add `pub mod prelude;`

#### 3b. #[non_exhaustive] on SdkError

**File: `sonos-sdk/src/error.rs`**

```rust
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SdkError {
    // ... existing variants unchanged
}
```

Update any internal tests with exhaustive `match` on `SdkError` to add `_ => unreachable!()` arm.

### Phase 4: Documentation + Cleanup

- Update `docs/specs/sonos-sdk.md` — new API surface, lazy init architecture, fluent navigation
- Update `docs/STATUS.md` — mark API polish as complete
- Update crate-level doc example in `sonos-sdk/src/lib.rs` to use `sonos.speaker("name")` and `sonos_sdk::prelude`
- Update rustdoc on `Speaker`, `Group`, `SonosSystem` structs

## Dependencies & Risks

**Risks:**
1. **Lazy init timing**: First `watch()` becomes ~50-100ms slower (socket bind + thread spawn). Mitigated by: this is a one-time cost, and query/action use cases get much faster.
2. **`Mutex` contention**: Concurrent `watch()` calls contend on the event manager mutex. Mitigated by: contention only happens during the brief init window; once initialized, the mutex is checked-then-returned quickly.
3. **StateManager internal field changes**: Changing `event_manager` from `Option` to `OnceLock` affects all internal reads. Mitigated by: `OnceLock::get()` returns `Option<&T>`, so the call sites remain similar.
4. **`#[non_exhaustive]` is technically breaking**: Downstream exhaustive `match` statements will fail. Mitigated by: pre-1.0 crate, semver allows this in 0.x minor bumps.

**Dependencies:**
- No external crate additions required
- No MSRV change required (`Mutex`, `OnceLock::get/set` available since 1.70)
- Phase ordering: Phase 1 (lazy events) is independent. Phase 2 (renames + navigation) is independent for method renames; the fluent navigation part (`speaker.group()`) depends on the `ensure_topology` extraction which is self-contained. Phase 3 (prelude + error) is fully independent. Phase 4 (docs) runs last. All phases can be implemented on separate branches if desired.

## Sources & References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-03-10-sdk-api-best-practices-brainstorm.md](../brainstorms/2026-03-10-sdk-api-best-practices-brainstorm.md) — Key decisions carried forward: lazy event manager via OnceLock/Mutex, method renames following Rust API Guidelines, fluent navigation replacing system-level cross-entity lookups

### Internal References

- Event manager creation: `sonos-sdk/src/system.rs:130-158`
- StateManager builder: `sonos-state/src/state.rs:634-704`
- PropertyHandle::watch(): `sonos-sdk/src/property/handles.rs:266-292`
- SpeakerContext definition: `sonos-sdk/src/property/handles.rs`
- SonosSystem::ensure_topology(): `sonos-sdk/src/system.rs:355-398`
- SdkError: `sonos-sdk/src/error.rs`
- Group::from_info: `sonos-sdk/src/group.rs:106-132`
- StateManager event_manager field: `sonos-state/src/state.rs:256`

### External References

- Rust API Guidelines — naming conventions: https://rust-lang.github.io/api-guidelines/naming.html
- reqwest entry point pattern: `reqwest::get()` + `Client::new()`
- octocrab fluent handler pattern: `octocrab::instance().repos().get()`
