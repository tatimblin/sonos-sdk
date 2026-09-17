# sonos-sdk Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

The lower-level crates in this workspace each solve one problem well and none of them alone.
`sonos-discovery` finds speakers, `sonos-api` speaks UPnP/SOAP, `sonos-state` caches values and
announces changes, `sonos-event-manager` reference-counts subscriptions, `sonos-stream` delivers
events and falls back to polling. Building an application directly on those five means:

1. **Discovering devices and tracking their IPs**, including across a DHCP move.
2. **Constructing and validating a typed operation** for every read and every command.
3. **Wiring the state cache** so a fetched value is visible to the rest of the process.
4. **Owning subscription lifetime** — knowing which UPnP service backs which property, when to
   subscribe, and when it is safe to tear down.
5. **Keeping those four in agreement**, especially around groups, where the coordinator owns
   playback state for every member.

`sonos-sdk` is the single crate an application depends on. It exposes a DOM-like surface —
`speaker.volume.get()`, `speaker.volume.fetch()`, `speaker.volume.watch()`, `speaker.play()` —
and owns the coordination behind it.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Fully synchronous API | Every public method is a plain `fn`. The target consumer is a blocking render loop or a CLI; neither should need a runtime handle or an `.await` |
| P0 | DOM-like property access | `speaker.volume` is a field carrying `get`/`fetch`/`watch`, so the three ways to read one property are discoverable from one place and cannot drift apart |
| P0 | Read and control in one surface | Reading volume and setting it belong on the same type. Properties cover reads; methods on `Speaker` and `Group` cover commands |
| P0 | Correct under grouping | A grouped member must report its coordinator's playback state, and a command sent to a member must land where the reads look |
| P1 | Pay only for what you use | A fetch-only program creates no subscriptions, no callback server, no runtime and no threads. Event infrastructure is built on the first `watch()` |
| P1 | RAII subscription lifetime | Holding a `WatchHandle` holds a subscription; dropping it releases one. There is no `unwatch()` to forget |
| P1 | No leaks across construction | Dropping a `SonosSystem` releases the state manager, the event-worker thread, the runtime and the callback socket |
| P2 | Offline-constructible for tests | Consumers must be able to build a fully-formed system from a synthetic device list with provably zero network I/O |

### 1.3 Non-Goals

- **Low-level UPnP access.** Operations that the SDK does not surface are reached through
  `sonos-api` directly. This crate does not aim to expose all 52 of them.
- **Manual subscription management.** Ref counting, grace periods, renewal and polling fallback
  live in `sonos-event-manager` and `sonos-stream`. The SDK says which service a property needs
  and holds a guard.
- **Async.** There is no `tokio` dependency, no `async fn` and no `.await` in this crate. A
  tokio runtime exists inside `sonos-event-manager`, created lazily on the first `watch()` and
  never surfaced here.
- **Persistence beyond a discovery cache.** The only thing written to disk is the device list
  (§4.6); property values live for the life of the `SonosSystem`.
- **Content browsing.** There is no ContentDirectory or music-service support.

### 1.4 Success Criteria

- [x] Every public method on `SonosSystem`, `Speaker`, `Group` and the property handles is
      synchronous
- [x] `speaker.volume.get()` returns the cached value with no network call
- [x] `speaker.volume.fetch()` performs one SOAP round trip and updates the shared cache
- [x] `speaker.volume.watch()` returns a `WatchHandle` whose `value()` is live for the handle's
      whole lifetime
- [x] `speaker.play()` and the rest of the control surface write the cache optimistically, so a
      subsequent `get()` reflects the command without waiting for an event
- [x] A `PerCoordinator` property read through a grouped member resolves to the coordinator
- [x] `drop(system)` releases the `StateManager` — asserted through a `Weak` that outlives it
- [x] `SonosSystem::from_devices_offline` performs no network I/O

---

## 2. Architecture

### 2.1 High-Level Design

```
+---------------------------------------------------------------------------+
|                          sonos-sdk (public)                               |
+---------------------------------------------------------------------------+
|  SonosSystem                             src/system.rs:75                 |
|    state_manager: Arc<StateManager>      (shared; sole owner of events)   |
|    api_client:    SonosClient            (clone of the SOAP singleton)    |
|    speakers:      RwLock<HashMap<String, Speaker>>   keyed on room name   |
|    last_rediscovery: AtomicU64           rediscovery cooldown             |
|    offline:       bool                   test constructors only (8.5)     |
+---------------------------------------------------------------------------+
|  Speaker  src/speaker.rs:123        |  Group  src/group.rs:76             |
|    id / name / ip / model_name      |    id / coordinator_id / member_ids |
|    volume, mute, bass, treble,      |    volume, mute,                    |
|    loudness, playback_state,        |    volume_changeable                |
|    position, current_track,         |    coordinator(), members(),        |
|    group_membership                 |    add_speaker(), remove_speaker(), |
|    play/pause/stop/seek/queue/...   |    dissolve(), set_volume(), ...    |
+---------------------------------------------------------------------------+
|  PropertyHandle<P>  src/property/handles.rs:306                           |
|    get()   -> Option<P>          cached read, one RwLock read             |
|    fetch() -> Result<P>          SOAP round trip + stamped cache write    |
|    watch() -> Result<WatchHandle<P>>   RAII lease + live read closure     |
|                                                                           |
|  GroupPropertyHandle<P>  src/property/handles.rs:937   same triad, group  |
+---------------------------------------------------------------------------+
                 |                    |                    |
                 v                    v                    v
+----------------------+  +----------------------+  +----------------------+
| sonos-state          |  | sonos-api            |  | sonos-discovery      |
| StateManager:        |  | SonosClient:         |  | get_with_timeout():  |
|  cache + fan-out     |  |  execute_enhanced    |  |  SSDP sweep          |
+----------------------+  +----------------------+  +----------------------+
                 |
                 v  (lazily, on first watch())
+---------------------------------------------------------------------------+
| sonos-event-manager -> sonos-stream -> callback-server                    |
|   subscription ref counting, event delivery, polling fallback             |
+---------------------------------------------------------------------------+
```

**Design Rationale**: the crate is a facade. The DOM-like shape — a property as a *field* that
carries its own verbs — was chosen over `speaker.get_property::<Volume>()` because it makes the
API navigable by autocomplete, and because it puts `get`, `fetch` and `watch` for one property
in one type where they cannot acquire inconsistent semantics.

Everything is synchronous because the two layers below the SDK already are: `sonos-api` blocks
on `ureq`, and `sonos-state` is a `std::thread` plus `std::sync::mpsc`. The async machinery
that does exist is confined to `sonos-event-manager`'s worker thread, which owns its own
current-thread runtime. A caller never sees it.

### 2.2 Module Structure

```
sonos-sdk/src/
├── lib.rs              # Public surface and re-exports
├── prelude.rs          # pub mod prelude — the common subset
├── system.rs           # SonosSystem: discovery, registry, groups, iter()  (private mod)
├── speaker.rs          # Speaker, SeekTarget, PlayMode                     (private mod)
├── group.rs            # Group, GroupChangeResult                          (private mod)
├── error.rs            # SdkError                                          (private mod)
├── cache.rs            # Discovery cache on disk                           (private mod)
└── property/           # pub mod property
    ├── mod.rs          # Re-exports every handle type
    └── handles.rs      # PropertyHandle, GroupPropertyHandle, WatchHandle,
                        #   Fetchable / FetchableWithContext / GroupFetchable,
                        #   SpeakerContext, GroupContext, type aliases
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `system` | Construction, discovery, speaker/group registry, change iteration | `mod` (private); `SonosSystem` re-exported at `src/lib.rs:75` |
| `speaker` | `Speaker` plus the AVTransport and RenderingControl command surface | `mod` (private); types re-exported at `src/lib.rs:74` |
| `group` | `Group` plus group lifecycle and GroupRenderingControl commands | `mod` (private); types re-exported at `src/lib.rs:73` |
| `error` | `SdkError` | `mod` (private); re-exported at `src/lib.rs:72` |
| `cache` | Disk cache of discovered devices | `mod` (private); nothing is `pub` |
| `property` | Handle types and fetch traits | `pub mod` (`src/lib.rs:126`) |
| `prelude` | The subset most programs need | `pub mod` (`src/lib.rs:120`) |

Only `property` and `prelude` are public modules. Everything else reaches the user through
re-exports, so the module layout is free to change without breaking callers.

`Fetchable`, `FetchableWithContext` and `GroupFetchable` are deliberately **not** re-exported
at the crate root: they are the extension points for adding a property to this crate, not part
of a consumer's vocabulary. They are reachable as `sonos_sdk::property::Fetchable`.

### 2.3 Key Types

#### `SonosSystem` (`src/system.rs:75`)

```rust
pub struct SonosSystem {
    state_manager: Arc<StateManager>,            // :85
    api_client: SonosClient,                     // :88
    speakers: RwLock<HashMap<String, Speaker>>,  // :91  keyed on display name
    last_rediscovery: AtomicU64,                 // :94
    offline: bool,                               // :102 test constructors only
}
```

**Purpose**: the entry point. Discovers devices, owns the shared `StateManager` and SOAP client,
and hands out `Speaker` and `Group` handles.

It derives nothing — it is not `Clone`, not `Debug`, not `Default`. `new()` returns
`Result<Self, SdkError>`.

**Invariants**:
- Every visible discovered device has an entry in `speakers`; satellites do not (§3.1)
- The `StateManager` knows every discovered device
- The event manager is unset until the first `watch()` triggers lazy initialisation
- Dropping a `SonosSystem` releases its `StateManager` (§8.7)

**Ownership**: created once per application. `Speaker` and `Group` handles clone the
`Arc<StateManager>` and the `SonosClient`, so they are cheap and independent of the system's
own lifetime for reads — but the event infrastructure lives and dies with the manager.

#### `Speaker` (`src/speaker.rs:123`)

```rust
#[derive(Clone)]
pub struct Speaker {
    pub id: SpeakerId,            // :126
    pub name: String,             // :128  display name, prefers room_name
    pub ip: IpAddr,               // :130
    pub model_name: String,       // :132

