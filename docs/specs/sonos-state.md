# sonos-state Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

`sonos-state` is the cache-and-notify layer between raw UPnP events and the DOM-like
`sonos-sdk` surface. Without it, every consumer would have to solve the same four problems:

1. **Events are stringly-typed XML deltas.** A UPnP `LastChange` payload carries whatever
   happened to change, as strings, in service-specific shapes. Somebody must turn
   `master_volume: Some("42")` into a typed `Volume(42)`.

2. **Events are addressed by IP, state is addressed by identity.** Speakers arrive over the
   wire as an `IpAddr`; applications think in terms of a speaker's stable UUID. DHCP can move
   a speaker's IP mid-session, so the mapping is not a one-time setup step.

3. **Group semantics do not match subscription semantics.** For `PerCoordinator` services
   (AVTransport, GroupRenderingControl, GroupManagement) only the coordinator holds real
   values; a group member's own events carry empty defaults. Naively storing every event
   overwrites good coordinator data with junk from members.

4. **Sync consumers need to know *that* something changed.** The SDK's target consumer is a
   blocking TUI render loop. It does not want an async stream per property; it wants one
   blocking "something you care about moved" channel it can drain per frame.

The design answer to (4) shapes the whole crate: **`ChangeEvent` is a notification, not a
value.** It carries `{speaker_id, property_key, service, timestamp}` and deliberately no
payload. Consumers re-read through `get_property::<P>()`, which means they always observe the
current value rather than replaying a queue of stale ones, and the channel stays a single
non-generic type regardless of how many property types exist.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Fully synchronous public API | The SDK is sync-first; `sonos-state` must be usable from a blocking render loop with no runtime handle and no `.await` |
| P0 | Valueless change notifications | Decouples the notification channel from property types and guarantees consumers read current, not stale, values |
| P0 | Type-safe typed properties | `Property::KEY` + `SonosProperty::{SCOPE, SERVICE}` associate a property with its storage location and UPnP service at compile time |
| P0 | Coordinator-correct reads and writes | Group members must observe the coordinator's playback state without duplicating it in the store |
| P1 | Notify only what is watched | The `watched` set gates emission, so an unwatched property updates the cache silently instead of waking the consumer |
| P1 | Lazy event plumbing | Nothing subscribes and no thread spawns until the first `watch()`; a fetch-only application pays no event cost |
| P1 | Non-poisoning locks | `parking_lot::RwLock` keeps a panic in one consumer from poisoning shared state for the rest |

### 1.3 Non-Goals

- **Device control.** No SOAP writes originate here. `sonos-sdk` performs the write, then
  calls `set_property()` so the cache reflects it immediately instead of waiting for an event.
- **Subscription lifecycle.** Ref counting, grace periods, renewal, and polling fallback all
  live in `sonos-event-manager` and `sonos-stream`. This crate only says *which* service it
  needs and holds the `WatchGuard` indirectly through the SDK handle.
- **Discovery.** `sonos-discovery::Device` values arrive via `add_devices()`.
- **Persistence.** State is in-memory only, for the life of the `StateManager`.
- **Per-property async streams.** There is no `watch_property()` returning a future or a
  channel receiver. The single `ChangeIterator` is the notification surface.

### 1.4 Success Criteria

- [x] Every public method on `StateManager` is synchronous
- [x] `ChangeEvent` carries no property value; consumers re-read via `get_property`
- [x] Unwatched property updates mutate the cache without emitting an event
- [x] Setting a property to its existing value emits nothing (`PropertyBag::set` returns `false`)
- [x] `PerCoordinator` events from non-coordinators are dropped, not stored
- [x] Group members watching a coordinator-owned property are notified without copying data
- [x] Topology-driven IP changes update both the store and the reverse IP map

---

## 2. Architecture

### 2.1 High-Level Design

```
                          +---------------------------------+
                          |            sonos-sdk            |
                          |  speaker.volume.watch()         |
                          |  speaker.volume.get()           |
                          |  system.iter()                  |
                          +----------------+----------------+
                                           |
       register_watch / get_property / set_property / iter
                                           v
+----------------------------------------------------------------------------+
|                      StateManager  (src/state.rs:303)                       |
|                                                                            |
|  store:         Arc<parking_lot::RwLock<StateStore>>                        |
|  watched:       Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>             |
|  ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>                     |
|  event_tx:      mpsc::Sender<ChangeEvent>  --------------+                  |
|  event_rx:      Arc<Mutex<mpsc::Receiver<ChangeEvent>>>  |                  |
|  event_manager: OnceLock<Arc<SonosEventManager>>         |                  |
|  event_init:    OnceLock<EventInitFn>                    |                  |
+---------------------------------+------------------------+------------------+
                 |                                         |
     read/write  |                            ChangeIterator (src/iter.rs:39)
                 v                                         ^
+----------------------------------------+                 |
| StateStore        (src/state.rs:90)    |                 | blocking recv()
|  speakers / ip_to_speaker              |                 |
|  speaker_props: HashMap<SpeakerId, Bag>|                 |
|  group_props:   HashMap<GroupId, Bag>  |                 |
|  system_props:  PropertyBag            |                 |
|  speaker_to_group / satellite_ids      |                 |
|  PropertyBag = HashMap<TypeId, Box<Any>>  (:259)         |
+--------------------+-------------------+                 |
                     ^                                     |
     apply() + emit  |                                     |
+--------------------+-------------------------------------+------------------+
|            event worker thread  (src/event_worker.rs:38, std::thread)        |
|  for event in SonosEventManager::iter()   <-- blocking, not async            |
|    ZoneGroupTopology -> decode_topology_event -> apply_topology_changes      |
|    otherwise         -> ip_to_speaker -> coordinator gate -> decode_event    |
|                      -> PropertyChange::apply -> maybe emit ChangeEvent      |
+----------------------------------------------------------------------------+
                     ^
                     |  EnrichedEvent
+----------------------------------------------------------------------------+
|      sonos-event-manager (subscriptions)  ->  sonos-stream (events/polling)  |
+----------------------------------------------------------------------------+
```

