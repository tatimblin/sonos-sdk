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

The design answer to (4) shapes the whole crate: **`ChangeEvent` carries the value it
announces.** It is `{speaker_id, change: PropertyChange, source, timestamp}`, where
`PropertyChange` is the same typed enum the decoder already produces.

This reverses an earlier decision. Through 0.6.x the event was a valueless doorbell and
consumers re-read through `get_property::<P>()`. That kept the channel non-generic — which is
still a goal — but it made queued events lossy in a way that could not be worked around: by the
time a consumer drains three queued events the store already holds the newest value, so a
`Playing -> Transitioning -> Playing` sequence read as `Playing` three times. The intermediate
state, and the fact that anything moved at all, were unrecoverable. It also cost one store lock
per event per watcher.

Reusing `PropertyChange` rather than introducing a parallel value type keeps the channel a
single non-generic type: the enum is closed over the 12 decodable properties, so `ChangeEvent`
stays concrete no matter how many watchers or property types exist. `property_key()` and
`service()` are now derived from the payload rather than stored beside it, so they cannot drift
from the value.

The store remains the right source for a *full repaint* — a dashboard redrawing every field
wants current state, not one property's history. Both readings are now available; the event is
authoritative for "what changed", the store for "what is true now".

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Fully synchronous public API | The SDK is sync-first; `sonos-state` must be usable from a blocking render loop with no runtime handle and no `.await` |
| P0 | Value-carrying change notifications | A consumer draining a backlog observes every value the property passed through, with no store lock per event |
| P0 | Monotonically ordered writes | A slow `fetch()` response must not overwrite a newer event-derived value; writes are ordered by *observation* time, not arrival time |
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
- [x] `ChangeEvent` carries the new value as a typed `PropertyChange`
- [x] A queued `Playing -> Transitioning -> Playing` burst is fully observable through `iter()`
- [x] A `fetch()` observed before a stored event is rejected rather than applied
- [x] Unwatched property updates mutate the cache without emitting an event
- [x] Setting a property to its existing value emits nothing (`PropertyBag::set` returns `WriteOutcome::Unchanged`)
- [x] `PerCoordinator` events from non-coordinators are dropped, not stored
- [x] Group members watching a coordinator-owned property are notified without copying data
- [x] `set_property()` writes to the bag `get_property()` reads from, for every property
- [x] Releasing one watcher of a property leaves the others watching and receiving
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
|  watched:       Arc<RwLock<WatchCounts>>   (refcounted, see 4.2)           |
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
|            event worker thread  (src/event_worker.rs:31, std::thread)        |
|  run_event_loop (:69): for event in SonosEventManager::iter()  <-- blocking  |
|    each event body wrapped in catch_unwind (:97) -> a panic skips one event  |
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
    watched: Arc<RwLock<WatchCounts>>,                            // refcounted, 4.2
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
    watched: Arc<RwLock<WatchCounts>>,
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

#### `ChangeEvent` (`src/state.rs`)

```rust
pub struct ChangeEvent {
    pub speaker_id: SpeakerId,
    pub change: PropertyChange,   // the new value, typed
    pub source: ChangeSource,     // Event | LocalAction | Fetch
    pub timestamp: Instant,       // when the value was *observed*
}

impl ChangeEvent {
    pub fn property_key(&self) -> &'static str;  // derived from `change`
    pub fn service(&self) -> Service;            // derived from `change`
}
```

**Purpose**: announce *and deliver* a change. `change` is the payload; `property_key()` and
`service()` are derived from it rather than stored, so the label cannot disagree with the value.
`source` lets a consumer distinguish a device report from this process's own optimistic write.

`timestamp` is the observation instant, not the send instant — the same `WriteStamp` that
ordered the write (see below), so an event and its store entry always agree.

#### `ChangeSource` / `WriteStamp` / `WriteOutcome` (`src/state.rs`)

```rust
pub enum ChangeSource { Event, LocalAction, Fetch }   // authority, descending

pub struct WriteStamp {
    pub observed_at: Instant,   // when the observation was made, NOT when written
    pub source: ChangeSource,
}

pub enum WriteOutcome { Changed, Unchanged, Stale }
```

**Purpose**: order writes by when the underlying observation happened. The distinction is
load-bearing for `fetch()`: it reads the device at *request* time but calls `set_property` at
*response* time, a gap of up to hundreds of milliseconds. Stamped at write time it would always
look newest and would clobber any event that arrived while it was in flight — the visible bug
being a speaker snapping back to its previous volume a moment after changing.

`PropertyBag::set` therefore checks staleness *before* comparing values, so a rejected write
neither changes the value nor advances the stamp. The stamp *is* recorded on an `Unchanged`
write, because that observation is still the most recent one and later writes must order against
it.

`WriteOutcome` replaces the previous `bool`: once writes are ordered, "the value differs" and
"the write was allowed" are separate questions. Only `Changed` emits a notification.

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
run_event_loop                     src/event_worker.rs:69
        |  catch_unwind per event -> panic logs at error! and skips one event
        v
handle_event                       src/event_worker.rs:117
        |
        +-- EventData::ZoneGroupTopology? --> 3.2
        |
        +-- ip_to_speaker lookup                    src/event_worker.rs:150
        |     miss -> warn + skip
        |
        +-- PerCoordinator + not coordinator?       src/event_worker.rs:177
        |     yes -> skip (member events carry empty defaults)
        |
        +-- decode_event()                          src/decoder.rs:182
        |     -> DecodedChanges { Vec<PropertyChange> }
        |
        +-- apply_property_change() per change      src/event_worker.rs:413
        |     PropertyChange::apply(stamp) -> WriteOutcome   src/decoder.rs
        |     outcome.changed() && watched -> event_tx.send(ChangeEvent { change, .. })
        |
        +-- PerCoordinator? notify_group_members()   src/event_worker.rs:387
              emits ChangeEvents carrying the coordinator's value; the store
              still holds one copy, in the coordinator's bag
                              |
                              v
        ChangeIterator::recv()                       src/iter.rs:52
                              |
        consumer re-reads get_property::<P>()        src/state.rs:545
                              -> get_resolved()      src/state.rs:188