    // RenderingControl
    pub volume: VolumeHandle,                 // :138
    pub mute: MuteHandle,                     // :140
    pub bass: BassHandle,                     // :142
    pub treble: TrebleHandle,                 // :144
    pub loudness: LoudnessHandle,             // :146

    // AVTransport
    pub playback_state: PlaybackStateHandle,  // :152
    pub position: PositionHandle,             // :154
    pub current_track: CurrentTrackHandle,    // :156

    // ZoneGroupTopology
    pub group_membership: GroupMembershipHandle,  // :162

    context: Arc<SpeakerContext>,             // :165
}
```

**Purpose**: one speaker, with nine property handles and the full command surface.

**Invariants**:
- Every handle shares the one `Arc<SpeakerContext>`, so they cannot disagree about identity,
  address, state manager or client
- `ip` is refreshed from the store during construction and after a topology update

**Ownership**: `Clone`. Cloning clones four `Arc`s and a `SonosClient` (itself an `Arc` clone).

#### `Group` (`src/group.rs:76`)

```rust
#[derive(Clone)]
pub struct Group {
    pub id: GroupId,                   // :79
    pub coordinator_id: SpeakerId,     // :81
    pub member_ids: Vec<SpeakerId>,    // :83

    pub volume: GroupVolumeHandle,                      // :89
    pub mute: GroupMuteHandle,                          // :91
    pub volume_changeable: GroupVolumeChangeableHandle, // :93