**Design Rationale**: the crate is a cache plus a doorbell, and the two are deliberately
separate. `StateStore` is the only thing that holds values; the `mpsc` channel carries only
notifications. That split is why the whole API can be sync — there is no need for a
per-property broadcast primitive, no need to keep senders alive per watcher, and no
generic parameter leaking into the channel type. It also means back-pressure cannot lose
state: if the consumer drains slowly, it may coalesce several notifications for one property,
but the value it eventually reads is the newest one.

`parking_lot::RwLock` guards the store rather than `std::sync::RwLock` because a panic in a
consumer thread must not poison state the render loop still needs. The `mpsc::Sender` on
`StateManager` is the reason `StateWatchRegistry` exists as a separate struct — see 2.3.

### 2.2 Module Structure

```
src/
+-- lib.rs            # Public surface and re-exports
+-- state.rs          # StateManager, StateStore, PropertyBag, ChangeEvent,
|                     #   StateWatchRegistry, StateManagerBuilder
+-- event_worker.rs   # spawn_state_event_worker: the std::thread event loop
+-- decoder.rs        # PropertyChange enum + per-service decode functions
+-- iter.rs           # ChangeIterator, TryIter, TimeoutIter
+-- property.rs       # Scope, SonosProperty, the 14 built-in property types
+-- model/            # SpeakerId/GroupId re-exports, SpeakerInfo
|   +-- mod.rs
|   +-- id_types.rs
|   +-- speaker.rs
+-- speaker.rs        # Tests only; the Speaker handle lives in sonos-sdk
+-- error.rs          # StateError, Result
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `state` | Manager, store, type-erased bags, watch registry, builder | `pub` |
| `event_worker` | Background thread draining the event manager | `pub(crate)` (`src/lib.rs:67`) |
| `decoder` | `EnrichedEvent` -> `Vec<PropertyChange>` | `pub` |
| `iter` | Blocking / try / timeout iteration over `ChangeEvent` | `pub` |
| `property` | `Scope`, `SonosProperty`, built-in property types | `pub` |
| `model` | Identity and static device metadata | `pub` |
| `speaker` | Test-only module (`src/speaker.rs:6`) | `pub` (empty in non-test builds) |
| `error` | Error types | `pub` |

### 2.3 Key Types

#### `StateManager` (`src/state.rs:303`)

```rust
pub struct StateManager {
    store: Arc<RwLock<StateStore>>,                              // parking_lot
    watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>,
    event_manager: OnceLock<Arc<SonosEventManager>>,
    event_tx: mpsc::Sender<ChangeEvent>,
    event_rx: Arc<Mutex<mpsc::Receiver<ChangeEvent>>>,
    _worker: Mutex<Option<JoinHandle<()>>>,
    cleanup_timeout: Duration,
    key_to_service: Arc<RwLock<HashMap<&'static str, Service>>>,
    event_init: OnceLock<EventInitFn>,
}
```

**Purpose**: the single sync entry point. Owns the cache, the watched set, and both ends of
the notification channel.

**Why `OnceLock` for `event_manager` and `event_init`**: live events are opt-in and expensive
(subscriptions, a callback HTTP server, a thread). A fetch-only application should pay none of
that. `event_init` (`src/state.rs:53`) is a closure installed by the SDK that builds the event
manager on demand; `PropertyHandle::watch()` calls it on the first watch
(`sonos-sdk/src/property/handles.rs:334`). `set_event_manager()` (`src/state.rs:771`) then
wires the registry and spawns the worker. Both are set-once, so repeated watches are no-ops.

**Why the receiver is `Arc<Mutex<..>>`**: `StateManager` is `Clone` (`src/state.rs:840`) and
clones share one store and one channel. A single `mpsc::Receiver` cannot be cloned, so it is
shared behind a mutex and every `iter()` (`src/state.rs:537`) hands out a `ChangeIterator`
over the same receiver. Consequence worth knowing: multiple concurrent iterators *compete*
for events rather than each seeing all of them.

**Invariants**:
- Every speaker in `store.speakers` has a matching `ip_to_speaker` entry, maintained on
  `add_devices()` (`src/state.rs:413`), `update_speaker_ip()` (`:500`), and topology IP updates
- `_worker` holds at most one thread; the thread exits when all `event_tx` clones drop

#### `StateWatchRegistry` (`src/state.rs:346`)

```rust
struct StateWatchRegistry {
    watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>,
    key_to_service: Arc<RwLock<HashMap<&'static str, Service>>>,
}
```

**Purpose**: implements `sonos_event_manager::WatchRegistry`
(`sonos-event-manager/src/manager.rs:37`) so the event manager can add and remove watches
without depending on `sonos-state`.

**Why it is not `StateManager` itself**: `WatchRegistry: Send + Sync`, but `StateManager`
holds an `mpsc::Sender`, which is `!Sync`. Rather than swap the channel out, the registry is a
separate struct holding only the `Arc`-shared fields it needs. `key_to_service` exists solely
so `unregister_watches_for_service` (`src/state.rs:358`) can reverse a `Service` back into the
property keys that belong to it when a subscription is finally torn down.

#### `StateStore` (`src/state.rs:90`)

```rust
pub struct StateStore {
    pub(crate) speakers: HashMap<SpeakerId, SpeakerInfo>,
    pub(crate) ip_to_speaker: HashMap<IpAddr, SpeakerId>,
    pub(crate) speaker_props: HashMap<SpeakerId, PropertyBag>,
    pub(crate) groups: HashMap<GroupId, GroupInfo>,
    pub(crate) group_props: HashMap<GroupId, PropertyBag>,
    pub(crate) system_props: PropertyBag,
    pub(crate) speaker_to_group: HashMap<SpeakerId, GroupId>,
    pub(crate) satellite_ids: HashSet<SpeakerId>,
}
```

**Purpose**: plain in-memory cache. No channels, no reactivity — reactivity is the manager's
`event_tx`.

**Key method — `get_resolved<P>()` (`src/state.rs:188`)**: if `P::SERVICE.scope()` is
`PerCoordinator` *and* `P::SCOPE == Scope::Speaker`, the read is redirected to the
coordinator's bag via `resolve_coordinator()` (`:171`). This is how a group member reports the
group's playback state without any data being copied into its own bag. Group-*scoped*
properties are excluded from that redirect because they already live in `group_props`.

**Invariants**:
- Every `member_id` of a group in `groups` has a `speaker_to_group` entry (`add_group`, `:141`)
- `clear_groups()` (`:161`) clears `groups`, `group_props`, and `speaker_to_group` together,
  so no mapping outlives its group

#### `PropertyBag` (`src/state.rs:259`)

```rust
pub(crate) struct PropertyBag {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}
```

**Purpose**: heterogeneous storage keyed by `TypeId::of::<P>()`, so adding a property type
requires no change to the store.

**Why `set` returns `bool` (`:279`)**: it compares against the current value and returns
`false` when unchanged. That single boolean is the crate's change-detection primitive — every
emission path is gated on it, which is what keeps UPnP's habit of re-sending identical
`LastChange` payloads from waking the consumer.

#### `ChangeEvent` (`src/state.rs:63`)

```rust
pub struct ChangeEvent {
    pub speaker_id: SpeakerId,
    pub property_key: &'static str,
    pub service: Service,
    pub timestamp: Instant,
}
```

**Purpose**: the doorbell. No value field, by design (see 1.1). `property_key` is
`&'static str` because it always comes from a `Property::KEY` constant, so the event allocates
nothing beyond the `SpeakerId` clone.