```

**Step-by-step**:

1. **Blocking drain** (`src/event_worker.rs:69`): a plain `std::thread` iterates
   `SonosEventManager::iter()`. No tokio runtime is created or required by this crate.
2. **Panic containment** (`src/event_worker.rs:97`): the body for each event runs inside
   `std::panic::catch_unwind`. See 4.6 for why this exists and why it is sound.
3. **Identity resolution** (`src/event_worker.rs:150`): the event's `speaker_ip` is mapped
   through `ip_to_speaker`. An unknown IP is logged and skipped rather than guessed at.
4. **Coordinator gate** (`src/event_worker.rs:177`): for `PerCoordinator` services
   (`sonos-api/src/service.rs:101`), events from non-coordinators are dropped. With no group
   data yet, the speaker is treated as its own coordinator — the safe default for a
   standalone speaker — and that fallback is logged at `debug` (`:191`), because it is also
   what an incomplete topology looks like and it silently promotes every member to
   coordinator.
5. **Decode** (`src/decoder.rs:182`): dispatches on `EventData` to
   `decode_rendering_control` (:201), `decode_av_transport` (:241), or
   `decode_group_rendering_control` (:311). `DeviceProperties` and `GroupManagement` decode to
   an empty vec (`:187`, `:190`) — the former has no API layer yet, the latter is action-only
   and surfaces its effects through topology events instead.
6. **Apply** (`src/decoder.rs`): `PropertyChange::apply(.., stamp)` routes by scope —
   speaker-scoped variants to `store.set()`, group-scoped variants resolve
   `speaker_to_group` first and write to `store.set_group()`. A group-scoped change for a
   speaker with no group mapping returns `WriteOutcome::Unchanged` and is logged at `warn`
   (`log_unmapped_group_change`, `src/decoder.rs:113`) rather than dropped silently.
7. **Emit if watched** (`src/event_worker.rs`): only when `apply` reported a real change
   *and* `(speaker_id, key)` is in `watched`. Both conditions must hold.
8. **Fan out to members** (`src/event_worker.rs`): for `PerCoordinator` services,
   `resolve_group_members` (`:367`) returns the non-coordinator members (empty for a
   standalone speaker or a non-coordinator), and each watching member gets its own
   `ChangeEvent` carrying the coordinator's value. The *store* still holds one copy — the
   member's `get_property` resolves back to
   the coordinator's bag.

### 3.2 Secondary Flow: topology replacement

`ZoneGroupTopology` is handled before the IP lookup (`src/event_worker.rs:138`) because it
describes every speaker at once rather than the one that sent it.

`decode_topology_event()` (`src/decoder.rs:341`) returns a `TopologyChanges`
(`src/decoder.rs:36`) carrying groups, per-speaker `GroupMembership`, `boot_seq` values,
current IPs parsed out of each `location` URL (`extract_ip_from_location`, `:397`), and the
IDs of speakers marked `Invisible="1"` (satellites).

`apply_topology_changes()` (`src/event_worker.rs:249`) then, under one write lock:
`clear_groups()`, re-add every group, `set` each `GroupMembership` while recording which
actually changed, update `boot_seq`, apply IP changes, and replace `satellite_ids`. It
releases the store lock before touching `ip_to_speaker` and before emitting, so the two
`RwLock`s are never held at once in that order.

**Why replace instead of diff**: a topology event is a full snapshot. Rebuilding is
straightforwardly correct; diffing would risk stale groups surviving a regrouping. The cost of
correctness is bounded — `GroupMembership` emissions are still gated on real change
(`src/event_worker.rs:352`), so rebuilding does not spam the consumer.

**Why an empty snapshot is ignored** (`src/event_worker.rs:270`): "replace" is only correct
when the event actually carries a snapshot, and a `ZoneGroupTopology` NOTIFY does not have to.
Sonos sends topology events for other evented variables too — `AlarmRunSequence`,
`ThirdPartyMediaServersX`, and (observed on real hardware) a bare
`<VanishedDevices></VanishedDevices>` — and `ZoneGroupTopologyEvent::zone_groups()`
(`sonos-api/src/services/zone_group_topology/events.rs:261`) returns an empty `Vec` whenever
the `ZoneGroupState` variable is absent. Clearing on that would drop `groups`, `group_props`,
and `speaker_to_group` in response to an unrelated update, so `groups()` would report nothing
and coordinator resolution would fall back to "every speaker is its own coordinator" until the
next full snapshot arrived. `apply_topology_changes` therefore treats `changes.groups.is_empty()`
as a *partial* event: it logs at `warn` with the other field counts and returns without
touching the store. The cost is that a genuine "all groups dissolved" snapshot cannot be
expressed by an empty event — but Sonos never sends one, because a speaker that is playing
nothing is still its own single-member group.

**Why `boot_seq` is stored**: GroupManagement's `AddMember` requires it, and topology events
are the only place it appears. `get_boot_seq()` (`src/state.rs:495`) exposes it to the SDK.

### 3.3 Secondary Flow: watch registration

`watch()` on an SDK handle (`sonos-sdk/src/property/handles.rs`) triggers lazy
`event_init`, calls `resolve_subscription_target()` (`src/state.rs:1159`) to route
`PerCoordinator` subscriptions to the coordinator's `(SpeakerId, IpAddr)`, then acquires a
`WatchGuard` from the event manager (`sonos-event-manager/src/manager.rs:201`). The guard's
`register_watch` flows back through `StateWatchRegistry` (`src/state.rs:353`).

The returned `WatchHandle` holds the guard and *reads through* `get_property()` on each access
rather than capturing a value — see §4.4 of the sonos-sdk spec. Reads therefore go through
`get_resolved()` and observe only what survived the write ordering in 4.1a; a handle cannot
report a value the store has already superseded.

`StateManager` also offers `watch_property_with_subscription()` (`src/state.rs:610`) and
`unwatch_property_with_subscription()` (`:636`), which register plus subscribe directly
without a guard. These predate the guard-based path and are not what the SDK uses; the
guard-based route is the one with grace-period cleanup.

Writes from the SDK land via `set_property()` and `set_group_property()`, which update the
cache and run the same `maybe_emit_change()` gate. `set_property()` routes the write through
`resolve_write_target()` so it lands in the same bag `get_property()` reads from — see 4.3.
Group property emissions are keyed on the group's *coordinator* ID, matching how watches are
registered.

### 3.4 Error Flow

```
sonos-api            --> StateError::Api            (From impl, src/error.rs:82)
unparseable IP       --> StateError::InvalidIpAddress   (src/state.rs:422)
subscribe failure    --> tracing::warn, watch degrades (src/state.rs:622)
unknown speaker IP   --> tracing::warn, event skipped   (src/event_worker.rs:159)
groupless group prop --> tracing::warn, change dropped  (src/decoder.rs:113)
empty topology event --> tracing::warn, event ignored   (src/event_worker.rs:270)
unparseable RelTime  --> tracing::debug, no Position emitted (src/decoder.rs:272)
overflowing duration --> None from parse_duration_ms     (src/decoder.rs:427)
panic in one event   --> tracing::error + counter, next event runs (src/event_worker.rs:97)
undecodable field    --> field omitted from Vec<PropertyChange>
closed channel       --> ChangeIterator returns None    (src/iter.rs:53)
```

**Error handling philosophy**: only `add_devices()` rejects input outright — an unparseable IP
means the caller has bad data and nothing useful can be cached. Everything on the event path
degrades instead of failing: a bad field is dropped, a bad event is skipped, a failed
subscription is logged and leaves the property readable from cache. A single malformed event
must not stop a long-running dashboard — which is also why a *panicking* event no longer stops
one (see 4.6).

**Degrade loudly, not silently**: every one of those fallbacks logs. The distinction matters
because several of them are indistinguishable from a real value at the API surface — a dropped
group property looks like "no group volume yet", a defaulted position looks like 0:00, and a
dead worker looks like a household where nothing ever changes. The log line is the only way a
consumer can tell the difference, so no fallback on the event path is allowed to be quiet.

---

## 4. Features

### 4.1 Feature: value-carrying change notifications

#### What

`ChangeEvent` delivers the new value alongside the identity of what changed.

#### Why

The store holds only the *latest* value. A consumer that drains a backlog and re-reads per event
therefore cannot see intermediate values, and cannot tell three changes from one. Carrying the
value makes the event stream a faithful record; it also removes one store lock per event per
watcher.

#### How

```rust
fn maybe_emit_change<P: SonosProperty>(&self, speaker_id: &SpeakerId, value: &P, stamp: WriteStamp) {
    if !is_pair_watched(&self.watched.read(), speaker_id, P::KEY) { return; }
    let Some(change) = value.to_change() else { /* warn: no variant */ return };
    let _ = self.event_tx.send(ChangeEvent::new(speaker_id.clone(), change, stamp));
}
```

`SonosProperty::to_change()` maps a typed value to its `PropertyChange` variant. It defaults to
`None` so a newly added property cannot silently acquire a wrong payload — it must opt in. The
only property returning `None` today is `Topology`, which is written wholesale by `initialize()`
and is not watchable; that path warns rather than dropping the notification silently.

Consumer side:

```rust
for event in manager.iter() {
    match &event.change {
        PropertyChange::Volume(v) => println!("{} -> {}%", event.speaker_id.as_str(), v.value()),
        other => println!("{} changed", other.key()),
    }
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Reuse `PropertyChange` as the payload | A new value-carrying enum, or `ChangeEvent<P>` | `PropertyChange` is already the typed, closed, decoder-produced representation of exactly these values. A parallel type would need the same 12 variants and could drift from the decoder; a generic `ChangeEvent<P>` would make the channel generic and force one channel per property type |
| Derive `property_key()` / `service()` from the payload | Keep them as struct fields | Two sources of truth for "which property is this" can disagree; derivation makes that impossible |
| Keep the store as well | Events only | A full repaint legitimately wants current state, not one property's history. Both readings now exist |
| `std::sync::mpsc` | `tokio::sync::broadcast` | No runtime needed; blocking `recv()` is exactly what a sync render loop wants. `broadcast::Receiver::blocking_recv()` additionally *panics* inside a Tokio runtime and offers no `recv_timeout` — see 4.1b |
| One unbounded queue per subscriber, fanned out | One shared receiver behind a `Mutex` | A shared receiver made concurrent `iter()` calls *compete*: each event went to whichever consumer won the lock, so each saw a random subset, silently. See 4.1b |

### 4.1b Feature: per-subscriber event fan-out

#### What

Every `iter()` call returns an **independent** `ChangeIterator` with its own unbounded queue.
All subscribers receive all events. `EventFanout` (`src/iter.rs`) owns the registry of senders;
`StateManager` holds one `Arc<EventFanout>` in place of the former `event_tx`/`event_rx` pair.

#### Why

`iter()` used to hand every caller a clone of one `Arc<Mutex<mpsc::Receiver>>`. Two
`for event in system.iter()` loops therefore *split* the stream between them — each event went
to whichever consumer happened to win the mutex. But `iter()` returning an independent iterator
is the universal Rust idiom for "iterate the whole thing", so the API read as a broadcast and
behaved as a work queue. Nothing errored, nothing was logged, and each consumer simply saw a
random subset: a dashboard that added a second event loop in a background thread started
dropping roughly half its updates with no signal at all. Silence instead of an error is exactly
the failure class this campaign exists to remove.

#### How

```rust
pub(crate) fn send(&self, event: ChangeEvent) -> usize {
    let mut inner = self.inner.lock();
    let mut delivered = 0;
    inner.subscribers.retain(|(_, tx)| match tx.send(event.clone()) {
        Ok(()) => { delivered += 1; true }
        Err(_) => false,   // receiver gone: reap the dead sender
    });
    delivered
}
```

Four properties make this safe:

- **Nothing is dropped.** Each subscriber owns an *unbounded* `std::sync::mpsc` queue, so a slow
  consumer neither loses events nor blocks a fast one. There is no lag state for a consumer to
  detect because there is no lag — which is why no lag-detection API is offered. The cost is
  unchanged from before: a subscriber that never drains grows its own queue (see 14.1).
- **Order is preserved per subscriber.** All sends happen under one lock in emit order, so every
  subscriber observes the same sequence the emitter produced, keeping 4.1a meaningful downstream.
- **Two independent cleanup paths.** `ChangeIterator::drop` deregisters by id immediately; `send`
  additionally reaps any subscriber whose receiver has gone. The registry cannot accumulate dead
  senders, and a departing consumer never stalls the survivors.
- **`ChangeIterator` holds a `Weak<EventFanout>`, not an `Arc`.** A strong reference would keep
  the fan-out — and so the iterator's own `Sender` — alive exactly as long as the iterator, so
  the channel could never close and `recv()` would block forever after the manager was dropped.
  `Weak` preserves the pre-fan-out behaviour: dropping the manager makes `recv()` return `None`.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Hand-rolled sync fan-out | `tokio::sync::broadcast` | This crate is sync-first and assumes no runtime. `blocking_recv()` **panics** ("Cannot block the current thread from within a runtime") when the caller's thread is already inside a runtime — the same runtime-within-runtime failure tracked against `sonos-stream` in `docs/STATUS.md` — and `broadcast::Receiver` has no `recv_timeout`, which `ChangeIterator` needs. Its fixed ring also drops events for slow consumers |
| Broadcast to all | Enforce a single consumer (`Result`/panic/compile-time move) | Cheaper and honest, but it forecloses the multi-consumer case instead of serving it. Two event loops is a reasonable thing for a dashboard to want |
| Unbounded per-subscriber queues | Bounded queues with a drop policy | Dropping events silently would recreate the very bug being fixed. Bounded-with-detection is possible later without changing the API shape, since nothing is dropped today |
| No replay for late subscribers | Buffer recent events for replay | The store already answers "what is the current state"; an event stream answers "what changed since I started listening". A replay buffer would need a retention policy and would blur the two |

### 4.1a Feature: monotonic write ordering

#### What

Every write carries a `WriteStamp` recording when its value was *observed*. A write observed
before the stored one is rejected as `Stale`.

#### Why

`fetch()` and UPnP events race. A `fetch()` issued at t0 whose response lands at t2 describes
the device as of t0, but without ordering it would overwrite an event that arrived at t1 with
newer truth. Ordering by arrival is simply wrong here; ordering by observation is correct.

#### How

Callers stamp at the right moment:

| Path | Stamp | Rationale |
|------|-------|-----------|
| Event worker (UPnP NOTIFY) | `WriteStamp::observed_at(Event, event.observed_at)` | The NOTIFY's arrival instant at the callback server, threaded down |
| Event worker (polling) | `WriteStamp::observed_at(Event, event.observed_at)` | The poll *request* instant, threaded down the same field |
| `set_property` (local action) | `WriteStamp::now(LocalAction)` | The device just acknowledged the action |
| `fetch()` | `WriteStamp::observed_at(Fetch, t_before_request)` | The read describes the device at request time |

Ties on an exact `Instant` break by `ChangeSource` authority (`Event` > `LocalAction` > `Fetch`),
so an event cannot be displaced by a `fetch()` that happens to share its timestamp.

A rejected `fetch()` still returns its value to its caller — it just does not overwrite a newer
cache entry. Staleness is tracked per property (`stamps: HashMap<TypeId, WriteStamp>` inside each
`PropertyBag`), so a slow volume fetch cannot stale out an unrelated mute write.

#### Where an event's observation instant comes from

`handle_event` takes one stamp per event from `EnrichedEvent::observed_at` — a monotonic
`Instant` — and passes it to every write and notification the event produces, so a whole
topology snapshot or a multi-property NOTIFY cannot order against itself.

`observed_at` is set upstream at the earliest point the process could have known the values.
For a **UPnP NOTIFY**:

1. `EventRouter::route_event` stamps `NotificationPayload::received_at` when the HTTP NOTIFY
   lands, before taking its own lock. An event buffered during the SUBSCRIBE/NOTIFY race keeps
   its *original* arrival instant when replayed, not the replay instant.
2. `EventProcessor::process_notification_for_registration` threads that instant into
   `EnrichedEvent::observed_at`, so the SID lookup, the XML parse, and the channel hop are not
   counted as part of the observation.
3. The worker stamps from it rather than from `Instant::now()`.

For a **poll**, the polling loop in `sonos-stream/src/polling/scheduler.rs` captures the instant
immediately before `poll_device_state` and threads it into `EnrichedEvent::observed_at`. A poll
response describes the device as of the request, exactly like `fetch()`, so the SOAP round trip,
the change comparison, and `state_to_event_data` all fall *after* the observation. The capture
sits inside the loop iteration and below the interval sleep, not above it: backdating past the
sleep would make a legitimately newer poll look older than it is and get dropped as stale —
the inverse failure, and the more damaging one.

`EnrichedEvent` carries both clocks on purpose. `timestamp: SystemTime` remains for display and
logging; `observed_at: Instant` is what ordering uses. The wall clock is unusable for ordering
because an NTP correction can step it backwards, which would invert the comparison. Deriving an
`Instant` from the existing `SystemTime` was rejected for the same reason: it needs a captured
(`SystemTime`, `Instant`) reference pair, and any backwards wall-clock jump between capture and
conversion yields a negative delta that must be clamped — a lossy fallback to guessing "now" in
exactly the case the ordering exists to survive. Capturing the monotonic instant directly at
ingest removes the conversion, and with it the failure mode. A logical/sequence clock was also
rejected: `fetch()` observations originate outside the event pipeline and there is no single
point that could allocate sequence numbers covering both.

Every write source now stamps at observation rather than at application, so both symptoms this
ordering exists to prevent ("volume snaps back", and `get()` permanently disagreeing with
`fetch()`) are addressed for UPnP-sourced, poll-sourced, `fetch()`, and local-action writes
alike.

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
service — and each key maps to a `WatchHolds`, not to a bare presence bit:

```rust
// src/state.rs
pub(crate) struct WatchHolds {
    subscription: bool,   // any WatchGuard registered for this pair
    direct: usize,        // outstanding StateManager::register_watch holds
}
pub(crate) type WatchCounts = HashMap<(SpeakerId, &'static str), WatchHolds>;
```

A pair is watched while the map contains it, and the entry is removed only when the flag is
clear *and* the count is zero. Registration comes either from `register_watch()` or through
`StateWatchRegistry` when a `WatchGuard` is acquired; release happens on `unregister_watch()`
or, when a subscription is finally torn down after its grace period, on
`unregister_watches_for_service()`, which uses `key_to_service` to find every key belonging to
that service. `is_pair_watched()` is the single read used by every emission gate.

**Why a watch is a hold, not a flag.** Several independent watchers can hold the same pair at
once: two widgets on one property, an SDK `WatchHandle` alongside a direct `register_watch()`, or
a handle being reacquired while the previous one has not yet dropped. When `watched` was a
`HashSet`, the first release removed the only entry, so a
surviving watcher went silent while still holding its `WatchHandle`: `is_watched()` returned
`false` and `system.iter()` stopped reporting the property. Worse, teardown was *wholesale* —
`unregister_watches_for_service` removed every key belonging to the service — so releasing a
`Volume` handle also unregistered `Mute`, `Bass`, `Treble` and `Loudness`, which all share
`RenderingControl`. Dropping one handle silenced its siblings.

**Why two fields instead of one counter.** The two kinds of hold are released by different
events on different schedules, and no single integer models both:

- `direct` holds are taken and released one at a time — `register_watch()` /
  `unregister_watch()`, normally from the SDK's `CacheOnlyGuard::drop`. These are the ones that
  need counting, so that *n* watchers survive *n-1* drops.
- `subscription` covers every `WatchGuard`, and is only ever cleared in *bulk*.
  `WatchGuard::drop` does not touch this map at all; it decrements
  `sonos-event-manager`'s per-`(ip, service)` ref count, and
  `unregister_watches_for_service()` fires later, once *that* count has reached zero — at which
  point every contributing guard is provably gone. A counter incremented per guard but cleared
  only in bulk would either leak (decrement-by-one leaves a residue, and a watch nobody holds
  emits forever) or over-release (clearing to zero while sibling guards are still alive). A
  boolean states exactly what is knowable.

Keeping them separate is what fixes the sibling bug: a subscription teardown clears only its
own flag and leaves individually-held `direct` watches — possibly on entirely different
properties — untouched.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Per-property watch keys | Per-service keys | A service carries several properties; per-service would emit for all of them |
| Cache updates regardless of watch | Only cache watched properties | Keeps `get()` useful right after `watch()` and lets an unwatched property be read without a fetch |
| Refcounted holds | Presence-only `HashSet` | One watcher releasing must not silence others holding the same pair; the set made the first drop win. Still required now that `watch()` need not be called per frame — overlapping holds arise from independent watchers, and the SDK's own `WatchHandle` tests cover exactly this |
| Split `subscription` flag + `direct` count | A single `usize` covering both | Guard holds are cleared in bulk, individual holds one at a time. One counter must either leak or over-release |
| `saturating_sub` on release | `panic!` / `debug_assert!` on over-release | An unbalanced release is a caller bug, but wrapping to `usize::MAX` would silently resurrect the watch forever — strictly the worse failure. Over-release is a no-op |

### 4.3 Feature: coordinator resolution

#### What

For `PerCoordinator` speaker-scoped properties, reads *and* SDK writes redirect to the
coordinator, non-coordinator events are dropped, and watching members are notified when the
coordinator changes.

#### Why

Sonos gives the group's coordinator authority over playback. Members still emit AVTransport
events, but with empty or default values. Storing those would clobber real data. And a member
must still be able to report the group's `PlaybackState` when asked.

#### How

Four cooperating pieces, all keyed off the same predicate:

```rust
// src/state.rs — reads redirect
pub(crate) fn get_resolved<P: SonosProperty>(&self, speaker_id: &SpeakerId) -> Option<P> {
    if P::SERVICE.scope() == ServiceScope::PerCoordinator && P::SCOPE == Scope::Speaker {
        let coordinator_id = self.resolve_coordinator(speaker_id);
        self.speaker_props.get(&coordinator_id)?.get::<P>()
    } else {
        self.speaker_props.get(speaker_id)?.get::<P>()
    }
}

// src/state.rs — SDK writes redirect, by the same rule
pub(crate) fn resolve_write_target<P: SonosProperty>(&self, speaker_id: &SpeakerId) -> SpeakerId {
    if P::SERVICE.scope() == ServiceScope::PerCoordinator && P::SCOPE == Scope::Speaker {
        self.resolve_coordinator(speaker_id)
    } else {
        speaker_id.clone()
    }
}
```

- **Event writes**: the worker drops `PerCoordinator` events from non-coordinators
  (`src/event_worker.rs`).
- **SDK writes**: `set_property()` routes through `resolve_write_target()`.
- **Notifications**: `notify_group_members()` (`src/event_worker.rs`) emits a
  `ChangeEvent` per watching member and copies nothing.
- **Subscriptions**: `resolve_subscription_target()` points the member's subscription at the
  coordinator's IP.

`resolve_coordinator()` returns the speaker's own ID when no group data exists, so a standalone
speaker and a not-yet-known speaker both behave correctly.

**Why writes must resolve too.** `set_property()` used to write the raw `speaker_id`, which
made the write and the read disagree for exactly the properties this feature exists for. The
SDK calls `set_property()` right after a successful SOAP action so the cache reflects the change
without waiting for an event (`sonos-sdk`'s `play()`, `pause()`, `stop()`, and `fetch()`); on a
*grouped member* that value landed in the member's own bag, while `get_property()` resolved to
the coordinator's. The optimistic update was written somewhere nothing reads — `play()` on a
grouped speaker left `playback_state.get()` reporting the old state until a real event arrived.
`fetch()` (`sonos-sdk/src/property/handles.rs`) had already worked around this by resolving the
target itself before calling `set_property`; moving the resolution into `set_property` makes
every caller correct and leaves `fetch()`'s own call redundant-but-harmless (it resolves to the
coordinator, and resolving twice is idempotent).

Resolution happens *inside* the store write lock, not in a separate read beforehand: taking the
coordinator under one lock and writing under another leaves a window in which a topology event
regroups the speaker and the write lands in the wrong bag.

**Notification keying.** A resolved write emits for the requesting speaker *and*, when they
differ, for the coordinator. Keying only on the coordinator would leave a member that watches
the property unnotified by its own write; keying only on the requester would leave the
coordinator's watchers unnotified about a change to their own bag.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Resolve on read | Copy coordinator values into each member's bag | One source of truth; regrouping needs no cache fix-up |
| Resolve writes with the same predicate | Leave `set_property` raw and fix each caller | Callers cannot see the read path's rule; one already got it right and the rest silently did not. Symmetry belongs where both halves are defined |
| `resolve_write_target` as a `StateStore` method | Inline the branch in `set_property` | The read branch and the write branch must not be able to drift apart; one named mirror of the other makes divergence visible |
| Resolve inside the write lock | Resolve via a read lock, then write | A regrouping between the two would send the write to the wrong bag |
| Emit for both requester and coordinator | Emit for one of them | Watchers exist on both sides and neither may be dropped |
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

`TryIter` and `TimeoutIter` are thin borrows over *one* `ChangeIterator`'s own queue, so no
variant takes ownership and all can be used against one iterator. They are views, not new
subscribers: they consume the same events `recv()` would. Call `iter()` again for an independent
stream — see 4.1b.

### 4.6 Feature: per-event panic containment

#### What

`run_event_loop` (`src/event_worker.rs:69`) wraps the body for each event — and only the body,
never the loop — in `std::panic::catch_unwind`. A panic logs at `error!` with the event's IP and
service, increments a per-worker counter, and the loop moves to the next event.

#### Why

The worker is a bare `thread::spawn` whose `JoinHandle` is never joined (`_worker` on
`StateManager`). Without this guard, a single panic anywhere in decoding — a slice index, an
arithmetic overflow in a debug build, a `unwrap` on a malformed field — terminated the thread
and with it *every* subsequent state update for the whole process. There was no log, no `Err`,
and no panic surfacing to the user; watches simply went quiet forever and a TUI kept rendering
its last known values as though the household had frozen. That failure mode is strictly worse
than dropping one event, and it is exactly the failure mode a long-running dashboard cannot
detect.

#### How

Two facts make recovery sound rather than merely optimistic:

- **`parking_lot::RwLock` does not poison.** Unlike `std::sync::RwLock`, a guard dropped during
  unwind simply releases the lock; there is no poisoned flag and no `PoisonError` on the next
  acquisition. So the store, the watched set, and the IP map are all still usable after a panic.
  This is the non-poisoning property already listed as a P1 design goal in 1.2, now load-bearing.
- **Events are independent.** `PropertyChange::apply` takes the write lock per change, so an
  aborted event leaves the store partially updated but internally consistent, and the next event
  for that service overwrites it. Nothing spans two events.

`catch_unwind` requires `UnwindSafe`, which `&Arc<RwLock<..>>` is not (interior mutability), so
the closure is wrapped in `AssertUnwindSafe`. The assertion is justified by the two facts above
and stated in a comment at the call site rather than left implicit.

**Guarding against masked bugs**: `catch_unwind` can turn a crash into a slow leak of dropped
updates, so panics are never swallowed. Every panic logs at `error!` individually, and every
`PANIC_ESCALATION_INTERVAL` (10) panics logs an additional escalated `error!` naming the running
total. There is deliberately no health-check API — the log is the interface.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Wrap the per-event body | Wrap the whole loop | Wrapping the loop only relocates the problem: the first panic still ends event processing |
| Recover and continue | Let the thread die, surface it via `JoinHandle` | Nothing joins the handle, and a sync API has no place to report it; a dropped event is a far smaller loss than a dead worker |
| Count and log loudly | Silent recovery | Recovery must not hide the bug that caused the panic |
| No health-check API | `is_healthy()` / panic count accessor | Out of scope; would add public surface to an internal crate for a condition that should be fixed, not polled |

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

A 12-variant enum, one per decodable property, with `apply`, `key`, `scope`, and `service`. It
exists so a decoded batch can be moved out of the decoder without generics and without holding
the store lock during decode — the worker decodes first, then takes the write lock per change.

Since 0.7.0 it is also the **`ChangeEvent` payload**, reached through `SonosProperty::to_change()`
for values written outside the decoder (local actions, `fetch()` results). That dual role is
deliberate: one closed, typed representation of "a property took this value", whether it came
from a NOTIFY or from a SOAP write.

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
| DIDL-Lite XML | Track metadata | `quick-xml` + `serde` | `parse_track_metadata` deserializes a private `DidlLite`/`DidlItem` pair. See 5.3a |
| Device/topology URLs | Speaker IP from a `location` value | `url` | `extract_ip_from_location` (`src/decoder.rs`); see 10.3 |
| `HH:MM:SS[.mmm]` | Positions and durations | Hand-rolled | `parse_duration_ms` (`src/decoder.rs`); rejects `NOT_IMPLEMENTED` and returns `None` on overflow. Kept hand-rolled: this is not a standard duration format any crate parses |

`Scope` and `SonosProperty` are deliberately not serializable — they are compile-time metadata.

#### 5.3a `parse_track_metadata`: a frozen 4-tuple and a two-stage parse

**The signature is a deliberately frozen cross-crate contract.**

```rust
pub fn parse_track_metadata(
    metadata: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>, Option<String>)
//     title           artist          album           album_art_uri
```

`sonos-sdk` destructures the tuple positionally (`sonos-sdk/src/property/handles.rs`), so
the arity, order and infallibility are all load-bearing. Two consequences:

- **Do not make it fallible.** A track whose metadata will not parse must read as "unknown
  track" — never `Err`, never a panic. Metadata arrives from streaming services and
  third-party firmware; a single odd title must not take down the caller's display.
  Returning `Result` would push a decision onto every call site that has only one sensible
  answer anyway.
- **Adding a field means changing every caller.** If a fifth value is ever needed, return a
  named struct rather than a 5-tuple.

**Why a strict parse then a lenient retry.** quick-xml's `unescape` rejects any entity it
does not recognize (`UnrecognizedSymbol`). Real DIDL routinely carries bare `&` in titles
and the occasional HTML entity like `&nbsp;`, so a strict-only parse would blank out
tracks that render fine today — a regression, not a fix. So:

1. **Strict** `quick_xml::de::from_str` first. Valid DIDL is the common case and this costs
   no allocation.
2. On failure, retry against a copy where `escape_stray_ampersands` has escaped every `&`
   that does not already begin a well-formed character reference. `&` becomes `&amp;`, and
   an unknown `&nbsp;` becomes the literal text `&nbsp;` — exactly what the old
   `.replace()` chain produced, so behaviour on real-world input is preserved.
3. Only if *that* also fails (malformed markup, not merely a bad entity) is the item
   dropped and all-`None` returned, with the reason at `debug`.

**The bug this replaced.** The previous implementation unescaped by chaining `.replace()`
calls with `&amp;` **first**, so `&amp;apos;` decoded to `'` instead of the literal
`&apos;` — double-escaped input silently lost a level. It also could not see CDATA,
comments, or a matching tag name inside an attribute value. Ordering bugs of that shape are
exactly why unescaping now belongs to the parser.

**Field names are element *local* names.** quick-xml resolves namespace prefixes, so
UPnP's `dc:title` deserializes as `title` and `upnp:albumArtURI` as `albumArtURI`;
`rename = "dc:title"` could never match. `dc:creator` is authoritative for artist, with
`r:albumArtist` as the fallback Sonos supplies for library tracks carrying no creator.
The private `DidlItem` here duplicates most of `sonos_api::events::DidlItem`, which lacks
`albumArtist`; consolidating the two DIDL models is worthwhile follow-up work.

#### 5.3b `extract_ip_from_location`: why `url::Url`

Topology members are addressed as `http://<ip>:1400/xml/device_description.xml`. The IP is
pulled out with `url::Url` rather than splitting on `"http://"`, `'/'` and `':'`, because
hand-splitting got two cases wrong:

- **IPv6 literals are bracketed** (`http://[fe80::1]:1400/...`), so splitting on `':'`
  truncated the host to `"[fe80"` and yielded `None`.
- **Userinfo, or a host with no port**, shifted whichever segment the naive split picked.

`Url::parse` also rejects a scheme-less string as a relative URL, which preserves the
previous `strip_prefix("http://")` behaviour of returning `None` for
`"192.168.1.1:1400/xml"`. A host that is a *name* rather than a literal address still
returns `None`: this value is a cache key for `ip_to_speaker`, not something to resolve.

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
| `sonos-sdk` | Sole consumer. Wraps `StateManager` in property handles; `system.iter()` (`sonos-sdk/src/system.rs:531`) returns our `ChangeIterator` | Internal crate — signatures may change with the SDK |

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

Derived with `thiserror`: `#[derive(thiserror::Error)]` plus one `#[error("...")]` per
variant. The hand-written `Display` and `Error::source` impls this crate used to carry
were replaced with derives producing **byte-identical messages**, so nothing downstream
that matches on error text changed. Only `Api` exposes a `source`, via `#[from]`, which
also supplies the `From<ApiError>` conversion the `?` operator needs. A unit test pins
both the message and the presence of the source, so an accidental `#[error]` reword or a
dropped `#[from]` fails the build rather than silently changing observable behaviour.

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

106 inline unit tests, no `tests/` directory and no network access. Everything the crate does
is a pure function over in-memory state plus one channel, so behaviour is testable by
constructing a store, applying changes, and asserting on both the store and the channel.

```
                    +-------------------+
                    | Live verification |  examples/minimal_example.rs (manual)
                    +--------+----------+
           +-----------------+------------------+
           |    Worker + store integration      |  event_worker.rs (17)
           +-----------------+------------------+
    +------+------+------+------+------+------+------+
    |               Unit tests                        |  state 36, decoder 27,
    +-------------------------------------------------+  property 15, iter 7, model 2, speaker 2
```

### 8.2 Unit Tests

**Location**: inline `#[cfg(test)] mod tests` in each source file.

**What is covered**:
- [x] Constructor clamping — `test_volume_clamping` (`src/property.rs:527`), `test_bass_clamping` (:534)
- [x] Property metadata constants — `test_property_constants` (`src/property.rs:601`)
- [x] Per-service decoding — `test_decode_rendering_control` (`src/decoder.rs:640`), `test_decode_av_transport` (:675), `test_decode_group_rendering_control` (:706)
- [x] Topology decode incl. IPs, satellites, `boot_seq` — `src/decoder.rs:588`, :1026, :1059
- [x] `PropertyChange` key/service/scope mapping — `src/decoder.rs`
- [x] Queued events preserve every intermediate value —
      `test_queued_events_preserve_every_intermediate_value` (`src/state.rs`) queues
      `Playing -> Transitioning -> Playing` before draining and asserts all three are observed,
      while the store holds only the last. Impossible to satisfy before 0.7.0
- [x] Monotonic write guard — `test_stale_fetch_does_not_clobber_newer_event_value` proves a
      `fetch()` observed before a stored event is rejected *and* that the newer value survives;
      `test_newer_write_is_accepted_after_an_earlier_one` proves the guard is not simply
      "reject everything after the first write"
- [x] Event payload correctness at the SDK boundary —
      `test_sdk_change_event_carries_value_and_source` (`sonos-sdk/src/property/handles.rs`)
- [x] Group fan-out payload — `test_per_coordinator_notifies_members_without_data_copy` asserts
      the member's event carries the coordinator's value
- [x] Channel semantics — `test_channel_closed` (`src/iter.rs:269`), `test_try_iter` (:233)
- [x] Coordinator resolution — `test_get_resolved_per_coordinator_reads_from_coordinator` (`src/state.rs:1687`), `test_get_resolved_per_speaker_reads_own_props` (:1735)
- [x] Watch gating — `test_change_event_emission` (`src/state.rs:1040`), `test_set_group_property_no_event_when_unwatched` (:1116)
- [x] Registry unregistration — `test_state_watch_registry_register_and_unregister`
- [x] Watch refcounting — `test_watch_refcount_survives_partial_release` proves *n* watchers
      of one property survive *n-1* releases and that an over-release does not resurrect the
      watch; `test_service_unregister_keeps_directly_held_watches` proves a subscription
      teardown leaves individually-held watches — including sibling properties of the same
      service — intact
- [x] Write/read symmetry — `test_set_property_on_group_member_is_readable_from_both` writes a
      `PerCoordinator` speaker-scoped property through a *group member* and asserts it is
      readable from both the member and the coordinator, then asserts a `PerSpeaker` write is
      *not* redirected
- [x] IP updates — `test_update_speaker_ip` (`src/state.rs:1783`)
- [x] Duration overflow — `test_parse_duration_ms_overflow_returns_none` (`src/decoder.rs:507`)
      proves `parse_duration_ms` returns `None` instead of panicking on components that
      overflow `u64`
- [x] Garbage position — `test_decode_av_transport_skips_position_when_rel_time_garbage`
      (`src/decoder.rs:515`) proves no `Position` change is emitted when `RelTime` will not
      parse, so 0:00 never masquerades as a real reading

### 8.3 Component Tests

`src/event_worker.rs:443` exercises the worker's helpers directly against a real
`StateStore`, a real `watched` set, and a real `mpsc` pair — no mocking, since all three are
cheap to construct.

Notable cases: `test_apply_property_change_with_watch` (`:496`) asserts an event fires only
when watched; `test_apply_topology_changes_no_event_when_membership_unchanged` (`:1061`) pins
the change-detection gate; `test_per_coordinator_notifies_members_without_data_copy` (`:1114`)
asserts the member is notified *and* that nothing was written to its bag;
`test_per_speaker_service_not_notified` (`:1245`) asserts the inverse for `PerSpeaker`
services.

`test_partial_topology_event_does_not_clear_groups` (`:932`) is the regression test for 3.2's
empty-snapshot guard: it seeds a group plus a group-scoped property, applies a
`TopologyChanges` with no groups, and asserts the group, its `group_props`, and its
`speaker_to_group` entry all survive with no notification emitted.

`test_worker_survives_decoder_panic` (`:991`) drives `run_event_loop` directly with two
events — one that panics, one valid — and asserts the valid event's `ChangeEvent` still
arrives and its value reached the store. The panic is injected through a `#[cfg(test)]`
sentinel IP (`PANIC_TRIGGER_IP`) checked at the top of `handle_event`, deliberately in place of
a permanent fault-injection API on an internal crate. A panic backtrace on stderr during this
test is expected output, not a failure.

### 8.4 Integration Tests

`sonos-state/examples/minimal_example.rs`, run by hand against real hardware. Requires a
Sonos speaker on the LAN and a firewall that permits UPnP callbacks (or accepts the polling
fallback).

### 8.5 Test Fixtures & Mocks

| Dependency | Strategy | Location |
|------------|----------|----------|
| `StateStore` | Real instance | `StateStore::new()` inline |
| `SpeakerInfo` | Local factory functions | `create_test_speaker_info()` in the relevant test module |
| `ChangeEvent` channel | Real `mpsc::channel()` | Assert on `try_recv()` and on `event.change` |
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
2. **Worker apply loop** (`src/event_worker.rs:219`) — takes the store write lock *per
   change* rather than once per event. Simpler and it shortens the window the render loop can
   be blocked, at the cost of re-locking a few times per event.
3. **`notify_group_members`** (`src/event_worker.rs:387`) — O(members x changes) with the
   `watched` read lock held. Bounded by real group sizes.
4. **Topology apply** (`src/event_worker.rs:284`) — the single largest write-lock hold: it
   rebuilds all groups under one lock. Frequency is low (regrouping is user-driven), so the
   duration is acceptable.

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| Worker thread | First `set_event_manager()` | When all `event_tx` clones drop and the event-manager iterator ends | One per `StateManager` |
| `PropertyBag` entry | First `set` for that speaker | With the speaker | One per `(entity, property type)` |
| `watched` hold | `register_watch` (counted) / `WatchGuard` acquisition (flag) | `unregister_watch` releases one count; `unregister_watches_for_service` clears the flag after the grace period. Entry removed at zero holds | Shared across `StateManager` clones |
| UPnP subscription | Delegated | Delegated | `sonos-event-manager` |

`StateManager` has no `Drop` impl. Shutdown is by channel closure: dropping the last clone
drops the last `event_tx`, the worker's next send fails, and the thread ends when the
event-manager iterator terminates.

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Forged UPnP event on the LAN | Low | Medium (wrong displayed state) | Events from IPs absent from `ip_to_speaker` are dropped (`src/event_worker.rs:159`) |
| Event flooding | Low | Low | Unbounded `mpsc` grows but never blocks the worker; unwatched properties never enqueue at all |
| Malformed XML in track metadata | Medium | Low | `parse_track_metadata` is infallible: a real XML parser (`quick-xml`) with a lenient retry, and all-`None` if both attempts fail. No panic, no error propagated to the display path (5.3a) |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Speaker IPs and UUIDs | Low (LAN-local) | Logged at `debug`/`warn`, not `info` |
| Track metadata | Low | No special handling |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| `Device.ip_address` | Must parse as `IpAddr` | `src/state.rs:419` |
| Event source IP | Must be a known speaker | `src/event_worker.rs:150` |
| Volume strings | `parse::<u8>()`, then `.min(100)` | `src/decoder.rs:206` |
| Group volume | `.min(100)` | `src/decoder.rs:315` |
| Durations | Exactly three `:`-separated parts, and checked arithmetic so an overflowing component yields `None` | `src/decoder.rs` (`parse_duration_ms`) |
| Topology `location` | Parsed with `url::Url`; the host must be a literal IPv4 or IPv6 address | `src/decoder.rs` (`extract_ip_from_location`) |
| Track metadata | Real XML parse, infallible by contract | `src/decoder.rs` (`parse_track_metadata`); see 5.3a |

---

## 11. Observability

### 11.1 Logging

| Level | What's Logged | Example |
|-------|--------------|---------|
| `warn` | Unknown speaker IP, failed subscribe/unsubscribe, event-manager device registration failure | "Received event from unknown speaker IP" (`src/event_worker.rs:159`); empty topology snapshot (`:270`); unmappable group-scoped change (`src/decoder.rs:113`) |
| `info` | Manager creation, worker start/stop, speaker IP changes | "State event worker started" (`src/event_worker.rs:39`); `error` is reserved for contained panics (`:97`) |
| `debug` | Per-event receipt, decode counts, per-change application, emissions | "Decoded {} property changes from event" (`src/event_worker.rs:213`); skipped Position and coordinator-lookup misses |
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
| 0.7.0 | **Breaking**: `ChangeEvent` carries the value as `PropertyChange`; `property_key`/`service` become methods; `source`/`timestamp` reflect observation. Monotonic write guard (`WriteStamp`, `WriteOutcome`, `ChangeSource`); `StateStore::set*` and `PropertyChange::apply` take a stamp and return `WriteOutcome` |
| 0.5.x–0.6.x | Sync-first design: `std::thread` worker, `mpsc` notifications, valueless `ChangeEvent`, coordinator resolution, lazy `EventInitFn` |
| 0.2.1 | `StateWatchRegistry` implementing `WatchRegistry`; moved to `parking_lot::RwLock` |
| 0.1.0 | Initial release |

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Properties start as `None` | First `get()` before any event returns nothing | Use the SDK's `fetch()` or `watch_or_fetch()` | — |
| `DeviceProperties` and `GroupManagement` decode to empty | No properties from those services | — | Tracked in `docs/STATUS.md` |
| Unbounded notification queues, now one per subscriber | A never-draining consumer grows memory, and each extra `iter()` adds its own queue plus a `ChangeEvent` clone (~168 bytes, plus heap strings for `CurrentTrack`) per event. Nothing is dropped, which is the intended tradeoff | Drain, or use `try_iter()` per frame; drop iterators you no longer read | Bounded queues with a *detectable* drop policy — a silent drop would reintroduce the bug 4.1b fixed |
| A `ChangeIterator` receives only events emitted after it was created | Taking an iterator after a write misses that write; there is no replay | Subscribe before the writes you want to observe; read current state from `get_property()` | None planned — see 4.1b trade-offs |
| `cleanup_timeout` unused | Builder option has no effect | Ignore it | Remove or wire through |
| `system_props` write-only in practice | `Topology` is stored by `initialize()` (`src/state.rs:681`) but has no public system-scoped getter | Use `groups()` / `speaker_infos()` | Add a system-scope accessor |
| An empty `ZoneGroupTopology` snapshot cannot express "no groups" | A hypothetical genuine all-groups-dissolved event would be ignored (`src/event_worker.rs:270`) | None needed — Sonos always reports at least one single-member group per speaker | Diff against the previous snapshot instead of replacing |
| Contained panics drop the event that caused them | A recurring panic silently loses updates for one service while the rest keep working | Watch the `error!` log; the escalated line names the running total | Fix the panicking decode path; there is deliberately no health-check API |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| Two overlapping watch paths: `watch_property_with_subscription` vs. the SDK's guard-based `acquire_watch` | `src/state.rs:610`, `:636` | Medium | Remove the pre-guard path once nothing depends on it |
| Watch holds are split across two crates: `WatchHolds.subscription` is a flag here because the real per-guard count lives in `sonos-event-manager`'s `service_refs`, keyed `(ip, service)` rather than `(speaker, key)` | `src/state.rs` (`WatchHolds`), `sonos-event-manager/src/manager.rs` | Low | Have `WatchGuard::drop` release its own `(speaker, key)` hold directly, so one counter covers both kinds and the flag can go |
| Unconstructed `StateError` variants | `src/error.rs:10` | Low | Prune to the variants actually produced |
| ~~Hand-rolled XML extraction while `sonos-stream` already depends on `quick-xml`~~ | ~~`src/decoder.rs`~~ | — | **Resolved 2026-08-17.** `extract_xml_element` deleted; `parse_track_metadata` now deserializes with `quick-xml` (5.3a) and `extract_ip_from_location` parses with `url` (5.3b). What remains is the duplicate DIDL model shared with `sonos_api::events::DidlItem` |
| Two DIDL-Lite models: the private one here and `sonos_api::events::DidlItem` | `src/decoder.rs`, `sonos-api/src/events/xml_utils.rs` | Low | The api one lacks `albumArtist`, which the artist fallback needs. Add it there and drop the local copy |
| `software_version` hardcoded to `"unknown"` | `src/state.rs:437` | Low | Read from the device description |
| Write lock retaken per change inside one event | `src/event_worker.rs:423` | Low | Batch under one lock if profiling shows it matters |
| Panic containment is a net, not a fix | `src/event_worker.rs:97` | Low | Any `error!` from it marks a real bug to be fixed at its source |

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
  Now materially harder than it looked: events carry values, so collapsing two events on one
  `(speaker_id, property_key)` *discards an observed value* — exactly what 4.1 exists to
  prevent. Any coalescing would have to be opt-in per consumer, not a channel-level default.
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
| Watched set | `HashMap<(SpeakerId, &'static str), WatchHolds>` gating notification emission; an entry exists while any watcher holds the pair (4.2) |
| ChangeEvent | Notification that a watched property changed, carrying the new value as a `PropertyChange` |
| WriteStamp | `{observed_at, source}` recording when a value was observed, used to reject out-of-order writes |
| ChangeSource | Provenance of a value: `Event` (device NOTIFY), `LocalAction` (post-action write), `Fetch` (SOAP read) |
| Coordinator | The speaker owning playback state for its group |
| Satellite | A speaker marked `Invisible="1"` in topology (surround, sub) |
| Event worker | The `std::thread` draining `SonosEventManager::iter()` |
| Partial topology event | A `ZoneGroupTopology` NOTIFY that carries no `ZoneGroupState`, so it decodes to zero groups and is ignored rather than applied (3.2) |

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
| 2026-08-15 | Claude Opus 5 | Documented the empty-topology-snapshot guard (3.2), per-event panic containment (4.6 and step 2 of 3.1), the "degrade loudly" rule in 3.4, and checked duration arithmetic; refreshed line references and test counts |
| 2026-08-17 | Claude Opus 5 | Hand-rolled XML and URL handling replaced by crates. Added 5.3a (`parse_track_metadata`'s frozen 4-tuple contract, the strict-then-lenient parse strategy, and the `&amp;`-ordering unescape bug it fixed) and 5.3b (why `url::Url` for `location`); updated 5.3, 10.1, 10.3 and the 14.2 debt rows; `StateError` is now `thiserror`-derived with byte-identical messages |
| 2026-08-16 | Claude Opus 5 | Recorded that SDK `WatchHandle`s read through `get_property()` live (3.3), and dropped the re-watch-per-frame framing from 4.2 — overlapping holds now come from independent watchers, not from a documented per-frame loop |
| 2026-08-15 | Claude Opus 5 | `watched` became reference-counted `WatchCounts` so releasing one watcher no longer silences its siblings (4.2), and `set_property()` now resolves `PerCoordinator` writes to the coordinator so writes land where reads look (4.3) |
