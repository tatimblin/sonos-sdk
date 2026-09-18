# sonos-event-manager Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

The Sonos SDK requires efficient management of UPnP event subscriptions across multiple speakers and services. Without proper coordination, the following problems arise:

1. **Subscription Proliferation**: Multiple components watching the same property (e.g., Volume on Speaker A) would each create separate UPnP subscriptions, wasting network bandwidth and device resources.

2. **Resource Leaks**: Without lifecycle management, subscriptions could persist indefinitely even when no consumer needs them, leaving unnecessary network connections and renewal timers active.

3. **Complexity Exposure**: The `sonos-stream` crate provides powerful but complex event handling (firewall detection, polling fallback, event enrichment). Higher-level consumers like `sonos-state` need a simpler interface focused on subscription lifecycle.

4. **Coordination Gap**: There's a semantic gap between "I want to watch Volume" (user intent) and "I need a UPnP subscription to RenderingControl service on device X" (implementation detail). Something needs to bridge this gap efficiently.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Reference-counted subscription lifecycle | Ensures subscriptions are created only when needed and cleaned up when no longer used |
| P0 | Thread-safe concurrent access | Multiple async tasks may watch properties simultaneously |
| P1 | Clean abstraction over sonos-stream | Hide EventBroker complexity from sonos-state |
| P1 | Zero subscription duplication | Multiple watchers for same device/service share one subscription |
| P2 | Observable subscription statistics | Enable debugging and monitoring of subscription health |

### 1.3 Non-Goals