#### `SonosProperty` (`src/property.rs:52`)

```rust
pub use state_store::Property;      // src/property.rs:15 — provides const KEY

pub trait SonosProperty: Property {
    const SCOPE: Scope;             // Speaker | Group | System  (src/property.rs:23)
    const SERVICE: Service;
}
```

**Purpose**: `KEY` comes from the generic `state-store` crate; `SCOPE` and `SERVICE` are the
Sonos-specific additions. `SERVICE` is what lets `watch()` know which UPnP service to
subscribe to from the property type alone, with no lookup table to keep in sync.

13 types implement it (`src/property.rs`): `Volume` (:69), `Mute` (:92), `Bass` (:115),
`Treble` (:138), `Loudness` (:161), `GroupVolume` (:188), `GroupMute` (:211),
`GroupVolumeChangeable` (:234), `PlaybackState` (:261), `Position` (:304), `CurrentTrack`
(:364), `GroupMembership` (:419), and `Topology` (:451). `GroupInfo` (:494) is a plain data
type carried inside `Topology`, not a property. Constructors clamp: `Volume::new` caps at
100 (:81), `Bass`/`Treble::new` clamp to ±10 (:127, :150).

---

## 3. Code Flow

### 3.1 Primary Flow: an event becomes a notification

```
SonosEventManager::iter()          sonos-event-manager/src/manager.rs:483
        |  EnrichedEvent (blocking)
        v
event worker thread                src/event_worker.rs:38
        |
        +-- EventData::ZoneGroupTopology? --> 3.2
        |
        +-- ip_to_speaker lookup                    src/event_worker.rs:64
        |     miss -> warn + skip
        |
        +-- PerCoordinator + not coordinator?       src/event_worker.rs:95
        |     yes -> skip (member events carry empty defaults)
        |
        +-- decode_event()                          src/decoder.rs:169
        |     -> DecodedChanges { Vec<PropertyChange> }
        |
        +-- apply_property_change() per change      src/event_worker.rs:298
        |     PropertyChange::apply() -> bool       src/decoder.rs:73
        |     changed && watched -> event_tx.send(ChangeEvent)
        |
        +-- PerCoordinator? notify_group_members()   src/event_worker.rs:272
              emits ChangeEvents for members, copies nothing
                              |
                              v
        ChangeIterator::recv()                       src/iter.rs:52
                              |
        consumer re-reads get_property::<P>()        src/state.rs:545
                              -> get_resolved()      src/state.rs:188
```

**Step-by-step**:

1. **Blocking drain** (`src/event_worker.rs:42`): a plain `std::thread` iterates
   `SonosEventManager::iter()`. No tokio runtime is created or required by this crate.
2. **Identity resolution** (`src/event_worker.rs:64`): the event's `speaker_ip` is mapped
   through `ip_to_speaker`. An unknown IP is logged and skipped rather than guessed at.
3. **Coordinator gate** (`src/event_worker.rs:95`): for `PerCoordinator` services
   (`sonos-api/src/service.rs:101`), events from non-coordinators are dropped. With no group
   data yet, the speaker is treated as its own coordinator — the safe default for a
   standalone speaker.
4. **Decode** (`src/decoder.rs:169`): dispatches on `EventData` to
   `decode_rendering_control` (:188), `decode_av_transport` (:228), or
   `decode_group_rendering_control` (:284). `DeviceProperties` and `GroupManagement` decode to
   an empty vec (`:174`, `:177`) — the former has no API layer yet, the latter is action-only
   and surfaces its effects through topology events instead.
5. **Apply** (`src/decoder.rs:73`): `PropertyChange::apply` routes by scope —
   speaker-scoped variants to `store.set()`, group-scoped variants resolve
   `speaker_to_group` first and write to `store.set_group()`, returning `false` if the speaker
   has no group yet.
6. **Emit if watched** (`src/event_worker.rs:313`): only when `apply` reported a real change
   *and* `(speaker_id, key)` is in `watched`. Both conditions must hold.
7. **Fan out to members** (`src/event_worker.rs:272`): for `PerCoordinator` services,
   `resolve_group_members` (`:252`) returns the non-coordinator members (empty for a
   standalone speaker or a non-coordinator), and each watching member gets its own
   `ChangeEvent`. No value is duplicated — the member's later `get_property` resolves back to
   the coordinator's bag.