    coordinator_ip: IpAddr,            // :96
    state_manager: Arc<StateManager>,  // :97
    api_client: SonosClient,           // :98
}
```

**Purpose**: a zone group. Every command it issues targets `coordinator_ip`, because the
coordinator is the only member with authority over group-wide state.

`Group::from_info` (`src/group.rs:106`) is `pub(crate)`: a `Group` is always derived from the
topology the `StateManager` holds, never constructed by a caller from parts that might not
correspond to a real group.

Note the deliberate type asymmetry with `Speaker`: `Group::set_volume` takes `u16` and
`set_relative_volume` takes `i16`, matching the GroupRenderingControl UPnP arguments, where the
per-speaker equivalents take `u8`/`i8`.

#### `PropertyHandle<P>` (`src/property/handles.rs:306`)

```rust
#[derive(Clone)]
pub struct PropertyHandle<P: SonosProperty> {
    context: Arc<SpeakerContext>,  // :308
    _phantom: PhantomData<P>,      // :309
}
```

**Purpose**: the get/fetch/watch triad for one property on one speaker. The nine `Speaker`
fields are type aliases over it (`src/property/handles.rs:870-894`), so there is exactly one
implementation of each verb regardless of property.

| Method | Line | Cost |
|--------|------|------|
| `get(&self) -> Option<P>` | :334 | One read lock on the state store, one clone |
| `watch(&self) -> Result<WatchHandle<P>, SdkError>` | :366 | Lazy event init on first call; then a ref-count increment |
| `is_watched(&self) -> bool` | :498 | One read lock |
| `speaker_id()` / `speaker_ip()` | :505 / :510 | Field reads |
| `watch_or_fetch(&self) -> Result<WatchHandle<P>, SdkError>` | :525 | `watch()` plus, if the store is empty, one `fetch()` — requires `P: Fetchable` |
| `fetch(&self) -> Result<P, SdkError>` | :556 | One SOAP round trip — requires `P: Fetchable` |

`GroupMembership` gets a concrete `fetch()` (`:617`) rather than a generic one, because Rust
forbids two generic impl blocks defining the same method and that property is
`FetchableWithContext` rather than `Fetchable`.

`GroupVolumeChangeable` has no `fetch()` at all (explained at `src/property/handles.rs:858`):
the value only ever arrives on an event.

**`SpeakerContext`** (`:25`) holds `speaker_id`, `speaker_ip`, `state_manager` and `api_client`;
`SpeakerContext::new` (`:35`) returns `Arc<Self>` directly, because nothing ever wants an
unshared one. `GroupContext` (`:904`) is its group equivalent, additionally carrying
`group_id` and `coordinator_id`.

#### `WatchHandle<P>` (`src/property/handles.rs:129`)

```rust
#[must_use = "dropping the handle starts the grace period — hold it to keep the subscription alive"]
pub struct WatchHandle<P> {
    read: Box<dyn Fn() -> Option<P> + Send + Sync>,  // :138
    mode: WatchMode,                                 // :139
    _cleanup: WatchCleanup,                          // :140
}
```

**Purpose**: an RAII lease on a subscription that is also a **live view** of the property.

| Method | Line | Behaviour |
|--------|------|-----------|
| `mode()` | :145 | Which delivery mode the watch got |
| `value()` | :158 | Reads the store **on every call** and returns `Option<P>` by value |
| `has_value()` | :166 | The same read, discarding the value |
| `has_realtime_events()` | :171 | `mode() == WatchMode::Events` |

The type parameter is unbounded — a `WatchHandle<P>` carries a closure, not a property
constraint — and the type is not `Clone`, because each handle is exactly one hold.

**Invariants**:
- While the handle lives, `value()` reports the property's current value, including after a
  regrouping the handle knew nothing about
- Dropping the handle releases exactly one hold; the subscription survives if any other holder
  remains, and otherwise enters a 50 ms grace period

#### `WatchMode` (`src/property/handles.rs:58`)

```rust
pub enum WatchMode {
    Events,     // :64  real-time UPnP
    Polling,    // :71  event manager present, polling fallback in force
    CacheOnly,  // :77  no event manager; the watched-set entry is held, nothing subscribes
}
```

`CacheOnly` is what an offline-constructed system produces, and what any system produces before
an `EventInitFn` has been installed. The cleanup for it is a `CacheOnlyGuard`
(`src/property/handles.rs:212`), which releases one reference-counted hold on
`(speaker_id, property_key)` — not the whole entry, so sibling watchers survive.

#### `SdkError` (`src/error.rs:5`)

```rust
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SdkError {
    #[error("state management error: {0}")]
    StateError(#[from] sonos_state::StateError),                       // :7

    #[error("api error: {0}")]
    ApiError(#[from] sonos_api::ApiError),                             // :10

    #[error("event manager error: {0}")]
    EventManager(String),                                              // :13

    #[error("speaker not found: {0}")]
    SpeakerNotFound(String),                                           // :16

    #[error("invalid ip address")]
    InvalidIpAddress,                                                  // :19

    #[error("property watcher closed")]
    WatcherClosed,                                                     // :22

    #[error("property fetch failed: {0}")]
    FetchFailed(String),                                               // :25

    #[error("validation failed: {0}")]
    ValidationFailed(#[from] sonos_api::operation::ValidationError),   // :28

    #[error("invalid operation: {0}")]
    InvalidOperation(String),                                          // :31

    #[error("discovery failed: {0}")]
    DiscoveryFailed(String),                                           // :34

    #[error("internal lock poisoned")]
    LockPoisoned,                                                      // :37
}
```

Eleven variants, and the enum **is** `#[non_exhaustive]` (`src/error.rs:4`). A downstream
`match` must carry a wildcard arm; in exchange, adding a variant is not a breaking change.

---

## 3. Code Flow

### 3.1 Primary Flow: System Initialisation

```
SonosSystem::new()                                    src/system.rs:116
        |
        +-- cache::load() / cache::is_stale()         src/cache.rs:36, :66
        |     hit  -> use the cached device list
        |     miss -> sonos_discovery::get_with_timeout(3s), then cache::save()
        v
from_devices_inner(devices)                           src/system.rs:265
        v
construct(devices, offline = false, seed = noop)      src/system.rs:274
        |
        +-- assemble(devices, offline)                src/system.rs:327   [no I/O]
        |     StateManager::new() + add_devices()
        |     build the EventInitFn, capturing a Weak<StateManager>
        |     SonosClient::new()  (clone of the SOAP singleton)
        |     build_speakers(devices, {} )            src/system.rs:551
        |
        +-- seed(&system)                             (test constructors only)
        |
        +-- ensure_topology()                         src/system.rs:796
        |     SOAP-polls zone_group_topology::state::poll per speaker
        |     until one answers; applies IP updates, then initialize(topology)
        |
        +-- rebuild_speakers_excluding_satellites()   src/system.rs:508
        |     rebuilds the name map now that satellite IDs are known
        |
        +-- refresh each Speaker.ip from the store    src/system.rs:296
```

**Step-by-step**:

1. **Cache or discover** (`src/system.rs:116`): `new()` prefers a non-stale cached device list
   (24-hour TTL, §4.6) and otherwise runs a 3-second SSDP sweep, saving the result.

2. **Assemble** (`src/system.rs:327`): the in-memory half, which performs no network I/O. It
   creates the `StateManager`, registers every device with it, installs the lazy `EventInitFn`,
   clones the SOAP client and builds a provisional speaker map.

3. **Topology** (`src/system.rs:796`): `ensure_topology()` polls `ZoneGroupTopology` state from
   the known speaker IPs until one answers, applies the IP updates it reports, and calls
   `StateManager::initialize(topology)`. Doing this *before* any subscription exists means group
   structure is already known when the first AVTransport event arrives, so coordinator
   suppression and member propagation work from event one.

4. **Re-key** (`src/system.rs:508`): the speaker map is rebuilt, this time excluding satellites.

5. **Refresh addresses** (`src/system.rs:296`): each `Speaker.ip` is re-read from the store,
   because topology may have reported a newer address than discovery did.

`assemble()` and `construct()` are shared by the production constructor and every offline test
constructor, so the Arc wiring and the *order* of the three topology-dependent steps each have
exactly one definition. That order is load-bearing — see below.

**Why satellite exclusion must precede name-keying.** `speakers` is keyed on `display_name()`
(`src/system.rs:24`), which prefers `room_name`. Every device in a bonded home theater — a
Playbar plus its surrounds and sub — reports the *same* `room_name`, so all of them hash to one
key and only one survives insertion. Satellites (`Invisible="1"`) are therefore skipped **inside**
`build_speakers()` (`src/system.rs:551`), before insertion, rather than filtered out of a
finished map.

Filtering afterwards can only inspect whichever device won the collision. If the winner is a
surround, removing it deletes the entire room from `speakers()` — while `groups()`, which reads
topology rather than the name map, still shows that room with a live volume. Skipping first
guarantees the visible coordinator, the one that accepts transport and volume commands, is the
only candidate for the key.

Satellite identity comes from topology and the key comes from the device list, so the map can
only be built correctly once both are known. That point is after `ensure_topology()`, which is
why `construct()` *rebuilds* the map there instead of mutating the provisional one.
`try_rediscover()` (`src/system.rs:637`) re-applies the same exclusion, since it replaces the
map wholesale.

**Two genuinely visible speakers sharing a room name** are disambiguated rather than dropped:
the first keeps the plain room name and later ones are suffixed with their speaker ID
(`"Basement (RINCON_…)"`). Sonos prevents duplicate room names in its own app, so this state
implies something unusual — a rename observed mid-discovery, a stale cache entry for a replaced
unit — but silently discarding a controllable speaker is the worse outcome in every such case.
`speakers()` and `speaker_by_id()` consequently see the true device count.

**Why the init closure captures a `Weak<StateManager>`** (`src/system.rs:311-326`): the
`EventInitFn` built in `assemble()` needs the `StateManager` in order to call
`set_event_manager()` on it — but the closure is then *stored on that same manager*, via
`set_event_init()`, which parks it in a `OnceLock`. A strong `Arc<StateManager>` capture would
close a reference cycle:

```
StateManager --OnceLock<EventInitFn>--> closure --Arc--> StateManager
```

Neither end could reach zero, so dropping a `SonosSystem` would free nothing — not the manager,
not its store, not the event-worker thread, and once anything called `watch()`, not the
`SonosEventManager` with its tokio runtime or the callback server's socket. A long-running
process that rebuilt its system on reconnect or config reload would accumulate all of it.

`Arc::downgrade` breaks the cycle at the only edge that can be weak without changing behaviour.
The closure `upgrade()`s on entry; while the system is alive that always succeeds, and the sole
way it can fail is a `watch()` racing teardown, where declining to build a runtime and bind a
socket for a dying system is exactly right. That failure logs at `debug` and returns `Ok(())`,
because a torn-down system is not a caller error.

The one-shot guard inside the closure is a plain `Arc<Mutex<bool>>`. Its only job is to keep two
concurrent first-`watch()` calls from each constructing an event manager; it deliberately does
not store the manager, because a second owner is what would recreate the cycle.

### 3.2 Secondary Flow: Property Fetch

```
speaker.volume.fetch()                     src/property/handles.rs:556
        |
        +-- P::build_operation()           the Fetchable impl, e.g. :686
        |
        +-- resolve target                 src/property/handles.rs:560-573
        |     PerCoordinator -> state_manager.resolve_subscription_target()
        |     PerSpeaker     -> own id, current IP from the store
        |
        +-- observed_at = Instant::now()   :580   BEFORE the request
        |
        +-- api_client.execute_enhanced()  :585   one blocking SOAP round trip
        |
        +-- P::from_response(response)     :589
        |
        +-- state_manager.set_property_stamped(target_id, value,
        |       WriteStamp::observed_at(ChangeSource::Fetch, observed_at))   :594
        |
        +-- Ok(value)                      :601   returned regardless of the cache verdict
```

**Ordering, not just synchronisation.** A `fetch()` is a *read at request time* that lands at
*response time*. If a UPnP event arrives in that window carrying a newer value, the fetch
response must not overwrite it — otherwise a speaker visibly snaps back to its previous value a
moment after changing. Stamping before the request and letting
`sonos-state` compare observation instants is what prevents that; the store returns
`WriteOutcome::Stale` and declines the write. See
[sonos-state.md](sonos-state.md) §4.1a.

The caller still receives the value it fetched. It asked the device a question and got an
answer; only the shared cache declines to regress.

**Target resolution** is the same rule the read path uses. For a `PerCoordinator` service the
write is routed to the coordinator, so it lands in the bag `get()` reads from. For a
`PerSpeaker` service the current IP is re-read from the store first, so a fetch issued after a
DHCP move still reaches the device.

### 3.3 Secondary Flow: Property Watch

```
speaker.volume.watch()                     src/property/handles.rs:366
        |
        +-- lazy event init                :375-390
        |     no event manager yet? call the stored EventInitFn once
        |
        +-- resolve subscription target    :393
        |     PerCoordinator -> (coordinator_id, coordinator_ip)
        |
        +-- event manager present?
        |     yes -> SonosEventManager::acquire_watch(..) -> WatchGuard
        |            (increments the (ip, service) ref count; 0 -> 1 subscribes)
        |     no  -> StateManager::register_watch(..) -> CacheOnlyGuard
        |
        +-- build the read closure, capturing Arc<SpeakerContext>
        |     it calls the same get_property() that `get()` calls
        |
        +-- WatchHandle { read, mode, _cleanup }
        |
        +-- drop -> WatchGuard::Drop -> release_watch()
              at zero holds, a 50 ms grace period starts
```

**The handle is a live view, not a snapshot.** `value()` invokes the stored closure, which reads
the store through the same accessor `get()` uses — `get_property` for `PropertyHandle`,
`get_group_property` for `GroupPropertyHandle`. That is what keeps the read correct rather than
merely fresh:

- It inherits **coordinator resolution** ([sonos-state.md](sonos-state.md) §4.3), so a
  `PerCoordinator` property read through a member's handle still resolves to the coordinator,
  including after a regrouping the handle knew nothing about.
- It inherits the **write-ordering guard** ([sonos-state.md](sonos-state.md) §4.1a). The store
  only ever holds the newest-*observed* value, so re-reading cannot resurrect staleness. A read
  has no stamp of its own; it observes whatever survived the ordering.
- It can legitimately return `None` where a cached copy would have returned `Some`: if the
  resolved location no longer holds a value — a dissolved group, a speaker that left the
  topology — the honest answer is "unknown".

**Why a boxed closure rather than a stored context.** The two `watch()` sites read different
stores with different keys (`PropertyHandle` by speaker, `GroupPropertyHandle` by group). An
enum or a second type parameter would put that difference in the public type; the closure keeps
`WatchHandle<P>` single-shaped and makes it impossible for the two sites to drift into different
notions of "current".

**Why `Option<P>` by value.** The store sits behind a `parking_lot::RwLock` shared with the event
worker. Returning `Option<&P>` would either pin that lock for the handle's lifetime or alias a
value the worker is free to replace. Properties are small and `Clone` by trait bound. This also
rules out `Deref<Target = Option<P>>`, which must return a reference and therefore requires a
stored value — i.e. a snapshot.

`watch_or_fetch()` (`src/property/handles.rs:525`) is the composition: acquire the handle, and
if the store has no value yet, call `fetch()` and discard the result. Discarding is correct
because the fetch writes into the store the handle reads from, and a stale-rejected fetch is
handled for free — rejection means an event already delivered something newer, which is what
`value()` will then return.

### 3.4 Secondary Flow: Control Commands

Every command on `Speaker` and `Group` follows one shape:

```
speaker.set_volume(42)                          src/speaker.rs:605
        |
        +-- build the operation                 rendering_control::set_volume("Master", 42)
        |
        +-- Speaker::exec(operation)            src/speaker.rs:280
        |     operation.map_err(SdkError::ValidationFailed)?
        |     api_client.execute_enhanced(&self.context.speaker_ip.to_string(), op)
        |
        +-- on success, write the cache optimistically
              state_manager.set_property(&id, Volume(42))    src/speaker.rs:609
```

The optimistic write is what makes `speaker.set_volume(42)` immediately visible to
`speaker.volume.get()` without waiting for the device's own event. It is stamped
`ChangeSource::LocalAction` at write time, which is honest — the device just acknowledged the
action — and it resolves to the coordinator for `PerCoordinator` properties, so it lands where
the matching read looks.

The commands that write the cache are `play`/`pause`/`stop` (`src/speaker.rs:298`, `:309`,
`:320`), `set_volume` (`:605`), `set_relative_volume` (`:616`, which writes the device's
*returned* volume rather than the requested delta), `set_mute` (`:632`), `set_bass` (`:641`),
`set_treble` (`:650`), `set_loudness` (`:659`), and the group equivalents
`Group::set_volume` (`src/group.rs:337`), `set_relative_volume` (`:347`) and `set_mute` (`:361`).

Commands with no corresponding cached property — `next`, `previous`, `seek`, the queue
operations, the alarm operations — write nothing and rely on the resulting device event.

`Group::exec` (`src/group.rs:253`) is the group counterpart and always targets `coordinator_ip`.

### 3.5 Error Flow

```
sonos_state::StateError            --> SdkError::StateError        (#[from])
sonos_api::ApiError                --> SdkError::ApiError          (#[from])
sonos_api::operation::ValidationError --> SdkError::ValidationFailed (#[from])
event-manager init failure (boxed) --> SdkError::EventManager
name lookup miss after rediscovery --> None from speaker(); SpeakerNotFound where a name is required
unparseable device IP             --> SdkError::InvalidIpAddress
coordinator self-add / self-remove --> SdkError::InvalidOperation
SSDP failure with no cache        --> SdkError::DiscoveryFailed
poisoned RwLock on `speakers`     --> SdkError::LockPoisoned
```

**Error handling philosophy**: upstream errors are wrapped with `#[from]` so `?` works and the
source chain is preserved. Errors the SDK originates carry a `String` because the interesting
detail is the message, not a further match. Lookups that can legitimately find nothing —
`speaker()`, `group()`, `coordinator()` — return `Option` rather than an error.

`Group::dissolve()` (`src/group.rs:314`) is the exception to "return `Result`": it returns a
`GroupChangeResult` (`src/group.rs:34`) carrying `succeeded` and `failed` vectors, because a
partial dissolve is a real and useful outcome that a single `Result` cannot express.

---

## 4. Features

### 4.1 Feature: DOM-like property access

#### What

Properties are fields on `Speaker` and `Group`, each carrying `get()`, `fetch()` and `watch()`.

#### Why

Grouping the three reads on one handle means they cannot drift apart, and makes the API
navigable by autocomplete rather than by documentation. It also gives each property one place to
express its own constraints: `GroupVolumeChangeable` simply has no `fetch()`, because no UPnP
operation returns it.

#### How

```rust
use sonos_sdk::prelude::*;

let system = SonosSystem::new()?;
let speaker = system.speaker("Living Room").unwrap();

// Cached read — no network
let volume = speaker.volume.get();

// Fresh read — one SOAP round trip, updates the shared cache
let fresh = speaker.volume.fetch()?;

// Reactive — hold the handle for as long as you want updates
let watch = speaker.volume.watch()?;
for _event in system.iter() {
    println!("volume now {:?}", watch.value());
}

// Commands live on the speaker itself
speaker.play()?;
speaker.set_volume(35)?;

// Fluent navigation
let group = speaker.group().unwrap();
let kitchen = group.speaker("Kitchen");
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Property as a struct field | `speaker.get_property::<Volume>()` | Discoverability, and a natural home for per-property differences in capability |
| Separate `get`/`fetch`/`watch` | One method with a freshness enum | Three different return types (`Option<P>`, `Result<P>`, `Result<WatchHandle<P>>`) and three different cost profiles; an enum would have to erase all of that |
| Hand-written type aliases over `PropertyHandle<P>` | A declarative macro generating a struct per property | The generic handle already carries every behaviour; a per-property struct would only restate it. Adding a property is one `Fetchable` impl plus one alias |
| Commands as methods, reads as handles | Commands on the handle too (`volume.set(35)`) | A command is an operation on the *speaker*, and several commands touch no property at all. Keeping the handle read-shaped keeps it honest |

### 4.2 Feature: Automatic, ordered state synchronisation

#### What

A successful `fetch()` or command writes its value into the shared `StateManager`, stamped with
when the value was *observed*, so the cache reflects it immediately without waiting for a device
event — and without ever overwriting something newer.

#### Why

Without the write, `get()` would disagree with the value `fetch()` just returned, and
`speaker.play()` would leave `playback_state.get()` reporting the old state until an event
arrived. Without the *ordering*, a slow fetch would clobber an event that overtook it, which is
visible as a value snapping back moments after changing.

#### How

```rust
// src/property/handles.rs:556 (abridged)
pub fn fetch(&self) -> Result<P, SdkError> {
    let observed_at = Instant::now();   // BEFORE the request

    let response = self.context.api_client
        .execute_enhanced(&target_ip.to_string(), P::build_operation()?)?;
    let value = P::from_response(response);

    self.context.state_manager.set_property_stamped(
        &target_id,
        value.clone(),
        WriteStamp::observed_at(ChangeSource::Fetch, observed_at),
    );

    Ok(value)
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Automatic cache update | Leave it to the caller | Every caller wants it, and the one that forgets produces a cache that silently disagrees with itself |
| Stamp before the request | Stamp when the response lands | Stamping on arrival makes every slow read look freshest, which is precisely the clobbering failure |
| Return the fetched value even when the cache rejects it | Return an error, or return the cached value | The caller asked the device a question and got an answer; substituting a different value would be worse. The cache-level decision is a separate concern |
| Optimistic write after a command | Wait for the device event | The device has already acknowledged the action; waiting makes every UI feel laggy for no added correctness |

### 4.3 Feature: `WatchHandle` is a live view

#### What

`WatchHandle::value()` reads the state store on every call and returns `Option<P>` by value. A
handle acquired once and held reports the property's *current* value for as long as it lives.
`has_value()` is the same read.

#### Why

A `WatchHandle` is an RAII lease on a subscription, and reading through a live lease should give
a live answer. A handle that cached its value at construction would report the store's contents
at the instant `watch()` returned, forever — so a handle held across a render loop would show one
value permanently, and a handle acquired before the first event would stay permanently empty.
The only workaround would be to re-`watch()` every frame, which is exactly the overlapping-hold
churn the grace period and the reference-counted watched set exist to absorb. Making the read
live removes the need for the pattern rather than merely making it survivable.

#### How

```rust
// src/property/handles.rs:129
pub struct WatchHandle<P> {
    read: Box<dyn Fn() -> Option<P> + Send + Sync>,
    mode: WatchMode,
    _cleanup: WatchCleanup,
}

// src/property/handles.rs:158
pub fn value(&self) -> Option<P> { (self.read)() }
```

`WatchCleanup` (`src/property/handles.rs:196`) is the RAII half, with three shapes:

| Variant | When | What drop does |
|---------|------|----------------|
| `Guard(WatchGuard)` | Event manager present, property owned by this speaker | Releases one `(ip, service)` ref count; a 50 ms grace period starts at zero |
| `CacheOnly(CacheOnlyGuard)` | No event manager | Releases one hold on `(speaker_id, property_key)` in the watched set |
| `CoordinatorGuard { .. }` | `PerCoordinator` property routed to a coordinator | Both: the guard releases the coordinator's subscription, the cache-only guard releases the *member's* watched-set entry |

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| `value()` re-reads the store | Make the handle a pure lease and read through `speaker.volume.get()` | Splitting them makes every call site carry two things to keep in sync, and the handle's `#[must_use]` no longer guards the thing you actually read |
| Boxed closure | Store `Arc<SpeakerContext>` / `Arc<GroupContext>` in the handle | The two `watch()` sites read different stores with different keys; an enum or second type parameter would put that in the public type |
| Return `Option<P>` by value | Return `Option<&P>` | A borrow would pin the store's `RwLock` for the handle's lifetime, or alias a value the event worker may replace |
| No `Deref<Target = Option<P>>` | Keep `Deref` over a cached copy | `Deref` must return a reference, which requires a stored value — i.e. a snapshot |
| Accept one lock per `value()` | Cache with an invalidation flag bumped by the event worker | A generation counter is a second source of truth about freshness whose failure mode is silent staleness. An uncontended `parking_lot` read lock is tens of nanoseconds against a 16 ms frame budget |

### 4.4 Feature: Lazy event infrastructure

#### What

No subscription, no callback server, no tokio runtime and no worker thread exist until the first
`watch()` call anywhere in the process.

#### Why

A CLI that fetches a volume and exits should not bind a socket, spawn a runtime and hold a UPnP
subscription open. Making the cost opt-in on first use keeps the fetch-only path free while
`watch()` stays a single call for the user.

#### How

`assemble()` (`src/system.rs:327`) builds an `EventInitFn` and installs it on the `StateManager`
with `set_event_init()`. `PropertyHandle::watch()` (`src/property/handles.rs:375`) checks
`state_manager.event_manager()` and, on a miss, invokes the stored closure exactly once. The
closure constructs a `SonosEventManager`, hands it to `StateManager::set_event_manager()` — which
wires the `WatchRegistry`, re-registers known devices, and spawns the state event worker — and is
a no-op thereafter because the manager lives in a `OnceLock`.

A `watch()` on a system with no `EventInitFn` — an offline test system — succeeds in
`WatchMode::CacheOnly` rather than failing. The property is still registered in the watched set,
so a `set_property` write still produces a `ChangeEvent`; only the UPnP subscription is absent.

### 4.5 Feature: Group-aware reads, writes and subscriptions

#### What

For `PerCoordinator` services — AVTransport, GroupRenderingControl, GroupManagement — a property
accessed through any member resolves to the group's coordinator, for reads, for writes, and for
the subscription target.

#### Why

Sonos gives the coordinator authority over playback for the whole group. Members emit AVTransport
events carrying empty defaults. Subscribing a member, storing its events, or reading its own bag
would all produce wrong answers.

#### How

The SDK does not implement the rule; it routes through the one place that does.
`PropertyHandle::watch()` and `fetch()` both call
`StateManager::resolve_subscription_target()`, and `set_property()` resolves internally. See
[sonos-state.md](sonos-state.md) §4.3 for the predicate and the store-side machinery.

Where the SDK does add something is the *member's* watched-set entry. A member watching a
coordinator-owned property gets a `WatchCleanup::CoordinatorGuard`: the subscription is held
against the coordinator, and a separate `CacheOnlyGuard` registers the member's own
`(speaker_id, property_key)` pair, so the member is notified through `system.iter()` when the
coordinator's value changes. Dropping the handle releases both.

### 4.6 Feature: Disk-cached discovery

#### What

`SonosSystem::new()` reads a cached device list from disk before falling back to a 3-second SSDP
sweep, and writes the sweep's result back.

#### Why

SSDP costs a fixed 3 seconds every time, which dominates startup for a CLI that runs often.
Household topology changes rarely.

#### How

`src/cache.rs`, none of which is public:

| Item | Line | Behaviour |
|------|------|-----------|
| `CACHE_TTL_SECS` | :12 | 24 hours |
| `cache_dir()` | :27 | `$SONOS_CACHE_DIR` if set, non-empty **and absolute**; otherwise `dirs::cache_dir().join("sonos")` |
| `load()` | :36 | Reads `cache.json`; rejects a file declaring more than 256 devices |
| `save()` | :46 | Writes `cache.json.tmp` then renames, removing the temp file if the rename fails |
| `is_stale()` | :66 | Older than the TTL — **or** stamped in the future, which means a clock change rather than a fresh cache |

A `speaker()` lookup that misses triggers `try_rediscover()` (`src/system.rs:637`), rate-limited
by `REDISCOVERY_COOLDOWN_SECS = 30` (`src/system.rs:105`), so a stale cache self-heals on the
first name that is not in it.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Atomic write via temp + rename | Write in place | A crash mid-write would otherwise leave a truncated JSON file that every subsequent start must fail to parse |
| Reject a cache declaring >256 devices | Trust the file | The file is user-writable; a bogus length is the cheapest way to make startup allocate wildly |
| Future timestamps are stale | Treat them as fresh | A clock that moved backwards would otherwise pin a stale cache until the TTL elapsed in the new frame |
| Absolute `SONOS_CACHE_DIR` only | Accept relative paths | A relative cache path resolves against the process CWD, so the same program caches to different places depending on where it was launched |

---

## 5. Data Model

### 5.1 Core Data Structures

The public data model is mostly re-exported. `sonos-sdk` defines `SonosSystem`, `Speaker`,
`Group`, `GroupChangeResult`, `SeekTarget`, `PlayMode`, `SdkError` and the handle types; the
property *values* come from `sonos-state` and the identity types from `sonos-api` through it.

#### `SeekTarget` (`src/speaker.rs:40`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeekTarget {
    Track(u32),     // :43  absolute queue position
    Time(String),   // :45  absolute "HH:MM:SS"
    Delta(String),  // :47  relative "+00:00:30" / "-00:00:30"
}
```

The private `unit()` (`:52`) and `target()` (`:61`) map each variant onto the UPnP `Unit` and
`Target` arguments, which is why the variants are not simply a string.

#### `PlayMode` (`src/speaker.rs:71`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Normal,             // :74  "NORMAL"
    RepeatAll,          // :76  "REPEAT_ALL"
    RepeatOne,          // :78  "REPEAT_ONE"
    ShuffleNoRepeat,    // :80  "SHUFFLE_NOREPEAT"
    Shuffle,            // :82  "SHUFFLE"
    ShuffleRepeatOne,   // :84  "SHUFFLE_REPEAT_ONE"
}
```

`Display` (`src/speaker.rs:87`) produces the UPnP strings. Neither enum is `#[non_exhaustive]`.

#### `GroupChangeResult` (`src/group.rs:34`)

```rust
#[derive(Debug)]
pub struct GroupChangeResult {
    pub succeeded: Vec<SpeakerId>,          // :37
    pub failed: Vec<(SpeakerId, SdkError)>, // :39
}
```

With `is_success()` (`:44`) and `is_partial()` (`:49`). Returned by `Group::dissolve()` and
`SonosSystem::create_group()`, both of which act on several speakers and can partially succeed.

### 5.2 Re-exported Types

`src/lib.rs` re-exports three groups of names.

**Property values and change events**, from `sonos-state` (`src/lib.rs:103-107`):

| Type | Purpose |
|------|---------|
| `Volume`, `GroupVolume`, `GroupMute`, `GroupVolumeChangeable` | Volume and mute values |
| `PlaybackState`, `CurrentTrack` | Transport state and track metadata |
| `SpeakerId`, `GroupId` | Identity newtypes (defined in `sonos-api`, re-exported through `sonos-state`) |
| `ChangeEvent`, `ChangeIterator`, `ChangeSource`, `PropertyChange` | The `system.iter()` stream and its payload |
| `WriteOutcome`, `WriteStamp` | The write-ordering vocabulary |

`Property` and `SonosProperty` are re-exported separately (`src/lib.rs:117`) so a downstream
crate can write code generic over a property without depending on `sonos-state` directly.

Not re-exported here but reachable through `sonos_state`: `Mute`, `Bass`, `Treble`, `Loudness`,
`Position`, `GroupMembership`, `Topology`.

**Operation response types**, from `sonos-api` (`src/lib.rs:87-95`) — the return types of the
`Speaker` and `Group` methods that report something back, such as `GetMediaInfoResponse` and
`SetRelativeVolumeResponse`. They are re-exported so a caller never has to add a `sonos-api`
dependency to name a return type.

**`sonos_discovery`** itself (`src/lib.rs:99`), gated on `feature = "test-support"`, so a test
can build the `Device` list that `from_devices_offline` takes.

#### `prelude` (`src/prelude.rs`)

`SdkError`, `Group`, `Speaker`, `PlayMode`, `SeekTarget`, `SonosSystem`, plus `GroupId`,
`GroupMute`, `GroupVolume`, `PlaybackState`, `SpeakerId`, `Volume`, `ChangeSource` and
`PropertyChange`.

Deliberately absent: `WatchHandle`, `WatchMode`, `PropertyHandle`, `GroupChangeResult`,
`CurrentTrack`, `ChangeEvent`, `ChangeIterator`. A program that needs those is doing something
specific enough to import them by name.

### 5.3 State Transitions

```
                SonosSystem::new()              first watch()
                       |                              |
 +-----------+         v          +--------------+    v     +------------------+
 | no system |------------------->| fetch-only   |--------->| live             |
 +-----------+                    | get/fetch OK |          | events flowing   |
                                  | no threads   |<---------| worker + runtime |
                                  +--------------+  drop()  +------------------+
```

**Invariants per state**:
- **fetch-only**: the `StateManager` exists and holds the device list and topology; no event
  manager, no worker thread, no runtime, no callback socket
- **live**: the event manager is set (`OnceLock`, one-way for a given system), the state event
  worker is running, and watched properties emit through `system.iter()`

The transition to *live* is one-way for a given `SonosSystem`. What does come back is
subscription state, managed by `sonos-event-manager` after its 50 ms grace period.

### 5.4 Serialization

| Format | Use Case | Library | Notes |
|--------|----------|---------|-------|
| JSON | The discovery cache | `serde_json` | `CachedDevices { devices, cached_at }`, `src/cache.rs:21` |
| Serde derives | Property values, `Device` | `serde` | Inherited from `sonos-state` and `sonos-discovery`; this crate adds none |

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `sonos-state` (pkg `sonos-sdk-state`) | `StateManager`, property types, `ChangeIterator` | The cache, the change stream, and the coordinator-resolution and write-ordering rules the SDK routes through |
| `sonos-api` | `SonosClient`, `Service`, operations, `SpeakerId`/`GroupId` | Typed UPnP operations and the SOAP execution path |
| `sonos-discovery` (pkg `sonos-sdk-discovery`) | `Device`, `get_with_timeout()` | SSDP discovery |
| `sonos-event-manager` (pkg `sonos-sdk-event-manager`) | `SonosEventManager`, `WatchGuard` | Reference-counted subscription lifetime behind `watch()` |
| `thiserror` | `SdkError` derive | Workspace convention |
| `tracing` | Diagnostics | `trace`/`debug` on the watch and discovery paths |
| `serde`, `serde_json` | The discovery cache | The only thing this crate serializes |
| `dirs` | Platform cache directory | Resolving `~/.cache/sonos` and its platform equivalents |

**Not a dependency: `tokio`.** The workspace defines it, but `sonos-sdk` never opts in. It
enters the graph only transitively, inside `sonos-event-manager`, `sonos-stream` and
`callback-server`.

Dev-dependencies: `proptest`, `chrono`, `ctrlc`, `tracing-subscriber` — the last three for the
examples, which CI compiles.

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| End-user applications | The primary entry point | Pre-1.0; breaking changes may land in minor versions |
| Examples (`sonos-sdk/examples/`) | Six programs exercising discovery, properties, groups and the watch lifecycle | Compiled by CI via `--all-targets` |

### 6.3 External Systems

```
+-----------------+     UPnP/SOAP over HTTP :1400     +-----------------+
|   sonos-sdk     |<--------------------------------->|  Sonos speakers |
| (via sonos-api) |                                   |                 |
+-----------------+                                   +-----------------+
        |  SSDP multicast 239.255.255.250:1900 (discovery)      ^
        |  HTTP NOTIFY inbound on 3400-3500 (events)            |
        +-------------------------------------------------------+
```

**Protocol**: UPnP/SOAP over HTTP on port 1400 for control; SSDP multicast for discovery;
inbound HTTP NOTIFY on a port in 3400-3500 for events.

**Authentication**: none. UPnP on the local network is an unauthenticated trust model.

**Error handling**: network errors surface as `SdkError::ApiError`; a device refusal surfaces as
the same variant carrying a `SoapFault` with the UPnP error code.

**Retry strategy**: none at this layer. Applications that need retries should implement them
around the call, distinguishing `SoapFault` codes in the 400s (retrying is futile) from the 500s
and 700s (may be transient).

---

## 7. Error Handling

### 7.1 Error Types

See §2.3 for the full `SdkError` definition. Three `#[from]` conversions cover the upstream
crates: `sonos_state::StateError`, `sonos_api::ApiError` and
`sonos_api::operation::ValidationError`.

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| One error type at the SDK boundary | `SdkError` with `#[from]` on upstream errors | A caller handles one type; the source chain still reaches the original |
| `#[non_exhaustive]` | `src/error.rs:4` | Adding a variant as the surface grows must not be a breaking change |
| Absence is not an error | `speaker()`, `group()`, `coordinator()` return `Option` | "No speaker by that name" is a normal answer, not a failure |
| Partial success is expressible | `GroupChangeResult` | A dissolve that reaches three of four members is neither a success nor a failure |
| Validation before the network | `ValidationFailed` from `.build()` | A bad argument should not cost a round trip |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `StateError` | Sometimes | Depends on the underlying variant; check the source |
| `ApiError::NetworkError` | Yes | Retry with backoff; the speaker may be briefly unreachable |
| `ApiError::SoapFault(4xx)` | No | The request itself is wrong; retrying cannot help |
| `ApiError::SoapFault(5xx / 7xx)` | Sometimes | Device-side or state-dependent; may succeed later |
| `ValidationFailed` | No | A caller bug — the argument is out of range or malformed |
| `InvalidOperation` | No | A caller bug — e.g. adding a coordinator to its own group |
| `EventManager` | Sometimes | The event infrastructure failed to start; `get()`/`fetch()` still work |
| `DiscoveryFailed` | Yes | Re-run discovery; check that SSDP multicast reaches the network |
| `SpeakerNotFound` | Yes | The name is wrong, or discovery is stale — a lookup miss already retries once |
| `InvalidIpAddress` | No | Malformed data from discovery or from a caller-supplied device list |
| `LockPoisoned` | No | A panic unwound while holding the speaker-map lock |
| `WatcherClosed` | — | Declared but never constructed; see §14.2 |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

111 `#[test]` functions — 60 unit, 51 integration — plus 32 doc-tests. None is a
`#[tokio::test]`, because nothing in the crate is async. 20 of the integration tests are
`#[ignore]`d because they need real hardware, leaving 91 that run by default.

```
              +-------------------------------+
              | Hardware integration (ignored)|  20 tests, run with --ignored
              +---------------+---------------+
      +-----------------------+-----------------------+
      |   Offline property tests (proptest)           |  31 tests in 14 proptest! blocks
      +-----------------------+-----------------------+
 +--------------------------------------------------------+
 |                     Unit tests                          |  handles 24, system 21,
 +--------------------------------------------------------+  group 12, speaker 3
```

**Running them**: the offline constructors are gated `#[cfg(any(feature = "test-support", test))]`,
and that `test` cfg covers only this crate's own unit tests. `tests/*.rs` are separate crates, so
they need the feature:

```bash
cargo test -p sonos-sdk --features test-support
```

A bare `cargo test -p sonos-sdk` fails to compile `tests/property_tests.rs`.

### 8.2 Unit Tests

**Location**: inline `#[cfg(test)] mod tests` — `src/property/handles.rs:1153` (24),
`src/system.rs:1026` (21), `src/group.rs:375` (12), `src/speaker.rs:668` (3).

**What is covered**:
- [x] Name-keyed registry behaviour: satellite exclusion, duplicate room-name disambiguation,
      case-insensitive lookup
- [x] Group navigation: `coordinator()`, `members()`, `speaker()`, `is_coordinator()`,
      `member_count()`, `is_standalone()`
- [x] Validation and guard clauses that return before any network call —
      `test_group_set_volume_rejects_over_100`, `test_add_speaker_rejects_coordinator_self_add`,
      `test_remove_speaker_rejects_coordinator_removal`,
      `test_dissolve_standalone_returns_empty_result`, and the `set_*_rejects_invalid` family
- [x] `WatchHandle` semantics: `value()` re-reads, `mode()` reports `CacheOnly` with no event
      manager, dropping one handle does not silence a sibling
- [x] Leak assertions (§8.7)

### 8.3 Component Tests

**Location**: `tests/property_tests.rs` — 31 tests inside 14 `proptest!` blocks, all offline,
none `#[ignore]`d. They build systems with `SonosSystem::from_devices_offline` and
`with_speakers`, then assert round-trip invariants over proptest-generated IDs, names and IPs:
a speaker registered under a generated name is retrievable by it, `speakers()` counts match the
device list, `speaker_by_id()` agrees with `speaker()`, and group membership is consistent in
both directions.

These tests deliberately build explicit `Device` lists rather than routing through
`with_speakers()`, which hardcodes `RINCON_{i:03}` and `192.168.1.{100+i}` and would discard the
generated values — leaving assertions that still pass while testing nothing.

### 8.4 Integration Tests

**Location**: `tests/integration_real_speakers.rs` (6), `tests/property_validation.rs` (6),
`tests/data_freshness.rs` (8). All 20 are `#[ignore]`d.

**Prerequisites**:
- [ ] At least one Sonos speaker on the LAN
- [ ] Inbound HTTP on a port in 3400-3500, or acceptance of the polling fallback

```bash
cargo test -p sonos-sdk --features test-support -- --ignored
```

`tests/data_freshness.rs` is structured so that adding a case is one `#[test] #[ignore]`
function using the shared helpers; its subject is whether `get()` after `fetch()` and after a
live event agrees with the device.

### 8.5 Offline Construction (`test-support`)

#### What

`SonosSystem::from_devices_offline(devices)` (`src/system.rs:192`) builds a fully-formed
`SonosSystem` from a caller-supplied device list while guaranteeing zero network I/O. It is
gated behind the `test-support` feature (or this crate's own `test` cfg). `with_speakers()`
(`:424`) and `with_groups()` (`:471`) share the guarantee through the `offline` field.

Two variants inject the topology a poll would have returned, so the topology-dependent
construction steps still run in their real order:

| Constructor | Line | Seeds | Usable from |
|---|---|---|---|
| `from_devices_offline_with_topology(devices, seed)` | :210 | Arbitrary state, via a `&Self` closure | **this crate only** — `state_manager` is private, so the closure can seed nothing downstream |
| `from_devices_offline_with_groups(devices, groups)` | :249 | `(GroupId, coordinator, members)` tuples | Any crate with `test-support` |

`from_devices_offline_with_groups` exists because multi-member topology is otherwise unreachable
from outside this crate: `with_groups()` only ever builds single-member groups, and the closure
form is unusable downstream. A consumer could not otherwise test anything depending on group size
or coordinator identity.

#### Why

`SonosSystem` has exactly **two** paths that reach the network without the caller asking, and
both are catastrophic in a test process where the device IPs are synthetic:

1. **Construction** — `ensure_topology()` (`src/system.rs:796`) SOAP-polls
   `zone_group_topology::state::poll` against every known speaker IP whenever
   `group_count() == 0`. Each unreachable IP costs `soap-client`'s 5s connect plus 10s read
   timeout, serially.
2. **Lookup miss** — `speaker(name)` that misses calls `try_rediscover()` (`src/system.rs:637`),
   a 3s SSDP sweep rate-limited by `REDISCOVERY_COOLDOWN_SECS = 30` (`:105`). Property tests
   generating random names pay the full cooldown per test binary.

Neither timeout represents a real failure — the IPs simply do not exist — so the suite spends
minutes proving nothing. Property tests that exercise only in-memory name/ID bookkeeping have no
reason to pay for either.

#### How

`offline: bool` is consulted at the top of both network entry points, so it closes both paths
with one flag rather than requiring each test to remember which methods are safe:

```rust
// src/system.rs:796
fn ensure_topology(&self) {
    if self.offline || self.state_manager.group_count() > 0 { return; }
    // ... SOAP poll every speaker IP ...
}

// src/system.rs:637
fn try_rediscover(&self, name: &str) {
    if self.offline { return; }
    // ... 3s SSDP sweep ...
}
```

Production paths (`new()` → `from_devices_inner()`) leave `offline = false`.

`from_devices_offline` runs the same `construct()` sequence as production; only the topology
*poll* is skipped. The post-topology steps still execute and, with no topology, are no-ops:
`get_satellite_ids()` is empty so the satellite re-key returns early, and no IPs have changed.

`from_devices_offline_with_topology(devices, seed)` exists for tests that must exercise those
steps. The `seed` closure runs after `assemble()` and before the satellite re-key — exactly the
window `ensure_topology()` occupies in production — so a test can inject the satellite IDs a real
poll would have returned. Such tests must **not** reproduce the sequence themselves: the *order*
of these steps is the substance of the behaviour (§3.1), so a test that re-implements the order
cannot detect production's order regressing.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| `offline: bool` field | A separate `OfflineSonosSystem`, or a trait-abstracted transport | One flag, two call sites, no API duplication. A mock transport is the right long-term fix and a much larger one |
| The flag closes *both* paths | Skip only `ensure_topology()` | Skipping construction alone leaves the 30s rediscovery cooldown on every lookup miss — the larger of the two costs |
| Reuse `assemble()` for both paths | Copy the constructor body | The Arc wiring (§3.1) is subtle enough that two copies would drift |
| A wall-clock bound to prove no I/O | Mock transport, or a network namespace | Without a mock transport a time bound is the only pure-unit assertion available; the 500 ms threshold is ~10x headroom over the real cost and ~10x below the cheapest timeout it guards |

### 8.6 Signature Assertions

#### What

Action methods that require a live speaker are type-checked by never-called functions rather
than executed by `#[test]` fns:

```rust
#[allow(dead_code)]
fn _assert_action_signatures(speaker: &Speaker) {
    fn void<T>(_: Result<T, SdkError>) {}
    void(speaker.play());
    void(speaker.pause());
    // ...
}
```

Four exist: `_assert_action_signatures` (`src/speaker.rs`), `_assert_group_action_signatures`
and `_assert_group_lifecycle_signatures` (`src/group.rs`), and `_assert_create_group_signature`
(`src/system.rs`).

#### Why

These assertions are about **types, not behaviour**. Executing them is pure cost: each call
opens a real TCP connection to a synthetic IP and waits out `soap-client`'s 5s connect timeout,
and the returned `Result` is discarded unexamined.

Rust type-checks the body of a function even when nothing calls it, so a never-called `fn` fails
the build the instant a signature drifts — the exact guarantee a `#[test]` version provides —
while contributing no runtime. The `_` prefix and `#[allow(dead_code)]` document the intent and
silence the unused warning under `-D warnings`.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Never-called fn | Delete the assertions | Signature drift in a public API should break the build; the assertion has real value, only its execution does not |
| Never-called fn | `let _ = ...` inside a `#[test]` | Still executes the call and still pays the timeout — the cost is the invocation |
| Take params by reference | Construct fixtures inside the fn | Nothing constructs them, so params keep the fn free of fixture setup |

**Boundary**: this pattern applies *only* where the assertion is purely about types. Tests that
assert real behaviour on paths returning before any network call remain executing `#[test]` fns
(§8.2). Validation and guard-clause logic must keep running.

### 8.7 Leak Assertions (`state_manager_weak`)

#### What

`SonosSystem::state_manager_weak()` (`src/system.rs:743`) returns a
`std::sync::Weak<StateManager>`. It is gated behind `test-support` (or the crate's own `test`
cfg), alongside `from_devices_offline`. Two tests use it:
`test_dropping_system_releases_state_manager` and
`test_dropping_system_after_watch_releases_state_manager` (`src/system.rs`).

#### Why

The Arc cycle described in §3.1 is invisible to every other kind of test. Construction succeeds,
teardown "succeeds", and every functional assertion passes — the only symptom is that memory, a
thread, a runtime and a socket are never reclaimed. Detecting that requires a handle that
**outlives the system** and can then be asked whether the target is gone.

Nothing already public can answer the question. `state_manager()` (`src/system.rs:727`) returns
`&Arc<StateManager>` borrowed from `&self`, so it cannot survive the drop, and cloning the `Arc`
first would itself keep the manager alive and mask the condition under test. A `Weak` is the only
observer that does not perturb what it measures.

The second test exists because the first does not exercise the closure. The cycle is created at
`assemble()` time, but the closure only *runs* on the first `watch()`; testing both means the
assertion holds whether or not lazy init ever fired.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| `Weak` accessor behind `test-support` | Assert `Arc::strong_count` via `state_manager()` while alive | A live count is not the invariant: `Speaker` handles legitimately hold their own `Arc`s, so the number tracks the device count. The invariant is "zero *after* drop" |
| Add a test-only accessor | Leave the cycle untested | The bug is silent by construction; an untested fix is indistinguishable from no fix |
| `Weak` | A `Drop` impl setting a flag | A flag proves `SonosSystem` dropped, which was never in doubt — the question is whether the *manager* was released |

### 8.8 Test Fixtures & Mocks

| Dependency | Strategy | Location |
|------------|----------|----------|
| `sonos-discovery` | Caller-supplied `Vec<Device>` | `from_devices_offline` and friends; `sonos_discovery` is re-exported under `test-support` so tests can name `Device` |
| Topology | Injected via `from_devices_offline_with_groups` / `_with_topology` | §8.5 |
| `sonos-api` | Not mocked | Anything needing a real SOAP response is an `#[ignore]`d hardware test |
| `StateManager` | Real instance | Cheap to construct; no I/O until a subscription exists |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| `get()` latency | < 1 us | One uncontended read lock plus a clone of a small value; it is called per property per frame |
| `WatchHandle::value()` latency | < 1 us | Same cost as `get()` — it is the same read |
| `fetch()` latency | < 100 ms | One SOAP round trip on a LAN |
| `SonosSystem::new()` with a warm cache | < 50 ms | No SSDP; the cost is the topology poll |
| `SonosSystem::new()` with a cold cache | ~3 s | The SSDP sweep timeout, which is fixed |
| Memory per speaker | < 10 KB | Handles are `Arc` clones plus a `PhantomData` |

### 9.2 Critical Paths

1. **`PropertyHandle::get()`** (`src/property/handles.rs:334`)
   - **Complexity**: O(1) — one read lock, one or two hash lookups (coordinator resolution adds
     one), one clone
   - **Bottleneck**: the property's own clone cost, which is why property types are small
   - **Note**: `WatchHandle::value()` (`:158`) is this same read behind a boxed closure

2. **`PropertyHandle::fetch()`** (`src/property/handles.rs:556`)
   - **Complexity**: O(1) network calls
   - **Bottleneck**: network latency; blocking, and it blocks the calling thread
   - **Optimization**: the shared `SoapClient` singleton pools connections

3. **`SonosSystem::speaker()`** (`src/system.rs:614`)
   - **Complexity**: O(1) on a hit; a 3s SSDP sweep on a miss
   - **Bottleneck**: the miss path, rate-limited to once per 30 s per system
   - **Note**: `offline` systems skip it entirely (§8.5)

4. **`SonosSystem::groups()`** (`src/system.rs:866`)
   - **Complexity**: O(groups x members), plus `ensure_topology()` if no topology is known
   - **Bottleneck**: the first call on a system with no topology, which SOAP-polls

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| `StateManager` | `SonosSystem::new()` | When the last `Arc` drops — including every `Speaker` and `Group` handle | Yes; one per system |
| `SonosClient` | `SonosSystem::new()` | Cheap clone of the process-wide SOAP singleton | Yes; the `ureq::Agent` is shared |
| Event manager, runtime, callback socket | First `watch()` anywhere | With the `StateManager` | One per system |
| UPnP subscription | `WatchHandle` acquisition at zero holds | Handle drop, after a 50 ms grace period | Reference-counted per `(ip, service)` |
| Discovery cache file | `SonosSystem::new()` on a cold start | Never; overwritten on the next cold start | N/A |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Hostile device answering SSDP | Low | Low | Device descriptions are validated (manufacturer and device type) before a speaker is created |
| Forged UPnP event on the LAN | Low | Medium | Events from IPs absent from the state manager's map are dropped; see [sonos-state.md](sonos-state.md) §10.1 |
| Tampered discovery cache | Low | Medium | The cache is user-writable by design. A declared device count above 256 is rejected; every cached IP is still contacted over plain UPnP, so a poisoned entry can misdirect commands |
| Denial of service via discovery | Low | Low | Discovery is bounded by a fixed 3s timeout |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Speaker IPs and UUIDs | Low | LAN-local; written to the discovery cache in plaintext |
| Speaker names | Low | User-configured |
| Track metadata | Low | Passed through, not persisted |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| Discovery responses | Device description must identify as Sonos; IP must parse | `sonos-discovery`; `Speaker::from_device` (`src/speaker.rs:184`) |
| Command arguments | `Validate` impls run at `.build()`, before any network call | `sonos-api`; surfaced as `SdkError::ValidationFailed` |
| Group membership operations | A coordinator may not be added to or removed from its own group | `src/group.rs:272`, `:293` |
| `SONOS_CACHE_DIR` | Must be non-empty **and** absolute | `src/cache.rs:27` |
| Cache file contents | Rejected above 256 devices; a future timestamp counts as stale | `src/cache.rs:40`, `:66` |
| Speaker names | None — trusted from the device | N/A |

---

## 11. Observability

### 11.1 Logging

`tracing` is a direct dependency and the crate emits records on the watch, discovery and
construction paths.

| Level | What's Logged | Example |
|-------|--------------|---------|
| `debug` | Lazy event-manager init, satellite exclusion, rediscovery, topology fetch failures | "Event manager not initialized, triggering lazy init"; "Topology fetch failed for {ip}" |
| `trace` | Every `watch()` call, with property service and speaker | `src/property/handles.rs:367` |
| `warn` | Duplicate room names, cache write failures | — |

Deeper diagnostics live downstream: event decoding in `sonos-state`, subscription lifetime in
`sonos-event-manager`, delivery and polling in `sonos-stream`.

### 11.2 Metrics

None exposed. `WatchHandle::mode()` is the closest thing — it reports whether a given watch is
receiving real-time events, polling, or nothing.

### 11.3 Tracing

No `#[instrument]` spans. Observability is event-based logging on the flows above.

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `test-support` | cargo feature | off | Exposes `from_devices_offline*`, `with_speakers`, `with_groups`, `state_manager_weak`, `from_discovered_devices` and the `sonos_discovery` re-export |

There are no runtime configuration options. Discovery timeout (3 s), rediscovery cooldown (30 s),
cache TTL (24 h) and the watch grace period (50 ms) are all constants.

### 12.2 Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `SONOS_CACHE_DIR` | No | `dirs::cache_dir().join("sonos")` | Directory for `cache.json`. Ignored unless non-empty and absolute (`src/cache.rs:27`) |

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `SonosSystem::new()` | Unstable | May gain a configuration argument |
| `speaker.<property>.get() / .fetch() / .watch()` | Stable | The core pattern |
| `WatchHandle` | Stable | `value()` is a live read; the handle is the subscription lease |
| `Speaker` command methods | Evolving | The set grows as operations are surfaced; existing signatures are stable |
| `Group` lifecycle methods | Evolving | `add_speaker`, `remove_speaker`, `dissolve`, `create_group` |
| `SdkError` | Stable | `#[non_exhaustive]`, so adding a variant is not breaking |
| `property::{Fetchable, FetchableWithContext, GroupFetchable}` | Unstable | Extension points for this crate, not part of the consumer vocabulary |
| `test-support` items | Unstable | Test scaffolding; shape follows the tests' needs |

### 13.2 Breaking Changes

**Policy**: pre-1.0, breaking changes may land in minor versions. Post-1.0, semantic versioning.

**Current deprecations**, all `#[deprecated(since = "0.2.0")]`:

| Deprecated | Replacement | Line |
|------------|-------------|------|
| `get_speaker_by_name()` | `speaker()` | `src/system.rs:629` |
| `get_speaker_by_id()` | `speaker_by_id()` | `src/system.rs:714` |
| `get_group_by_id()` | `group_by_id()` | `src/system.rs:904` |
| `get_group_by_name()` | `group()` | `src/system.rs:979` |
| `get_group_for_speaker()` | `group_for_speaker()`, or `speaker.group()` | `src/system.rs:939` |

### 13.3 Version

Published as `sonos-sdk`, versioned from the workspace (`version.workspace = true`), so it moves
in lockstep with every other crate in the SDK. A per-crate `CHANGELOG.md` sits beside the
manifest.

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Blocking discovery | `SonosSystem::new()` can take 3 s on a cold cache | Accept it, or keep the cache warm | An async or incremental constructor |
| Blocking `fetch()` and commands | Each call occupies the calling thread for a round trip | Call from a worker thread | An async variant, which needs an async `soap-client` |
| No speaker may be added after construction | A speaker that appears later is invisible until a lookup miss triggers rediscovery | Look the speaker up by name — the miss path rediscovers | An explicit refresh method |
| Rediscovery is rate-limited to once per 30 s per system | A speaker that appears twice within the window stays invisible | Wait out the cooldown | Make the cooldown configurable |
| `Group::set_volume` takes `u16`, `Speaker::set_volume` takes `u8` | Surprising asymmetry | None needed | It mirrors the UPnP argument types; changing it would hide that |
| Multi-subnet households are not supported for events | Speakers on a subnet the callback URL cannot reach fall back to polling | Polling still delivers state, less promptly | Per-subscription callback URLs; see [callback-server.md](callback-server.md) §14.1 |
| No content browsing | Cannot list or search a music library | Use `sonos-api` directly | ContentDirectory support |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| `SdkError::WatcherClosed` is never constructed | `src/error.rs:22` | Low | Remove it; `#[non_exhaustive]` makes that non-breaking |
| A `PropertyHandle::watch()` doc comment refers to `system.configure_events()`, which does not exist | `src/property/handles.rs:76` | Low | Delete the reference |
| `from_discovered_devices` changes visibility with the `test-support` feature | `src/system.rs:158-168` | Low | Make it consistently `pub(crate)` and route tests through the offline constructors |
| `ensure_topology()` polls speakers serially until one answers | `src/system.rs:796` | Medium | With every speaker unreachable this costs 15 s per speaker; poll concurrently or bound the total |
| Command coverage is uneven across services | `src/speaker.rs`, `src/group.rs` | Low | AVTransport and RenderingControl are well covered; GroupManagement is reached only indirectly |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| `Mute`, `Bass`, `Treble`, `Loudness`, `Position` in the prelude | P2 | They are already handles on `Speaker`; the prelude lags the surface | — |
| Explicit `refresh()` on `SonosSystem` | P1 | Adding a speaker mid-session currently depends on a lookup miss | — |
| Concurrent topology prefetch | P2 | Bounds the worst case in §14.2 | — |
| Async variant | P2 | Lets the SDK be used from an async application without `spawn_blocking` | An async `soap-client` |
| ContentDirectory support | P2 | Library browsing and search | New `sonos-api` service |

### 15.2 Open Questions

- [ ] **Should `fetch()` fall back to the cached value on a network error?** It currently returns
      the error. Returning stale data silently would be worse for a caller that asked for
      freshness, but a `fetch_or_cached()` might be worth having explicitly.
- [ ] **Should property handles carry `set()`?** Commands live on `Speaker` and `Group` today
      (§4.1). A `volume.set(35)` would read well, but several commands map to no property at all,
      so the two surfaces would not be symmetric.
- [ ] **Should the discovery cache record topology as well as devices?** It would remove the
      topology poll from warm starts, at the cost of a staleness class that is harder to detect
      than a missing speaker.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| Property handle | `PropertyHandle<P>` or `GroupPropertyHandle<P>` — a field on `Speaker`/`Group` carrying `get`/`fetch`/`watch` for one property |
| DOM-like API | Properties accessed as fields rather than through a generic getter |
| `WatchHandle` | RAII lease on a subscription that also reads the property live (§4.3) |
| Watch mode | Whether a watch is served by UPnP events, by polling, or by the cache alone |
| Grace period | The 50 ms window after the last `WatchHandle` for an `(ip, service)` drops, during which the subscription survives and can be reclaimed |
| Coordinator | The speaker that owns playback state for its group |
| Satellite | A speaker marked `Invisible="1"` in topology — a home-theater surround or sub |
| `ChangeIterator` | Blocking per-subscriber iterator over `ChangeEvent`; each `system.iter()` returns an independent one that receives every event |
| Offline system | A `SonosSystem` built by a `test-support` constructor, with both network paths closed (§8.5) |

### B. References

- [sonos-state specification](sonos-state.md) — the cache, the change stream, coordinator
  resolution and write ordering
- [sonos-api specification](sonos-api.md) — UPnP operations and SOAP execution
- [sonos-event-manager specification](sonos-event-manager.md) — subscription reference counting
  and the grace period
- [sonos-discovery specification](sonos-discovery.md) — SSDP discovery
- [callback-server specification](callback-server.md) — inbound NOTIFY handling and callback
  address selection
- [docs/STATUS.md](../STATUS.md) — per-service implementation status across the layers