- **Event filtering**: The crate provides the raw multiplexed event stream; filtering by property type is handled by `sonos-state`
- **State storage**: This crate manages subscriptions, not the actual state values (that's `sonos-state`)
- **Direct user API**: This is an internal crate; users should use `sonos-state` for reactive state management
- **Per-consumer event streams**: All events flow through a single multiplexed iterator; routing is delegated to consumers

### 1.4 Success Criteria

- [x] First watcher for a (device, service) pair triggers exactly one UPnP subscription
- [x] Subsequent watchers for the same pair increment reference count without network calls
- [x] Last watcher dropping triggers subscription cleanup
- [x] Thread-safe operations under a documented lock order (§4.2)
- [x] Clean integration with sonos-stream's EventBroker

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         sonos-state                                      │
│  (StateManager: get_property / set_property / register_watch / iter)    │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │
                                 │ Uses
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      sonos-event-manager                                 │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                     SonosEventManager                             │   │
│  │  ┌────────────────┐  ┌──────────────────┐  ┌─────────────────┐   │   │
│  │  │   Device       │  │   Reference      │  │   Worker thread │   │   │
│  │  │   Registry     │  │   Counting       │  │   owns the      │   │   │
│  │  │  RwLock<Hash-  │  │  RwLock<HashMap  │  │   EventBroker   │   │   │
│  │  │  Map<IP,Dev>>  │  │  <Key, usize>>   │  │   + a runtime   │   │   │
│  │  └────────────────┘  └──────────────────┘  └─────────────────┘   │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │
                                 │ Delegates to
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         sonos-stream                                     │
│  (Internal: EventBroker, UPnP subscriptions, polling fallback)          │
└─────────────────────────────────────────────────────────────────────────┘
```

**Design Rationale**: The Reference-Counted Observable pattern (similar to RxJS `refCount()`) was chosen because:
1. It naturally maps to the "watch property" use case where multiple UI components may observe the same data
2. It provides automatic resource cleanup without requiring explicit unsubscribe calls
3. It's a well-understood pattern that's proven effective in reactive programming libraries

### 2.2 Module Structure

```
src/
├── lib.rs              # Public API surface, re-exports, prelude
├── manager.rs          # SonosEventManager implementation
├── timer.rs            # Shared grace-period teardown timer (private)
├── worker.rs           # Background worker thread + Command enum
├── iter.rs             # Blocking event iterator
└── error.rs            # Error types (EventManagerError)
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `lib.rs` | Public API, re-exports from dependencies, prelude module | `pub` |
| `manager.rs` | SonosEventManager struct and all subscription management logic | `pub` |
| `timer.rs` | `TeardownTimer`: one thread servicing a deadline-ordered queue of pending teardowns | private |
| `worker.rs` | Background thread owning the tokio runtime and the `EventBroker` | `pub` |
| `iter.rs` | `EventManagerIterator` over the sync event channel | `pub` |
| `error.rs` | EventManagerError enum and Result type alias | `pub` |

`timer.rs` is private on purpose: the grace period is an implementation detail
of the watch lifecycle, and nothing outside this crate should be able to stop,
drain or schedule on it.

### 2.3 Key Types

#### `SonosEventManager`

```rust
pub struct SonosEventManager {
    /// Commands to the background worker, which owns the EventBroker
    command_tx: tokio_mpsc::UnboundedSender<Command>,

    /// Deadline queue for grace-period teardowns, serviced by one thread
    teardown_timer: TeardownTimer,

    /// Events from the background worker (sync channel)
    event_rx: Arc<Mutex<mpsc::Receiver<EnrichedEvent>>>,

    /// Map of device IP addresses to device information
    devices: Arc<RwLock<HashMap<IpAddr, Device>>>,

    /// Reference counting for service subscriptions: (device_ip, service) -> ref_count
    service_refs: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>,

    /// Grace periods in flight, keyed by (ip, service). The AtomicBool is a
    /// claim token, not merely a cancellation flag.
    pending_unsubscribes: Arc<parking_lot::Mutex<PendingUnsubscribes>>,

    /// Watched-property set bridge into sonos-state (set once)
    watch_registry: OnceLock<Arc<dyn WatchRegistry>>,

    _worker: JoinHandle<()>,
}
```

**Purpose**: Central facade that coordinates device registration, subscription lifecycle, and event stream access. The `EventBroker` itself lives on the worker thread, behind the command channel, so this type stays sync.

**Invariants**:
- Reference counts are always non-negative, and an entry is removed rather than left at zero
- A subscription exists in EventBroker if and only if the reference count is > 0, or a grace period is pending for that key
- Device map entries are never removed (devices can be added but not explicitly removed)
- At most one pending teardown per `(ip, service)`, and its token has exactly one claimant
- Every path that takes a key's reference count from 0 to 1 claims the pending teardown token first — `acquire_watch` and `ensure_service_subscribed` alike. They share `service_refs`, so a path that skipped the claim would let a teardown fire under a caller holding a live reference, which is precisely how the second invariant above becomes false
- Locks are taken in the order given in §4.2 and never the reverse

**Ownership**: Created once per application, typically owned by `sonos-state::StateManager`. Handed out as `Arc<SonosEventManager>`; every `WatchGuard` holds one.

#### `WatchRegistry` Trait

```rust
pub trait WatchRegistry: Send + Sync + 'static {
    fn register_watch(&self, speaker_id: &SpeakerId, key: &'static str, service: Service);
    fn unregister_watches_for_service(&self, ip: IpAddr, service: Service);
}
```

**Purpose**: Bridges the event-manager and state-manager for watched-property set management. Defined in sonos-event-manager, implemented by `StateWatchRegistry` in sonos-state. Enables the grace period to clean up watched entries when unsubscribing.

**Implementor contract** (see §4.2 "Callback contract"): `unregister_watches_for_service` is called from the shared teardown thread **while the manager's pending-unsubscribe mutex is held**. Implementations must be short, non-blocking, free of I/O, and must not re-enter `SonosEventManager` — every entry point takes that same mutex. A panic is caught and logged; a hang blocks every subsequent teardown for the life of the process.

**Ordering note**: `acquire_watch` calls `register_watch` *after* it resolves any pending teardown, not before. An implementor may therefore observe `unregister_watches_for_service` immediately followed by `register_watch` for the same service; it must not assume a register always precedes its matching unregister.

#### `TeardownTimer` (private, `timer.rs`)

```rust
pub(crate) struct TeardownTimer {
    shared: Arc<Shared>,   // Mutex<Queue { heap: BinaryHeap<Scheduled>, stopped: bool }> + Condvar
}
```

**Purpose**: One thread per manager, servicing a deadline-ordered queue of pending teardowns, so that releasing *n* watches costs *n* heap pushes rather than *n* OS threads. An immediate-mode TUI holding ~9 handles at 60 fps produces ~540 releases per second, which is the load this exists to absorb (`src/timer.rs:1-21`).

**Why a dedicated thread rather than the worker's runtime**: the worker runs `new_current_thread`, and the UPnP subscribe path reaches blocking `ureq` calls inside `async fn`s without `spawn_blocking`. One SUBSCRIBE to an unreachable speaker wedges that runtime for the full ~5s connect timeout, during which a `tokio::time::sleep` scheduled on it would not fire; the dedicated thread fires the same teardown at ~50 ms. Teardown timing must be independent of broker health.

**Invariants**:
- The thread holds only an `Arc<Shared>` and a `WeakUnboundedSender<Command>`, so it borrows no manager state and cannot outlive anything it dereferences. The weak sender is what lets the worker still observe its command channel closing.
- `stop()` is one-way and belongs to `Drop`; `drain()` clears the queue without latching and is what `shutdown()` uses.
- `schedule()` never panics and returns the teardown back to the caller on refusal. Exactly two refusals: the timer is stopped, or `now + delay` is not a representable `Instant`.
- A spawn failure latches `stopped`, so the caller falls back to firing inline rather than silently queueing work nothing will ever run.
- The thread body wraps `run` in a `catch_unwind` restart loop; a panic costs one iteration, not the service.
- `Drop` signals and does **not** join — joining would block a `Drop` on an implementor-supplied callback with no upper bound.

#### `PendingTeardown` (private, `manager.rs`)

```rust
pub(crate) struct PendingTeardown {
    ip: IpAddr,
    service: Service,
    delay: Duration,                                   // GRACE_PERIOD today
    cancelled: Arc<AtomicBool>,                        // the claim token
    pending: Arc<parking_lot::Mutex<PendingUnsubscribes>>,
    registry: Option<Arc<dyn WatchRegistry>>,
}
```

**Purpose**: Everything a deferred teardown needs to resolve itself. Carries the shared pending-map handle rather than a manager reference, so the timer thread can never keep the manager alive.

**Invariants**:
- `fire()` claims by `swap`ping the token under the pending-map mutex, so exactly one of expiry and cancellation wins. It returns `true` only for the winner — including when the registry callback panicked.
- `delay` rides on the teardown rather than being read from a constant, so making the grace period configurable is later plumbing only.
- The mutex is held **across** the registry callback. This is the ordering guarantee `acquire_watch` depends on; see §4.2.

#### `WatchGuard`

```rust
#[must_use = "dropping the guard immediately starts the grace period"]
pub struct WatchGuard {
    event_manager: Arc<SonosEventManager>,
    speaker_id: SpeakerId,
    property_key: &'static str,
    ip: IpAddr,
    service: Service,
}
```

**Purpose**: RAII guard returned by `acquire_watch()`. Each instance holds one ref count on the (ip, service) subscription. `Drop` calls `release_watch()` which never panics. Not `Clone`, not `Copy` — each guard is exactly one subscription hold.

**Invariants**:
- Dropping a `WatchGuard` decrements the service ref count, and never panics.
- When the ref count reaches zero, a `PendingTeardown` is scheduled on the shared `TeardownTimer` — a mutex, a heap push and a condvar notify. No thread is created.
- If `acquire_watch()` is called within the 50 ms window, it claims the teardown's token under the pending-map mutex and the subscription is reused.
- If the timer refuses the schedule (thread never spawned, or an unrepresentable deadline), the teardown fires **inline** inside the drop. It is never dropped on the floor.

#### `EventManagerError`

```rust
// src/error.rs:5
#[derive(Error, Debug)]
pub enum EventManagerError {
    BrokerInitialization(#[from] sonos_stream::BrokerError),
    DeviceRegistration { device_ip, service, source },
    DeviceUnregistration { device_ip, service, source },
    ConsumerCreation { device_ip, service },
    DeviceNotFound(IpAddr),
    SubscriptionNotFound { device_ip, service },
    ChannelClosed,
    Discovery(#[from] sonos_discovery::DiscoveryError),
    Sync(String),
    LockPoisoned,
    InvalidIpAddress(String),
    WorkerDisconnected,
}
```

**Purpose**: covers every failure mode in subscription management. 12 variants; **not**
`#[non_exhaustive]`, so a downstream exhaustive `match` breaks when a variant is added.
`WorkerDisconnected` is the one every command-sending method can return: the worker thread owns
the `EventBroker`, so a closed command channel is the only way a subscribe or unsubscribe can
fail synchronously. `Result<T>` (`src/error.rs:73`) is the crate-wide alias.

---

## 3. Code Flow

### 3.1 Primary Flow: Service Subscription (First Watcher)

```
┌───────────────────┐     ┌────────────────────┐     ┌─────────────────┐
│  sonos-sdk        │────▶│  SonosEventManager │────▶│  worker thread  │
│  handle.watch()   │     │  acquire_watch()   │     │  EventBroker::  │
│                   │     │  -> WatchGuard     │     │  register_      │
└───────────────────┘     └────────────────────┘     │  speaker_service│
       │                          │                  └─────────────────┘
       ▼                          ▼                          ▲
   User holds             ref count 0 -> 1                   │
   a WatchGuard           + claim pending teardown   Command::Subscribe
```

**Step-by-step** (`acquire_watch`, `src/manager.rs:470`):

1. **Increment** (`src/manager.rs:478-493`): take the `service_refs` write lock, bump
   `(device_ip, service)`, record whether it was zero, and **release the lock**.
2. **Claim** (`src/manager.rs:503`): call `claim_pending_teardown()` (`:564`). Winning the claim
   means a grace period was in flight and the subscription is still live, so it is reused.
3. **Register the watch** (`src/manager.rs:521`): only now call
   `WatchRegistry::register_watch`. Registering before the claim resolves would leave a window
   in which an in-flight `fire()` wipes the watched set under a live guard (`:505-520`).
4. **Subscribe if needed** (`src/manager.rs:526-544`): send `Command::Subscribe` only when the
   count went 0 -> 1 *and* no teardown was cancelled. A closed command channel yields
   `EventManagerError::WorkerDisconnected`.
5. **Return** a `WatchGuard` holding `Arc<SonosEventManager>` plus the key.

`ensure_service_subscribed()` (`src/manager.rs:726`) runs the same increment-then-claim
sequence without step 3, because it has no `(speaker_id, property_key)` pair to register
(`:722-725`).

### 3.2 Secondary Flow: Service Subscription (Subsequent Watchers)

When the reference count is already above zero:

1. **Increment** (`src/manager.rs:478`): the count goes from *n* to *n+1*; `was_zero` is false.
2. **No claim** (`src/manager.rs:503`): `should_subscribe` is false, so no teardown is claimed —
   there cannot be one in flight while the count is non-zero.
3. **Register the watch** (`src/manager.rs:521`): still happens, since watches are per
   `(speaker_id, property_key)` rather than per subscription.
4. **Return** (`src/manager.rs:546`): a second `WatchGuard`, with no command sent and no
   network call.

### 3.3 Tertiary Flow: Subscription Release

```
┌───────────────────┐     ┌────────────────────┐     ┌─────────────────┐
│  WatchGuard::drop │────▶│  release_watch()   │────▶│ TeardownTimer   │
│                   │     │  ref count -> 0    │     │ +50ms deadline  │
└───────────────────┘     └────────────────────┘     └────────┬────────┘
                                                              │ expiry
                                                              ▼
                                                   ┌─────────────────────┐
                                                   │ PendingTeardown::   │
                                                   │ fire(): Unsubscribe │
                                                   │ + unregister watches│
                                                   └─────────────────────┘
```

**Step-by-step** (`release_watch`, `src/manager.rs:584`):

1. **Decrement** (`src/manager.rs:595`): `saturating_sub(1)` under the `service_refs` write
   lock. At zero the entry is removed (`:606`) rather than left behind.
2. **Mint a claim token** (`src/manager.rs:618`): a fresh `Arc<AtomicBool>`, inserted into
   `pending_unsubscribes` under an explicitly named-and-dropped guard (`:628-630`) so the
   `pending_unsubscribes -> timer.queue` lock order stays visible.
3. **Schedule** (`src/manager.rs:644`): hand a `PendingTeardown` to the shared
   `TeardownTimer`. On refusal, fire it **inline** (`:662`) rather than dropping it.
4. **Expiry** (`src/manager.rs:117`): `PendingTeardown::fire()` claims the token under the
   pending-map mutex, sends `Command::Unsubscribe` (`:148`), then calls
   `unregister_watches_for_service` (`:153`) — both still holding that mutex, which is the
   ordering `acquire_watch` depends on.

`release_service_subscription()` (`src/manager.rs:797`) is deliberately **asymmetric**: it
unsubscribes the instant the count reaches zero, with no grace period and no watched-set
unregister. Both differences are explained at `src/manager.rs:764-796` — this path is not the
churning one, and a service-wide unregister here would wipe watched pairs belonging to live
`WatchGuard`s.

### 3.4 Error Flow

```
sonos_stream::BrokerError ──▶ EventManagerError::DeviceRegistration ──▶ sonos_state::StateError
                              EventManagerError::BrokerInitialization
```

**Error handling philosophy**: Errors are wrapped with context (device IP, service) to aid debugging. The `thiserror` derive provides automatic `From` conversions for upstream errors.

---

## 4. Features

### 4.1 Feature: Reference-Counted Subscriptions

#### What

A `usize` count per `(device_ip, service)`, held in `service_refs: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>` (`src/manager.rs:399`), tracks how many consumers need each subscription. The count drives subscription creation and cleanup.

#### Why

Without reference counting, either:
- Each watcher creates its own subscription (wasteful)
- A single shared subscription with manual lifecycle management (error-prone)
- Complex pub/sub routing per consumer (over-engineered)

Reference counting provides automatic, correct lifecycle management.

#### How

Every method here is synchronous — the `EventBroker` and its runtime live on the worker
thread, reached through a command channel.

```rust
// First watcher - sends Command::Subscribe
manager.ensure_service_subscribed(device_ip, Service::RenderingControl)?;
// count: 0 -> 1

// Second watcher - increments count only
manager.ensure_service_subscribed(device_ip, Service::RenderingControl)?;
// count: 1 -> 2, no command, no network call

// Second watcher dropped
manager.release_service_subscription(device_ip, Service::RenderingControl)?;
// count: 2 -> 1, subscription remains

// First watcher dropped
manager.release_service_subscription(device_ip, Service::RenderingControl)?;
// count: 1 -> 0, entry removed, Command::Unsubscribe sent
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| One `RwLock<HashMap<Key, usize>>` | Per-entry atomics in a concurrent map | The count is never read without also deciding what to do about it, so the critical section is a lock either way. One lock keeps the documented order (§4.2) checkable by reading one file |
| `saturating_sub` on release | `panic!` / wrapping decrement | An unbalanced release is a caller bug, but wrapping to `usize::MAX` would pin the subscription open forever — strictly the worse failure |
| Remove the entry at zero | Leave a zero-valued entry | "Present" and "held" then mean the same thing, so the invariant in §2.3 is checkable by lookup |
| Commands over a channel | Call the broker directly under the lock | The broker is async and its subscribe path blocks; sending a command keeps every public method sync and keeps network latency out of the critical section |

### 4.2 Feature: RAII Watch Guards with Grace Period

#### What

`acquire_watch()` returns a `WatchGuard` that holds one ref count. When dropped, a 50ms grace period prevents unnecessary unsubscribe/resubscribe churn. Re-acquiring within 50ms cancels the pending unsubscribe.

#### Why

TUI frameworks like ratatui reconstruct widgets every frame. Calling `watch()` in `draw()` methods would cause subscription churn without a grace period. The RAII pattern ensures subscriptions are tied to scope lifetime.

#### How

```rust
// acquire_watch() increments the ref count and returns a WatchGuard
let guard = event_manager.acquire_watch(&speaker_id, "volume", ip, Service::RenderingControl)?;

// ... use guard ...

// WatchGuard::Drop calls release_watch()
// If the ref count hits 0:
//   - insert a claim token into pending_unsubscribes, release that mutex
//   - TeardownTimer::schedule(PendingTeardown) — mutex, heap push, condvar notify
//   - on refusal (no timer thread, or an unrepresentable deadline) fire inline
// The shared timer thread, once the deadline passes:
//   - pops the teardown, then outside the queue lock calls fire()
//   - fire() takes the pending-map mutex and swaps the token:
//       already true  -> a re-acquire claimed it; do nothing
//       was false     -> remove the entry, send Command::Unsubscribe,
//                        then clear the watched set via WatchRegistry,
//                        all still under that mutex
```

#### The claim protocol

The grace window is a race between two events on one `(ip, service)`: the timer
expiring, and an `acquire_watch` arriving to reuse the subscription. Both swap
the *same* `Arc<AtomicBool>` while holding the pending-map mutex, and `swap`
returns the previous value, so exactly one observes `false` and owns the
outcome. Never zero winners (a stale map entry that made the next acquire skip
its `Subscribe`) and never two (an unsubscribe underneath a live guard).

Ordering falls out of the same mutex:

- **Commands.** `Unsubscribe` is sent *inside* the mutex, so a racing
  `Subscribe` can only enqueue behind it. The channel is FIFO, so the worker
  sees unsubscribe-then-subscribe and ends up subscribed — never the reverse.
- **The watched set.** `acquire_watch` calls `register_watch` only *after* its
  claim resolves on that mutex. It therefore either cancels the teardown, or
  blocks until the unregister has finished and registers on top of it.

#### Lock ordering

```
service_refs  ->  pending_unsubscribes  ->  timer.queue
                  pending_unsubscribes  ->  WatchRegistry (implementor's locks)
```

Taken in this order on every path and never the reverse. `release_watch` binds
a named guard on `pending_unsubscribes` and drops it explicitly before calling
`schedule`; `shutdown` drains-and-claims under that mutex, releases it, and only
then calls `TeardownTimer::drain`. The timer thread holds only one of
`timer.queue` and `pending_unsubscribes` at a time — `run` pops under the queue
lock and fires outside it.

#### Callback contract

`PendingTeardown::fire` holds the pending-map mutex across
`unregister_watches_for_service`. Dropping it early would look like an
improvement and is not: it opens a window in which a re-acquire completes, hands
back a live guard over a live subscription, and *then* the teardown wipes that
pair out of the watched set, so every event for it is filtered until the guard
drops. Holding the mutex is what makes that impossible.

The price is that implementor code runs under a manager lock, which bounds
`drop`, `shutdown()` and `acquire_watch`'s claim by the callback's duration.
Hence the contract in §2.3: short, non-blocking, no I/O, no re-entry. A panic is
contained by `call_unregister` and the timer's restart loop; a hang is not
recoverable.

Offloading the callback to another queue is **declined**, not deferred: an
offload puts `unregister` on a queue `register_watch` is not on, which
reintroduces exactly the reordering above. A registry that needs slow work must
defer it internally, where it can order its own mutations.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| One shared timer thread | One `std::thread::spawn` per release | ~540 releases/sec on the TUI path made per-release threads the dominant cost: 27.0–28.8 us median release against 2.7–3.2 us. A release is now a mutex, a heap push and a notify |
| Dedicated OS thread | `tokio::time::sleep` on the worker runtime | The worker is `new_current_thread` and blocks in `ureq`; one SUBSCRIBE to an unreachable speaker wedged it 5,002.7 ms while the dedicated thread fired at 50.4 ms |
| `AtomicBool` claim token, swapped under a mutex | Channel-based cancel; bare `AtomicBool` flag | The swap's return value *is* the claim, which is what makes "exactly one winner" hold. A bare flag cannot order the map mutation against the callback |
| Registry callback under the pending mutex | `drop(pending)` first; offload to a queue | Ordering the watched-set mutation against `acquire_watch` — see "Callback contract" |
| `catch_unwind` + in-thread restart loop | Supervisor thread; let it die | A supervisor costs a thread, a join handle, a liveness protocol and a shutdown race with `TeardownTimer::drop`, and buys nothing an in-thread restart does not |
| Inline teardown on schedule refusal | Leak the teardown; panic in `Drop` | Panicking in `Drop` can abort the process and leaking silently stops releasing subscriptions. Inline is the only option that does neither |
| `shutdown()` drains the timer | `shutdown()` stops the timer | `shutdown()` is public and leaves the manager usable, but stopping is one-way, so every later release would silently skip its teardown. Only `Drop` may latch |
| `parking_lot` locks | `std::sync` locks | Non-poisoning, so a panicking callback leaves usable state; guards unlock on unwind |
| `release_watch()` returns `()` | Returns `Result` | Must never panic in Drop; errors logged internally |
| `GRACE_PERIOD` fixed at 50 ms | Configurable | Deliberately deferred; `delay` already rides on `PendingTeardown`, so it is later plumbing only |

### 4.3 Feature: Device Registry

#### What

A thread-safe mapping from IP addresses to discovered device information.

#### Why

Higher-level code works with device identifiers, but network operations need IP addresses. The registry provides this translation.

#### How

```rust
// Add devices from discovery
let devices = sonos_discovery::get();
event_manager.add_devices(devices)?;          // src/manager.rs:675

// Query devices
let all_devices = event_manager.devices();    // src/manager.rs:691
let specific = event_manager.device_by_ip(ip); // src/manager.rs:696
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| No device removal API | Full CRUD operations | Simpler model; device removal is rare and can be handled by rebuilding the manager |
| `Arc<RwLock<HashMap>>` | A concurrent map | The registry is written once at startup and read thereafter, so per-entry locking buys nothing |

### 4.4 Feature: Multiplexed Event Stream

#### What

A single `EventIterator` provides access to ALL events from ALL registered devices and services.

#### Why

- Simplifies consumer code (one event loop, not N per device)
- Each event is tagged with `speaker_ip` and `service` for routing
- Matches the state management model where one processor handles all updates

#### How

`iter()` (`src/manager.rs:864`) hands out an `EventManagerIterator` (`src/iter.rs:15`) over a
shared `std::sync::mpsc::Receiver`, so it blocks rather than awaiting:

```rust
for enriched_event in event_manager.iter() {
    // enriched_event.speaker_ip and enriched_event.service for routing
    match enriched_event.service {
        Service::RenderingControl => handle_volume_mute(enriched_event),
        Service::AVTransport => handle_playback(enriched_event),
        _ => {}
    }
}
```

`EventManagerIterator` is `Clone` (`src/iter.rs:73`) and every clone shares one receiver, so
concurrent consumers **compete** for events rather than each seeing all of them. It also offers
`recv()` (`:28`), `try_recv()` (`:35`), `recv_timeout()` (`:42`), `try_iter()` (`:49`) and
`timeout_iter()` (`:56`). Per-subscriber fan-out is `sonos-state`'s job — see
[sonos-state.md](sonos-state.md) §4.1b.

---

## 5. Data Model

### 5.1 Core Data Structures

#### Subscription Key

```rust
// Composite key for subscription tracking
type SubscriptionKey = (IpAddr, Service);
```

**Lifecycle**:
1. **Creation**: When first watcher requests a subscription
2. **Mutation**: Reference count changes via atomic operations
3. **Destruction**: When reference count reaches zero

**Memory considerations**: each entry is a `(IpAddr, Service)` key plus a `usize` count, on the
order of tens of bytes, in one `HashMap` behind an `RwLock`.

#### Device Entry

```rust
// From sonos_discovery::Device (sonos-discovery/src/lib.rs:53)
pub struct Device {
    pub id: String,          // UDN, e.g. "uuid:RINCON_000E58A0123456"
    pub name: String,
    pub room_name: String,
    pub ip_address: String,
    pub port: u16,           // always 1400
    pub model_name: String,
}
```

**Lifecycle**:
1. **Creation**: Via `add_devices()` from discovery results
2. **Mutation**: None (read-only after insertion)
3. **Destruction**: When manager is dropped

### 5.2 State Transitions

```
                              ensure_service_subscribed()
                             ┌──────────────────────────┐
                             │                          │
                             ▼                          │
┌─────────────┐    first    ┌─────────────┐   again   ┌─────────────┐
│ Unsubscribed│────────────▶│ Subscribed  │◀─────────▶│ Subscribed  │
│  (count=0)  │             │  (count=1)  │           │  (count>1)  │
└─────────────┘             └─────────────┘           └─────────────┘
       ▲                          │                          │
       │   release_service_       │                          │
       │   subscription()         │                          │
       │   (count→0)              ▼                          │
       └──────────────────────────┴──────────────────────────┘
                             release (count>1)
```

**Invariants per state**:
- **Unsubscribed**: no entry in `service_refs`, no active EventBroker registration, and no grace period pending for the key
- **Subscribed (count=1)**: Entry exists, EventBroker has active subscription
- **Subscribed (count>1)**: Entry exists with count > 1, still one EventBroker subscription

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `sonos-stream` | EventBroker for UPnP events | Core event infrastructure; provides transparent event/polling switching |
| `sonos-api` | Service enum, device types | Shared type definitions across SDK |
| `sonos-discovery` | Device type | Device information from network discovery |
| `tokio` | Async runtime, owned by the worker thread | The `EventBroker` is async; the runtime is confined to `worker.rs` so this crate's own API stays sync |
| `parking_lot` | Non-poisoning `RwLock`/`Mutex`/`Condvar` | A panicking `WatchRegistry` callback must leave usable state, and guards must unlock on unwind (§4.2) |
| `thiserror` | Error derive | Clean error type definitions |
| `tracing` | Logging | Debug visibility into subscription lifecycle |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| `sonos-state` | `StateManager` wraps `SonosEventManager`, calls `ensure_service_subscribed()` for property watches | Internal API; changes coordinated with sonos-state |

### 6.3 External Systems

This crate does not directly interact with external systems. All Sonos device communication is delegated to `sonos-stream`, which in turn uses `sonos-api` for SOAP calls and `callback-server` for event reception.

---

## 7. Error Handling

### 7.1 Error Types

```rust
#[derive(Error, Debug)]
pub enum EventManagerError {
    #[error("Failed to initialize event broker: {0}")]
    BrokerInitialization(#[from] sonos_stream::BrokerError),

    #[error("Failed to register device {device_ip} for service {service:?}: {source}")]
    DeviceRegistration {
        device_ip: IpAddr,
        service: sonos_api::Service,
        #[source]
        source: sonos_stream::BrokerError,
    },

    #[error("Failed to unregister device {device_ip} for service {service:?}: {source}")]
    DeviceUnregistration {
        device_ip: IpAddr,
        service: sonos_api::Service,
        #[source]
        source: sonos_stream::BrokerError,
    },

    #[error("Failed to create event consumer for {device_ip} service {service:?}")]
    ConsumerCreation {
        device_ip: IpAddr,
        service: sonos_api::Service,
    },

    #[error("Device with IP {0} not found")]
    DeviceNotFound(IpAddr),

    #[error("Subscription for device {device_ip} service {service:?} not found")]
    SubscriptionNotFound {
        device_ip: IpAddr,
        service: sonos_api::Service,
    },

    #[error("Event channel has been closed")]
    ChannelClosed,

    #[error("Device discovery failed: {0}")]
    Discovery(#[from] sonos_discovery::DiscoveryError),

    #[error("Internal synchronization error: {0}")]
    Sync(String),

    #[error("Internal lock was poisoned")]
    LockPoisoned,

    #[error("Invalid IP address: {0}")]
    InvalidIpAddress(String),

    #[error("Background worker has disconnected")]
    WorkerDisconnected,
}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Context preservation | Structured error variants with device_ip and service fields | Debugging requires knowing which device/service failed |
| Error chaining | `#[source]` attribute on wrapped errors | Preserve root cause while adding context |
| Semantic categorization | Separate variants for registration vs unregistration | Different recovery strategies may apply |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `BrokerInitialization` | No | Fatal; cannot create event infrastructure |
| `DeviceRegistration` | Yes | Retry with exponential backoff; polling fallback handled by EventBroker |
| `DeviceNotFound` | Yes | Re-run discovery or check IP address |
| `SubscriptionNotFound` | Yes | Warning only; may indicate double-release bug |
| `ChannelClosed` | No | Fatal; event stream terminated |
| `Sync` | Maybe | Internal error; may indicate lock poisoning |
| `LockPoisoned` | No | A `std::sync` lock unwound through a panic |
| `InvalidIpAddress` | Yes | Caller supplied an unparseable address |
| `WorkerDisconnected` | No | The worker thread is gone; the manager can no longer subscribe or unsubscribe |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    ┌───────────────────┐
                    │  Integration      │  Manual testing with real devices
                    │  (examples/)      │
                    └─────────┬─────────┘
              ┌───────────────┴───────────────┐
              │       Unit Tests              │  ~80% coverage goal
              │   (src/manager.rs tests)      │
              └───────────────────────────────┘
```

The crate is thin (bridges sonos-state and sonos-stream), so testing focuses on:
1. Reference counting logic correctness
2. Device registry operations
3. Integration via the smart_dashboard example

### 8.2 Unit Tests

**Location**: inline `#[cfg(test)]` modules — 30 tests in total: `src/manager.rs` (21),
`src/timer.rs` (4), `src/iter.rs` (4), `src/worker.rs` (1). All are plain `#[test]`; none is a
`#[tokio::test]`, since the crate's whole public API is sync.

**What to test**:
- [x] Initial subscription state (not subscribed)
- [x] Device management (add, query, lookup)
- [x] Reference count increment/decrement
- [x] First-subscription triggers registration
- [x] Last-release triggers cleanup

**Watch lifecycle and teardown timer**:

| Test | What it pins down |
|------|-------------------|
| `test_grace_period_cancelled_by_reacquire` | A re-acquire inside the window keeps the subscription |
| `test_grace_period_fires_after_timeout` | Expiry tears down — and does so while the worker runtime is wedged in `ureq`, which is the timer-independence assertion |
| `test_expiry_then_reacquire_must_resubscribe` | A fired teardown clears its own entry, so the next acquire resubscribes |
| `test_cancel_then_late_expiry_keeps_subscription` | A claimed teardown firing late does nothing at all |
| `test_cancel_and_expiry_have_exactly_one_winner` | 500 contended rounds; per-round unregister delta is exactly `usize::from(fired)` |
| `test_expiry_racing_reacquire_keeps_the_watch_registered` | The re-acquire's `register_watch` lands *after* the expiry's unregister |
| `test_registry_panic_does_not_kill_the_timer` | A panicking callback costs one teardown, not the thread — and is contained by `call_unregister` specifically, asserted as `TeardownTimer::restarts() == 0` |
| `test_timer_thread_restarts_after_a_panic` | The second layer: a panic that escapes `run` restarts the loop rather than ending the service. Unreachable through the registry, so the panic is injected |
| `test_inline_teardown_contains_a_panicking_registry` | The inline path has no restart loop behind it, so `call_unregister` is all that keeps a registry panic out of `WatchGuard::drop` |
| `test_ensure_subscribed_inside_grace_window_claims_the_teardown` | `ensure_service_subscribed` claims the pending token instead of subscribing alongside it |
| `test_inline_teardown_when_timer_unavailable` | With no timer, the teardown resolves inside the drop — asserted without sleeping |
| `test_release_after_shutdown_still_unregisters` | `shutdown()` does not latch the grace mechanism off — asserted on the *pending* grace period, since the inline fallback reaches the same end state under `stop()` |
| `test_guard_drop_with_disconnected_worker` | Dropping a guard after shutdown still clears the watched set, and does so after a real grace period |
| `test_shutdown_drains_pending_grace_timers` | Shutdown tears down exactly once, and stays at once |
| `test_manager_drop_during_teardown_fire` | Dropping the manager mid-teardown completes rather than deadlocking |
| `test_immediate_mode_churn_costs_no_threads` | Median release under 8 us over 1,000 cycles, plus a Linux-only bound on the *growth* in process threads across those cycles |
| `test_unrepresentable_deadline_is_refused` | `Duration::MAX` is refused, not panicked on |
| `test_schedule_after_stop_is_refused` | A stopped timer hands the teardown back |
| `test_earliest_deadline_pops_first` | The heap is min-by-deadline |

**Testing note**: tests that need a deterministic interleaving drive
`PendingTeardown::fire` by hand (`in_flight_teardown`) and take the queued copy
off the real timer with `TeardownTimer::drain`, rather than racing a sleep
against the 50 ms grace period.

**Port allocation**: each test that builds a manager takes a disjoint
`with_callback_ports` range so parallel runs cannot collide. 4000–6100 are in
use; new tests continue from 6100.

### 8.3 Integration Tests

**Location**: `examples/smart_dashboard.rs`

**Prerequisites**:
- [x] At least one Sonos device on the network
- [x] Network allows UPnP callbacks (or polling fallback)

**What to test**:
- [x] End-to-end property watching through sonos-state
- [x] Multiple watchers sharing subscriptions
- [x] Automatic cleanup when watchers are dropped

### 8.4 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| `EventBroker` | Real broker in tests (no mocking) | N/A — uses actual sonos-stream |
| `Device` | Inline struct construction | `src/manager.rs:1228` |
| `PendingTeardown` | `test_support::dummy_teardown()` | `src/manager.rs:207` |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Measured | Rationale |
|--------|--------|----------|-----------|
| `release_watch` (ref count → 0, teardown scheduled) | < 8 us median | 0.92–1.00 us alone and 2.71–3.17 us under the full suite on a cold binary (Apple M-series); 1.71 us on Linux CI | The immediate-mode TUI path runs ~9 handles at 60 fps ≈ 540 releases/sec |
| First subscription latency | < 500ms | not measured | Includes UPnP network round-trip |
| Memory per subscription | < 100 bytes | not measured | Support many devices without excessive memory |
| Teardown latency under a wedged worker | ≈ `GRACE_PERIOD` | ~50 ms | Teardown timing must not depend on broker health, and the worker's runtime can be wedged for the full ~5s `ureq` connect timeout |

`test_immediate_mode_churn_costs_no_threads` (`src/manager.rs:2087`) enforces the median bound
and reports the measured numbers through `eprintln!`. On Linux it additionally bounds the
*growth* in process threads across 1,000 release cycles, which is what pins the shared-timer
design in place.

### 9.2 Critical Paths

1. **Reference Count Update** (`acquire_watch` / `release_watch`)
   - **Complexity**: O(1) amortized — `parking_lot::RwLock<HashMap<(IpAddr, Service), usize>>`
   - **Bottleneck**: the write lock, held for the increment only
   - **Note**: the counts are plain `usize` under one `RwLock`. The decision to subscribe or tear down depends on the transition through zero, which has to be observed atomically with the update

2. **Teardown Scheduling** (`release_watch` → `TeardownTimer::schedule`)
   - **Complexity**: O(log n) in the number of pending teardowns
   - **Bottleneck**: two uncontended mutexes and, only when the new deadline is the earliest, a condvar notify
   - **Optimization**: one thread for all teardowns instead of one per release

3. **Teardown Expiry** (`TeardownTimer::run` → `PendingTeardown::fire`)
   - **Complexity**: O(log n) to pop, plus the registry callback
   - **Bottleneck**: the pending-map mutex, held across the callback by design (§4.2)

4. **Event Iteration**
   - **Complexity**: O(1) per event
   - **Bottleneck**: Channel receive
   - **Optimization**: Unbounded channel avoids backpressure blocking

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| UPnP subscriptions | First ensure_service_subscribed() | Last release_service_subscription() | Yes - via reference counting |
| Event channels | Manager construction | Manager drop | No - single channel |
| Device entries | add_devices() | Never explicitly | Yes - HashMap retains all |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious device injection | Low | Medium | Devices only added via discovery (SSDP) |
| Reference count manipulation | Very Low | Low | Counts mutate only under the `service_refs` write lock; internal API only |
| DoS via subscription spam | Low | Medium | sonos-stream has max_registrations limit |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Device IP addresses | Low | Local network only; not transmitted externally |
| Device names | Low | User-chosen names; not sensitive |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| Device IP from discovery | `str::parse::<IpAddr>()`, failure yields `EventManagerError::InvalidIpAddress` | `src/manager.rs:675` (`add_devices`) |
| Service enum | Type-safe enum from sonos-api | Compile-time |

---

## 11. Observability

### 11.1 Logging

| Level | What's Logged | Example |
|-------|--------------|---------|
| `debug` | Reference count transitions | "Service reference count for 192.168.1.100 RenderingControl: 0 -> 1" |
| `debug` | Registration with EventBroker | "Registered RenderingControl for device 192.168.1.100" |
| `debug` | Manager drop statistics | "SonosEventManager dropping, 3 active service subscriptions" |
| `debug` | Grace-period cancellation | "Grace period for {ip}:{service} was cancelled by a re-acquire, keeping subscription" (`src/manager.rs:121`) |
| `warn` | Release without reference | "Attempted to release subscription but no references found" (`src/manager.rs:820`) |
| `warn` | Registry callback panicked | `call_unregister` (`src/manager.rs:190`) |
| `debug` | Timer outlived its manager | "outlived the manager, skipping teardown" (`src/timer.rs:345`) |

### 11.2 Metrics

The crate exposes subscription statistics via `service_subscription_stats()`:

```rust
// Returns HashMap<(IpAddr, Service), usize>
let stats = manager.service_subscription_stats();
for ((device_ip, service), ref_count) in stats {
    println!("{} {:?}: {} references", device_ip, service, ref_count);
}
```

### 11.3 Tracing

**Span structure**:
```
[ensure_service_subscribed]
  └── [EventBroker::register_speaker_service]  (if first reference)
```

Spans are implicit via `tracing::debug!` calls; no explicit span instrumentation.

---

## 12. Configuration

### 12.1 Configuration Options

The manager accepts `BrokerConfig` from sonos-stream for underlying EventBroker configuration:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `callback_port_range` | `(u16, u16)` | `(3400, 3500)` | Port range for the HTTP callback server |
| `base_polling_interval` | `Duration` | 5s | Polling interval when events are unavailable |
| `max_polling_interval` | `Duration` | 30s | Ceiling for adaptive polling backoff |
| `enable_proactive_firewall_detection` | `bool` | `true` | Whether to detect firewall blocking |

The full set is documented in [sonos-stream.md](sonos-stream.md) §12.

```rust
let config = BrokerConfig::default()
    .with_callback_ports(3400, 3500)
    .with_polling_interval(Duration::from_secs(5), Duration::from_secs(30));

let manager = SonosEventManager::with_config(config)?;   // src/manager.rs:425
```

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `SonosEventManager::new()` / `with_config()` | Unstable | Internal crate; may change |
| `acquire_watch()` | Unstable | The primary API. Returns a `WatchGuard`; RAII subscription management |
| `release_watch()` | `pub(crate)` | Called from `WatchGuard::Drop`; must never panic |
| `ensure_service_subscribed()` / `release_service_subscription()` | Unstable | The manual, guard-free pair. Deliberately asymmetric (§3.3) |
| `iter()` | Unstable | Repeatable; every `EventManagerIterator` shares one receiver |
| `WatchRegistry` | Unstable | Implementor contract in §2.3 is load-bearing |

### 13.2 Breaking Changes

**Policy**: As an internal crate, breaking changes are coordinated with `sonos-state` and do not follow semver guarantees for external consumers.

**Current deprecations**: None

### 13.3 Version

Published as `sonos-sdk-event-manager` (lib name `sonos_event_manager`), versioned from the
workspace (`version.workspace = true`), so it moves in lockstep with `sonos-sdk`.

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Concurrent `EventManagerIterator`s compete for events | Two consumers each see a subset, silently | Drain from one place, or fan out in `sonos-state` (see [sonos-state.md](sonos-state.md) §4.1b) | Fan out here too, if a second in-crate consumer ever appears |
| `release_service_subscription()` leaves watched pairs registered | A pair interleaved with a `WatchGuard` on the same key can outlive its subscription | None needed — no events arrive for it, and the next real teardown clears it | Documented at `src/manager.rs:790-793` |
| No device removal | Cannot remove devices once added | Recreate manager | Evaluate need based on usage |
| Sibling-key register race | Thread B acquiring `"mute"` while A acquires `"volume"` takes the `should_subscribe == false` path and never touches the pending mutex, so its `register_watch` is unordered against a concurrent expiry | Unreachable on the single-threaded TUI path | Needs the `service_refs` increment under the pending mutex — changes the manager's whole locking shape, so its own PR |
| Registry callbacks run under a manager lock | `drop`, `shutdown()` and `acquire_watch`'s claim can each block for one callback | The §2.3 contract bounds it | Shard `pending_unsubscribes`, or per-key token locks. Trigger: any callback measured above 100 us, or observed contention |
| `acquire_watch` leaks a ref count and a registration if `send(Subscribe)` fails | A watch that will never receive events | None | Pre-existing; not addressed by the teardown work |
| Post-`shutdown()` `Unsubscribe` commands accumulate | Unbounded channel grows until the manager drops | None needed in practice | Pre-existing; bounded by manager lifetime |
| `GRACE_PERIOD` is not configurable | 50 ms for everyone | None | `delay` already rides on `PendingTeardown`, so this is plumbing only |
| The timer thread is never joined | A teardown may still be running as the manager drops | `Drop` blocks on the pending-map mutex, so an in-flight callback completes first | Declined by design — joining would block `Drop` on implementor code |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| `acquire_watch` leaks its ref count and registration when `send(Subscribe)` fails | `src/manager.rs:526-544` | Medium | Roll back the increment and the `register_watch` on send failure |
| `service_refs` increment is outside the pending mutex, so a sibling-key acquire is unordered against a concurrent expiry | `src/manager.rs:478-503` | Medium | Requires reshaping the manager's locking; see §14.1 |
| Field declaration order in `SonosEventManager` is load-bearing for drop order | `src/manager.rs:371-411` | Low | Make the dependency explicit rather than relying on declaration order |
| Unwinding is assumed: `panic = "abort"` turns both containment layers into process aborts | `src/manager.rs:180-189` | Low | Document in the release profile, or detect at build time |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Roll back a failed `acquire_watch` | P1 | A watch that can never receive events should not hold a reference | — |
| Configurable `GRACE_PERIOD` | P2 | `delay` already rides on `PendingTeardown`, so this is plumbing only | — |
| Subscription health monitoring | P2 | Detect stale subscriptions | Metrics infrastructure |
| Device removal API | P2 | Support dynamic device changes | Usage analysis |

### 15.2 Open Questions

- [ ] **Should reference counting be at the property level instead of service level?** Currently, watching Volume and Mute both increment the RenderingControl count. This is correct but coarse-grained. Property-level counting would be more precise but add complexity.

- [ ] **Should we expose subscription state changes as events?** UI could show "Connected to Speaker A" status. Would require an additional event type.

- [ ] **Should `iter()` fan out per subscriber, as `sonos-state` does?** Today `sonos-state` is the only consumer and does the fan-out itself; a second consumer would need it here.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| Reference Counting | Tracking how many consumers need a resource, cleaning up when count reaches zero |
| UPnP Subscription | A registration with a Sonos device to receive real-time state change notifications |
| Service | A UPnP service category (AVTransport, RenderingControl, etc.) grouping related operations |
| EventBroker | The sonos-stream component that manages subscriptions and provides the event stream |
| Property | A specific piece of state (Volume, Mute, PlaybackState) that can be watched |

### B. References

- [RxJS refCount documentation](https://rxjs.dev/api/operators/refCount) - Inspiration for the reference counting pattern
- [UPnP Device Architecture](http://upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v1.1.pdf) - UPnP subscription model
- [sonos-stream specification](sonos-stream.md) — underlying event infrastructure
- [sonos-state specification](sonos-state.md) — the sole consumer, and where `WatchRegistry` is implemented