### 3.2 Secondary Flow: topology replacement

`ZoneGroupTopology` is handled before the IP lookup (`src/event_worker.rs:50`) because it
describes every speaker at once rather than the one that sent it.

`decode_topology_event()` (`src/decoder.rs:314`) returns a `TopologyChanges`
(`src/decoder.rs:36`) carrying groups, per-speaker `GroupMembership`, `boot_seq` values,
current IPs parsed out of each `location` URL (`extract_ip_from_location`, `:370`), and the
IDs of speakers marked `Invisible="1"` (satellites).

`apply_topology_changes()` (`src/event_worker.rs:155`) then, under one write lock:
`clear_groups()`, re-add every group, `set` each `GroupMembership` while recording which
actually changed, update `boot_seq`, apply IP changes, and replace `satellite_ids`. It
releases the store lock before touching `ip_to_speaker` and before emitting, so the two
`RwLock`s are never held at once in that order.

**Why replace instead of diff**: a topology event is a full snapshot. Rebuilding is
straightforwardly correct; diffing would risk stale groups surviving a regrouping. The cost of
correctness is bounded — `GroupMembership` emissions are still gated on real change
(`src/event_worker.rs:232`), so rebuilding does not spam the consumer.

**Why `boot_seq` is stored**: GroupManagement's `AddMember` requires it, and topology events
are the only place it appears. `get_boot_seq()` (`src/state.rs:495`) exposes it to the SDK.

### 3.3 Secondary Flow: watch registration

`watch()` on an SDK handle (`sonos-sdk/src/property/handles.rs:334`) triggers lazy
`event_init`, calls `resolve_subscription_target()` (`src/state.rs:736`) to route
`PerCoordinator` subscriptions to the coordinator's `(SpeakerId, IpAddr)`, then acquires a
`WatchGuard` from the event manager (`sonos-event-manager/src/manager.rs:201`). The guard's
`register_watch` flows back through `StateWatchRegistry` (`src/state.rs:353`).

`StateManager` also offers `watch_property_with_subscription()` (`src/state.rs:610`) and
`unwatch_property_with_subscription()` (`:636`), which register plus subscribe directly
without a guard. These predate the guard-based path and are not what the SDK uses; the
guard-based route is the one with grace-period cleanup.

Writes from the SDK land via `set_property()` (`src/state.rs:558`) and
`set_group_property()` (`:574`), which update the cache and run the same
`maybe_emit_change()` gate (`:663`). Group property emissions are keyed on the group's
*coordinator* ID, matching how watches are registered.

### 3.4 Error Flow

```
sonos-api            --> StateError::Api            (From impl, src/error.rs:82)
unparseable IP       --> StateError::InvalidIpAddress   (src/state.rs:422)
subscribe failure    --> tracing::warn, watch degrades (src/state.rs:622)
unknown speaker IP   --> tracing::warn, event skipped   (src/event_worker.rs:76)
undecodable field    --> field omitted from Vec<PropertyChange>
closed channel       --> ChangeIterator returns None    (src/iter.rs:53)
```

**Error handling philosophy**: only `add_devices()` rejects input outright — an unparseable IP
means the caller has bad data and nothing useful can be cached. Everything on the event path
degrades instead of failing: a bad field is dropped, a bad event is skipped, a failed
subscription is logged and leaves the property readable from cache. A single malformed event
must not stop a long-running dashboard.

---

## 4. Features

### 4.1 Feature: valueless change notifications

#### What

`ChangeEvent` names what changed; the consumer re-reads the value.

#### Why

Three payoffs. (a) One non-generic channel serves every property type — no per-type
broadcast, no type erasure in the channel. (b) A slow consumer coalesces rather than replays:
whatever it reads is current. (c) The API stays sync, because reading is just a lock and a
clone.

#### How

```rust
// src/state.rs:663
fn maybe_emit_change(&self, speaker_id: &SpeakerId, property_key: &'static str, service: Service) {
    let is_watched = self.watched.read().contains(&(speaker_id.clone(), property_key));
    if is_watched {
        let _ = self.event_tx.send(ChangeEvent::new(speaker_id.clone(), property_key, service));
    }
}
```

Consumer side:

```rust
for event in manager.iter() {
    if event.property_key == Volume::KEY {
        if let Some(v) = manager.get_property::<Volume>(&event.speaker_id) {
            println!("{} -> {}%", event.speaker_id.as_str(), v.value());
        }
    }
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Notification without value | `ChangeEvent<P>` carrying the value | Would make the channel generic; forces one channel per property type and leaks `P` into `iter()` |
| `std::sync::mpsc` | `tokio::sync::broadcast` | No runtime needed; blocking `recv()` is exactly what a sync render loop wants |
| Shared receiver behind a `Mutex` | Receiver per clone | `mpsc::Receiver` is not cloneable, and one drain point matches one render loop |

### 4.2 Feature: watched-set gating

#### What

The cache always updates; the channel only fires for `(speaker_id, property_key)` pairs in the
`watched` set.

#### Why

A speaker emits `RenderingControl` events for volume, mute, bass, treble, and loudness
together. An application watching only volume should not be woken four extra times. Gating on
the watched set means the cache stays warm for `get()` while the notification channel stays
quiet.

#### How

Watches are keyed `(SpeakerId, &'static str)` — per speaker *and* per property, not per
service. Registration comes either from `register_watch()` (`src/state.rs:590`) or through
`StateWatchRegistry` when a `WatchGuard` is acquired. Removal happens on
`unregister_watch()` (`:597`) or, when a subscription is finally torn down after its grace
period, on `unregister_watches_for_service()` (`:358`), which uses `key_to_service` to find
every key belonging to that service.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Per-property watch keys | Per-service keys | A service carries several properties; per-service would emit for all of them |
| Cache updates regardless of watch | Only cache watched properties | Keeps `get()` useful right after `watch()` and lets an unwatched property be read without a fetch |

### 4.3 Feature: coordinator resolution

#### What

For `PerCoordinator` speaker-scoped properties, reads redirect to the coordinator, non-coordinator
events are dropped, and watching members are notified when the coordinator changes.

#### Why

Sonos gives the group's coordinator authority over playback. Members still emit AVTransport
events, but with empty or default values. Storing those would clobber real data. And a member
must still be able to report the group's `PlaybackState` when asked.

#### How

Three cooperating pieces:

```rust
// src/state.rs:188 — reads redirect
pub(crate) fn get_resolved<P: SonosProperty>(&self, speaker_id: &SpeakerId) -> Option<P> {
    if P::SERVICE.scope() == ServiceScope::PerCoordinator && P::SCOPE == Scope::Speaker {
        let coordinator_id = self.resolve_coordinator(speaker_id);
        self.speaker_props.get(&coordinator_id)?.get::<P>()
    } else {
        self.speaker_props.get(speaker_id)?.get::<P>()
    }
}
```

- **Writes**: the worker drops `PerCoordinator` events from non-coordinators
  (`src/event_worker.rs:95`).
- **Notifications**: `notify_group_members()` (`src/event_worker.rs:272`) emits a
  `ChangeEvent` per watching member and copies nothing.
- **Subscriptions**: `resolve_subscription_target()` (`src/state.rs:736`) points the
  member's subscription at the coordinator's IP.

`resolve_coordinator()` (`src/state.rs:171`) returns the speaker's own ID when no group data
exists, so a standalone speaker and a not-yet-known speaker both behave correctly.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Resolve on read | Copy coordinator values into each member's bag | One source of truth; regrouping needs no cache fix-up |
| Drop non-coordinator events | Store them per speaker | Their values are empty defaults and would overwrite good data |
| Notify members explicitly | Let members poll | Members watch their own `(id, key)` pair, so they need their own event |

### 4.4 Feature: lazy event plumbing

#### What

No subscription, no callback server, and no worker thread exist until the first `watch()`.

#### Why

A one-shot CLI that fetches volume and exits should not pay for a subscription, an HTTP
callback listener, and a thread. Making events opt-in on first use keeps the fetch-only path
cheap while `watch()` stays a single call for the user.

#### How

`StateManagerBuilder::build()` (`src/state.rs:903`) leaves `event_manager` as an unset
`OnceLock` unless one was supplied via `with_event_manager()` (`:897`). The SDK installs an
`EventInitFn` (`:53`, `set_event_init` at `:827`); the first `watch()` calls it, which calls
`set_event_manager()` (`:771`) — wiring `StateWatchRegistry`, re-registering known devices,
and spawning the worker. `OnceLock` makes every subsequent watch a no-op.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| `OnceLock` + injected closure | Build the event manager in `new()` | Fetch-only users pay nothing; avoids a circular crate dependency on SDK error types |
| `Box<dyn Error + Send + Sync>` in `EventInitFn` | Concrete SDK error type | `sonos-state` cannot depend on `sonos-sdk` |

### 4.5 Feature: blocking iteration variants

#### What

`ChangeIterator` (`src/iter.rs:39`) offers `recv()` (:52), `recv_timeout()` (:67),
`try_recv()` (:82), `try_iter()` (:98), and `timeout_iter()` (:106), plus `Iterator`
(:114) delegating to `recv()`.

#### Why

Different sync consumers need different blocking behaviour: a dedicated event thread wants to
block forever; a TUI wants to drain what is available and get back to rendering; a
fixed-tick loop wants a bounded wait.

#### How

`TryIter` (`src/iter.rs:126`) and `TimeoutIter` (`:139`) are thin borrows over the same
receiver, so no variant takes ownership and all can be used against one `StateManager`.

---

## 5. Data Model

### 5.1 Core Data Structures

#### `SpeakerId` / `GroupId`

Re-exported from `sonos-api` (`src/model/id_types.rs:5`; defined at
`sonos-api/src/types.rs:34`). `SpeakerId::new` strips a leading `uuid:`
(`sonos-api/src/types.rs:40`) so the same speaker referenced from an SSDP `USN` and from a
topology event hashes to one key.

**Lifecycle**: created during discovery or topology decode; immutable; dropped with the
manager.

#### `SpeakerInfo` (`src/model/speaker.rs:9`, aliased at `src/model/mod.rs`)

```rust
pub struct Speaker {
    pub id: SpeakerId,
    pub name: String,
    pub room_name: String,
    pub ip_address: IpAddr,
    pub port: u16,
    pub model_name: String,
    pub software_version: String,
    pub boot_seq: u32,
    pub satellites: Vec<SpeakerId>,
}
```

Static device metadata, distinct from the dynamic values in `PropertyBag`. `add_devices()`
sets `software_version` to `"unknown"` (`src/state.rs:437`) because discovery does not carry
it. `ip_address` is mutable in place through `update_speaker_ip_address()` (`:227`), the only
field that changes after insertion.

#### `PropertyChange` (`src/decoder.rs:51`)

A 12-variant enum, one per decodable property, with `apply` (:73), `key` (:111), `scope`
(:130), and `service` (:149). It exists so a decoded batch can be moved out of the decoder
without generics and without holding the store lock during decode — the worker decodes first,
then takes the write lock per change (`src/event_worker.rs:308`).

### 5.2 State Transitions

```
                add_devices()                    first watch() -> event_init
                     |                                    |
+-----------+        v         +--------------+           v        +------------------+
| unknown   |----------------->| cached       |--------------------| live             |
| (no entry)|                  | fetch/get ok |                    | worker running   |
+-----------+                  | no events    |<-------------------| events flowing   |
                               +--------------+  guard dropped +   +------------------+
                                                 grace expires
```

**Invariants per state**:
- **unknown**: no `speakers` entry, no `ip_to_speaker` entry; `get_property` returns `None`
- **cached**: registered and readable; `event_manager` unset, no worker thread
- **live**: `event_manager` set, worker thread draining, watched properties emitting

The transition to `live` is one-way for a given `StateManager` — `OnceLock` never unsets. What
does return is subscription state, managed by `sonos-event-manager` after its grace period.

### 5.3 Serialization

| Format | Use Case | Library | Notes |
|--------|----------|---------|-------|
| Serde derive | Property values, `SpeakerInfo`, `Topology`, `GroupInfo` | `serde` | For consumer persistence; not used internally |
| DIDL-Lite XML | Track metadata | Hand-rolled | `parse_track_metadata` (`src/decoder.rs:406`) via `extract_xml_element` (`:430`) |
| `HH:MM:SS[.mmm]` | Positions and durations | Hand-rolled | `parse_duration_ms` (`src/decoder.rs:378`); rejects `NOT_IMPLEMENTED` |

`Scope` and `SonosProperty` are deliberately not serializable — they are compile-time metadata.

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `sonos-api` | `Service`, `ServiceScope`, `SpeakerId`, `GroupId`, `ApiError` | Canonical service definitions; `ServiceScope` (`sonos-api/src/service.rs:38`) drives coordinator resolution |
| `sonos-stream` | `EnrichedEvent`, `EventData`, per-service state structs | Already normalizes UPnP events and polling into one shape |
| `sonos-event-manager` | `SonosEventManager`, `WatchRegistry` | Owns subscription ref counting and grace periods |
| `sonos-discovery` | `Device` | Input type for `add_devices()` |
| `state-store` | Base `Property` trait (`src/property.rs:15`) | Domain-agnostic `KEY`; `SonosProperty` adds the Sonos parts |
| `parking_lot` | `RwLock` | Non-poisoning: a panicking consumer must not poison shared state |
| `serde` | Derives on property types | Lets consumers persist values |
| `tracing` | Logging | Workspace-wide convention |

No async runtime dependency: the worker is a `std::thread` and the channel is `std::sync::mpsc`.

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| `sonos-sdk` | Sole consumer. Wraps `StateManager` in property handles; `system.iter()` (`sonos-sdk/src/system.rs:484`) returns our `ChangeIterator` | Internal crate — signatures may change with the SDK |

`sonos-state` is published to crates.io only as a transitive dependency of `sonos-sdk`.

### 6.3 External Systems

No direct network I/O. All device communication is mediated by `sonos-stream` (events,
polling) and `sonos-api` (SOAP), both reaching speakers over HTTP on port 1400. Discovery's
SSDP multicast on `239.255.255.250:1900` is likewise out of this crate's scope.

---

## 7. Error Handling

### 7.1 Error Types

```rust
// src/error.rs:10
pub enum StateError {
    Init(String),
    Parse(String),
    Api(sonos_api::ApiError),
    AlreadyRunning,
    ShutdownFailed,
    LockError(String),
    SpeakerNotFound(crate::model::SpeakerId),
    InvalidUrl(String),
    InitializationFailed(String),
    DeviceRegistrationFailed(String),
    SubscriptionFailed(String),
    InvalidIpAddress(String),
    LockPoisoned,
}
```

Hand-written `Display` (`src/error.rs:51`) and `Error::source` (`:73`) rather than
`thiserror`, which this crate does not depend on. Only `Api` exposes a `source`
(`src/error.rs:76`), reached through `From<ApiError>` (`:82`).

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Reject bad input, degrade on bad events | `add_devices` returns `InvalidIpAddress`; the worker logs and skips | Caller data errors are fixable; a malformed event must not stop a dashboard |
| Warn, do not fail, on subscription problems | `tracing::warn` in `watch_property_with_subscription` (`src/state.rs:622`) | The property stays readable from cache; the SDK can fall back to polling |
| Partial decode over total failure | Decoders push only the fields they parsed | One bad field should not discard the rest of the event |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `InvalidIpAddress` | No | Fix the `Device` before calling `add_devices()` |
| `Api` | Yes | Retry the operation; usually transient network trouble |
| `SubscriptionFailed` | Yes | SDK falls back to polling or cache-only mode |
| unknown-IP event | Automatic | Call `add_devices()` for the speaker, or wait for a topology event to refresh its IP |

Several variants (`AlreadyRunning`, `ShutdownFailed`, `LockError`, `InvalidUrl`,
`SpeakerNotFound`, `LockPoisoned`) are declared but unconstructed inside this crate — they
remain for consumers and for paths that predate the current design.

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

97 inline unit tests, no `tests/` directory and no network access. Everything the crate does
is a pure function over in-memory state plus one channel, so behaviour is testable by
constructing a store, applying changes, and asserting on both the store and the channel.

```
                    +-------------------+
                    | Live verification |  examples/minimal_example.rs (manual)
                    +--------+----------+
           +-----------------+------------------+
           |    Worker + store integration      |  event_worker.rs (15)
           +-----------------+------------------+
    +------+------+------+------+------+------+------+
    |               Unit tests                        |  state 33, decoder 25,
    +-------------------------------------------------+  property 15, iter 7, speaker 2
```

### 8.2 Unit Tests

**Location**: inline `#[cfg(test)] mod tests` in each source file.

**What is covered**:
- [x] Constructor clamping — `test_volume_clamping` (`src/property.rs:527`), `test_bass_clamping` (:534)
- [x] Property metadata constants — `test_property_constants` (`src/property.rs:601`)
- [x] Per-service decoding — `test_decode_rendering_control` (`src/decoder.rs:565`), `test_decode_av_transport` (:600), `test_decode_group_rendering_control` (:631)
- [x] Topology decode incl. IPs, satellites, `boot_seq` — `src/decoder.rs:513`, :951, :984
- [x] `PropertyChange` key/service/scope mapping — `src/decoder.rs:699`, :713, :731
- [x] Channel semantics — `test_channel_closed` (`src/iter.rs:269`), `test_try_iter` (:233)
- [x] Coordinator resolution — `test_get_resolved_per_coordinator_reads_from_coordinator` (`src/state.rs:1687`), `test_get_resolved_per_speaker_reads_own_props` (:1735)
- [x] Watch gating — `test_change_event_emission` (`src/state.rs:1040`), `test_set_group_property_no_event_when_unwatched` (:1116)
- [x] Registry unregistration — `test_state_watch_registry_register_and_unregister` (`src/state.rs:1526`)
- [x] IP updates — `test_update_speaker_ip` (`src/state.rs:1783`)

### 8.3 Component Tests

`src/event_worker.rs:327` exercises the worker's helpers directly against a real
`StateStore`, a real `watched` set, and a real `mpsc` pair — no mocking, since all three are
cheap to construct.

Notable cases: `test_apply_property_change_with_watch` (`:376`) asserts an event fires only
when watched; `test_apply_topology_changes_no_event_when_membership_unchanged` (`:812`) pins
the change-detection gate; `test_per_coordinator_notifies_members_without_data_copy` (`:865`)
asserts the member is notified *and* that nothing was written to its bag;
`test_per_speaker_service_not_notified` (`:996`) asserts the inverse for `PerSpeaker`
services.

### 8.4 Integration Tests

`sonos-state/examples/minimal_example.rs`, run by hand against real hardware. Requires a
Sonos speaker on the LAN and a firewall that permits UPnP callbacks (or accepts the polling
fallback).

### 8.5 Test Fixtures & Mocks

| Dependency | Strategy | Location |
|------------|----------|----------|
| `StateStore` | Real instance | `StateStore::new()` inline |
| `SpeakerInfo` | Local factory functions | `create_test_speaker_info()` in the relevant test module |
| `ChangeEvent` channel | Real `mpsc::channel()` | Assert on `try_recv()` |
| `EnrichedEvent` | Direct struct construction | `src/decoder.rs` tests |
| `SonosEventManager` | Not mocked | Worker helpers are tested directly instead of through `iter()` |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Event -> notification latency | sub-millisecond | It is a decode, a hash write, and a channel send |
| `get_property` cost | one read lock + one clone | Called on every notification and every render |
| Memory per watched property | one `HashSet` entry + one boxed value | Should scale to a whole household |

### 9.2 Critical Paths

1. **`get_property` / `get_resolved`** (`src/state.rs:545`, `:188`) — read lock, up to two
   hash lookups (coordinator resolution adds one), then a clone. Called per notification per
   frame, so the clone cost is the property's own clone cost; keeping property types small
   matters.
2. **Worker apply loop** (`src/event_worker.rs:124`) — takes the store write lock *per
   change* rather than once per event. Simpler and it shortens the window the render loop can
   be blocked, at the cost of re-locking a few times per event.
3. **`notify_group_members`** (`src/event_worker.rs:272`) — O(members x changes) with the
   `watched` read lock held. Bounded by real group sizes.
4. **Topology apply** (`src/event_worker.rs:169`) — the single largest write-lock hold: it
   rebuilds all groups under one lock. Frequency is low (regrouping is user-driven), so the
   duration is acceptable.

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| Worker thread | First `set_event_manager()` | When all `event_tx` clones drop and the event-manager iterator ends | One per `StateManager` |
| `PropertyBag` entry | First `set` for that speaker | With the speaker | One per `(entity, property type)` |
| `watched` entry | `register_watch` / `WatchGuard` acquisition | `unregister_watch`, or `unregister_watches_for_service` after the grace period | Shared across `StateManager` clones |
| UPnP subscription | Delegated | Delegated | `sonos-event-manager` |

`StateManager` has no `Drop` impl. Shutdown is by channel closure: dropping the last clone
drops the last `event_tx`, the worker's next send fails, and the thread ends when the
event-manager iterator terminates.

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Forged UPnP event on the LAN | Low | Medium (wrong displayed state) | Events from IPs absent from `ip_to_speaker` are dropped (`src/event_worker.rs:76`) |
| Event flooding | Low | Low | Unbounded `mpsc` grows but never blocks the worker; unwatched properties never enqueue at all |
| Malformed XML in track metadata | Medium | Low | Index-based extraction (`src/decoder.rs:430`) returns `None` rather than panicking |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Speaker IPs and UUIDs | Low (LAN-local) | Logged at `debug`/`warn`, not `info` |
| Track metadata | Low | No special handling |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| `Device.ip_address` | Must parse as `IpAddr` | `src/state.rs:419` |
| Event source IP | Must be a known speaker | `src/event_worker.rs:73` |
| Volume strings | `parse::<u8>()`, then `.min(100)` | `src/decoder.rs:193` |
| Group volume | `.min(100)` | `src/decoder.rs:288` |
| Durations | Requires exactly three `:`-separated parts | `src/decoder.rs:378` |
| Topology `location` | Requires `http://` prefix and a parseable host | `src/decoder.rs:370` |

---

## 11. Observability

### 11.1 Logging

| Level | What's Logged | Example |
|-------|--------------|---------|
| `warn` | Unknown speaker IP, failed subscribe/unsubscribe, event-manager device registration failure | "Received event from unknown speaker IP" (`src/event_worker.rs:76`) |
| `info` | Manager creation, worker start/stop, speaker IP changes | "State event worker started" (`src/event_worker.rs:39`) |
| `debug` | Per-event receipt, decode counts, per-change application, emissions | "Decoded {} property changes from event" (`src/event_worker.rs:118`) |
| `trace` | Iterator yields | `ChangeIterator::recv` (`src/iter.rs:55`) |

The `debug` level is the intended level for diagnosing "why did my watch not fire?" — it
traces IP resolution, the coordinator gate, decode output, and the watched-set check, which is
the full set of places an event can be legitimately dropped.

### 11.2 Tracing

No explicit `#[instrument]` spans; observability is event-based logging on the flow above.

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `cleanup_timeout` | `Duration` | 5s (`src/state.rs:878`) | Builder field, stored but not currently read by any code path — subscription teardown timing lives in `sonos-event-manager`'s `GRACE_PERIOD` (`sonos-event-manager/src/manager.rs:27`) |
| `event_manager` | `Option<Arc<SonosEventManager>>` | `None` | Supply at build time (`:897`) for eager events; otherwise events arrive lazily via `EventInitFn` |

### 12.2 Environment Variables

None. Configuration is programmatic.

---

## 13. Migration & Compatibility

### 13.1 API Stability

`sonos-state` is a workspace-internal crate. It is published so that `sonos-sdk` resolves on
crates.io, but its API carries no stability guarantee and may change in any release. Consumers
should depend on `sonos-sdk`.

| API | Stability | Notes |
|-----|-----------|-------|
| `StateManager`, `ChangeEvent`, `ChangeIterator` | Internal | Shaped by SDK needs |
| `Property` / `SonosProperty` and the built-in types | Internal, re-exported by `sonos-sdk` | Adding a property is additive |
| `decoder::*` | Internal | Grows with each new service |

### 13.2 Breaking Changes

The workspace versions together; breaking changes ride the `sonos-sdk` version.

### 13.3 Version History

| Version | Changes |
|---------|---------|
| 0.5.x | Sync-first design: `std::thread` worker, `mpsc` notifications, valueless `ChangeEvent`, coordinator resolution, lazy `EventInitFn` |
| 0.2.1 | `StateWatchRegistry` implementing `WatchRegistry`; moved to `parking_lot::RwLock` |
| 0.1.0 | Initial release |

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Single shared `ChangeIterator` receiver | Concurrent iterators compete for events instead of each seeing all | Drain from one place and fan out in application code | Would need a broadcast primitive |
| Properties start as `None` | First `get()` before any event returns nothing | Use the SDK's `fetch()` or `watch_or_fetch()` | — |
| `DeviceProperties` and `GroupManagement` decode to empty | No properties from those services | — | Tracked in `docs/STATUS.md` |
| Unbounded notification channel | A never-draining consumer grows memory | Drain, or use `try_iter()` per frame | Bounded channel with a drop policy |
| `cleanup_timeout` unused | Builder option has no effect | Ignore it | Remove or wire through |
| `system_props` write-only in practice | `Topology` is stored by `initialize()` (`src/state.rs:681`) but has no public system-scoped getter | Use `groups()` / `speaker_infos()` | Add a system-scope accessor |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| Two overlapping watch paths: `watch_property_with_subscription` vs. the SDK's guard-based `acquire_watch` | `src/state.rs:610`, `:636` | Medium | Remove the pre-guard path once nothing depends on it |
| Unconstructed `StateError` variants | `src/error.rs:10` | Low | Prune to the variants actually produced |
| Hand-rolled XML extraction while `sonos-stream` already depends on `quick-xml` | `src/decoder.rs:430` | Low | Move DIDL parsing into `sonos-stream` or adopt `quick-xml` here |
| `software_version` hardcoded to `"unknown"` | `src/state.rs:437` | Low | Read from the device description |
| Write lock retaken per change inside one event | `src/event_worker.rs:308` | Low | Batch under one lock if profiling shows it matters |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Bounded notification channel | P1 | Bound memory when a consumer stalls | Choose a drop policy |
| Coalesce notifications per property | P2 | A burst on one property need only wake the consumer once | Timer or dedup on the watched key |
| System-scope property accessor | P2 | `Topology` is stored but unreachable | — |
| Decoders for new services | P2 | Follows API/stream layers per `docs/STATUS.md` | `sonos-api`, `sonos-stream` |

### 15.2 Open Questions

- [ ] **Should `ChangeEvent` be coalesced in the channel rather than by the consumer?**
  Because events carry no value, dedup on `(speaker_id, property_key)` is safe — the consumer
  re-reads anyway. The open part is where to put the dedup window.
- [ ] **Should `get_resolved` be exposed directly?** Today coordinator resolution is implicit
  in `get_property`. An explicit "give me my own value, unresolved" accessor might be useful
  for diagnostics.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| Property | A typed value with a `KEY`, a `Scope`, and an owning `Service` (e.g. `Volume`) |
| Scope | Where a property is stored: `Speaker`, `Group`, or `System` (`src/property.rs:23`) |
| ServiceScope | How a service subscribes: `PerSpeaker`, `PerNetwork`, `PerCoordinator` (`sonos-api/src/service.rs:38`) |
| PropertyBag | `HashMap<TypeId, Box<dyn Any>>` holding one entity's property values |
| Watched set | `HashSet<(SpeakerId, &'static str)>` gating notification emission |
| ChangeEvent | Valueless notification that a watched property changed |
| Coordinator | The speaker owning playback state for its group |
| Satellite | A speaker marked `Invisible="1"` in topology (surround, sub) |
| Event worker | The `std::thread` draining `SonosEventManager::iter()` |

### B. References

- [UPnP Device Architecture 1.0](http://upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v1.0.pdf)
- [Sonos Control API Documentation](https://developer.sonos.com/reference/control-api/)
- [docs/STATUS.md](../STATUS.md) — service completion matrix
- [docs/specs/sonos-event-manager.md](sonos-event-manager.md) — subscription lifecycle
- [docs/specs/sonos-stream.md](sonos-stream.md) — event delivery and polling fallback
- [docs/specs/state-store.md](state-store.md) — the generic `Property` trait

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2026-01-14 | Claude Opus 4.5 | Initial specification created |
| 2026-08-15 | Claude Opus 5 | Rewritten to match the implemented sync-first design. The prior revision documented an async `tokio::sync::watch` architecture (`reactive.rs`, `store.rs`, `watcher.rs`, `change_iterator.rs`, `decoders/*`, `PropertyWatcher<P>`, async `watch_property()`) that does not exist in the code |
