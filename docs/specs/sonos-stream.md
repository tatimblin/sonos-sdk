# sonos-stream Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

Sonos devices support UPnP/SOAP event notifications for real-time state updates, but this requires the host machine to receive incoming HTTP connections. Many network environments (firewalls, NAT configurations, corporate networks) block these incoming connections, making UPnP events unreliable or impossible.

Without this crate, developers would face:
- **30+ second delays** waiting for event timeouts before discovering events are blocked
- **No fallback mechanism** when UPnP events fail
- **Inconsistent behavior** across different network configurations
- **Complex integration** between callback servers, subscription management, and polling systems
- **Code duplication** in sonos-state for handling event processing and network resilience

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Transparent event/polling switching | Users should receive events regardless of network conditions without code changes |
| P0 | Proactive firewall detection | Detect blocked firewalls immediately rather than waiting for timeout |
| P1 | Resource efficiency | Share HTTP connections and minimize unnecessary polling |
| P1 | Complete event enrichment | Every event includes full context (source, speaker, service, timestamp) |
| P2 | Adaptive polling intervals | Adjust polling frequency based on device activity |
| P2 | Comprehensive statistics | Expose operational metrics for debugging and monitoring |

### 1.3 Non-Goals

- **Direct end-user API**: This is an internal crate for sonos-state, not for direct consumption
- **State management**: Only provides raw events; sonos-state handles state aggregation
- **Device control**: This crate only receives events; sonos-api handles device commands
- **Device discovery**: Relies on sonos-discovery for finding devices

### 1.4 Success Criteria

- [x] Events arrive within 100ms when UPnP is available
- [x] Firewall blocking detected within 15 seconds (configurable)
- [x] Seamless fallback to polling with no event loss
- [x] Memory usage remains stable under continuous operation
- [x] All events include source attribution (UPnP vs polling)

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          EventBroker (Public API)                        │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌──────────────────┐  ┌─────────────────────────────┐ │
│  │   Registry   │  │ SubscriptionMgr  │  │     EventDetector           │ │
│  │ (Reg IDs)    │  │ (UPnP Subs)      │  │ (Timeout Monitoring)        │ │
│  └──────┬──────┘  └────────┬─────────┘  └──────────────┬──────────────┘ │
│         │                  │                           │                 │
├─────────┴──────────────────┴───────────────────────────┴─────────────────┤
│                         Event Processing Layer                           │
│  ┌───────────────────────────────┐  ┌───────────────────────────────┐   │
│  │      EventProcessor           │  │      PollingScheduler         │   │
│  │  (UPnP XML → EnrichedEvent)   │  │  (State Polling → Events)     │   │
│  └───────────────┬───────────────┘  └─────────────┬─────────────────┘   │
│                  │                                │                      │
│                  └────────────┬───────────────────┘                      │
│                               ▼                                          │
│                      ┌─────────────────┐                                 │
│                      │  EventIterator  │                                 │
│                      │ (Unified Stream)│                                 │
│                      └─────────────────┘                                 │
└─────────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        External Dependencies                             │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────────────────┐ │
│  │ callback-server│  │   sonos-api    │  │    sonos-discovery         │ │
│  │ (HTTP Events)  │  │ (SOAP/UPnP)    │  │    (Device Finding)        │ │
│  └────────────────┘  └────────────────┘  └────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
```

**Design Rationale**: The layered architecture separates concerns cleanly:
- **Registration layer** manages speaker/service identities
- **Subscription layer** handles UPnP protocol complexity
- **Processing layer** unifies events from multiple sources
- **Iterator layer** provides a simple consumption interface

This allows sonos-state to remain focused on reactive state management while sonos-stream handles all network resilience concerns.

### 2.2 Module Structure

```
src/
├── lib.rs                    # Public API exports and crate documentation
├── broker.rs                 # EventBroker - main orchestrator
├── config.rs                 # BrokerConfig - all configuration options
├── error.rs                  # Error types hierarchy
├── registry.rs               # Speaker/service registration with dedup
├── events/
│   ├── mod.rs                # Module exports
│   ├── types.rs              # EnrichedEvent and EventData definitions
│   ├── processor.rs          # UPnP XML parsing and event enrichment
│   └── iterator.rs           # Sync/async event consumption interfaces
├── subscription/
│   ├── mod.rs                # Module exports
│   ├── manager.rs            # UPnP subscription lifecycle management
│   └── event_detector.rs     # Event timeout detection
└── polling/
    ├── mod.rs                # Module exports
    ├── scheduler.rs          # Polling task management
    └── strategies.rs         # Service-specific polling implementations
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `broker` | Main orchestration and lifecycle management | `pub` |
| `config` | Configuration types and validation | `pub` |
| `error` | Error type definitions | `pub` |
| `registry` | Thread-safe speaker/service registration | `pub(crate)` primarily |
| `events` | Event types, processing, and iteration | `pub` |
| `subscription` | UPnP subscription management | `pub(crate)` |
| `polling` | Fallback polling system | `pub(crate)` |

### 2.3 Key Types

#### `EventBroker`

```rust
pub struct EventBroker {
    registry: Arc<SpeakerServiceRegistry>,
    subscription_manager: Arc<SubscriptionManager>,
    event_processor: Arc<EventProcessor>,
    callback_server: Arc<CallbackServer>,
    firewall_coordinator: Option<Arc<FirewallDetectionCoordinator>>,
    event_detector: Arc<EventDetector>,
    polling_scheduler: Arc<PollingScheduler>,
    event_sender: mpsc::UnboundedSender<EnrichedEvent>,
    event_receiver: Option<mpsc::UnboundedReceiver<EnrichedEvent>>,
    config: BrokerConfig,
    shutdown_signal: Arc<AtomicBool>,
    background_tasks: Vec<tokio::task::JoinHandle<()>>,
    // ...
}
```

**Purpose**: Central coordinator that manages all event streaming components and provides the public API.

**Invariants**:
- Only one `EventIterator` can be created per broker instance
- All background tasks are tracked for graceful shutdown
- Registry, subscription manager, and polling scheduler remain synchronized

**Ownership**: Created by sonos-state's `StateManager`, owned for the duration of the application.

#### `EnrichedEvent`

```rust
pub struct EnrichedEvent {
    pub registration_id: RegistrationId,
    pub speaker_ip: IpAddr,
    pub service: Service,
    pub event_source: EventSource,
    pub timestamp: SystemTime,
    pub observed_at: Instant,
    pub event_data: EventData,
}
```

**Purpose**: Unified event structure that combines raw event data with full context.

**Invariants**:
- `registration_id` always maps to a valid registration in the registry
- `timestamp` reflects when the event was constructed, not when it occurred on the device.
  Display and logging only — it is a `SystemTime` and can step backwards under NTP correction,
  so it must never be used to order two events
- `observed_at` is the monotonic instant the values were *observed*, and is what consumers order
  writes by. For a UPnP NOTIFY it is the arrival instant at the callback server, threaded down
  from `NotificationPayload::received_at`; a buffered event replayed after late SID registration
  keeps its original arrival instant. For a poll it is the instant the poll *request* was issued,
  captured by the polling loop before `poll_device_state` — a poll response describes the device
  as of the request, exactly like `fetch()`. See `sonos-state` spec §4.1a for the ordering rule
  these stamps feed
- `event_source` accurately identifies whether this came from UPnP or polling

**Construction**: `EnrichedEvent::new` stamps `observed_at` as "now" and is correct only when
construction and observation coincide — no production path in this crate qualifies, since both
sources observe before they construct. `EnrichedEvent::observed_at` takes the instant explicitly
and is what both the UPnP and polling paths use.

**Ownership**: Created by EventProcessor, passed through channels, consumed by sonos-state.

#### `RegistrationId`

```rust
pub struct RegistrationId(u64);
```

**Purpose**: Unique identifier for a speaker/service registration, enabling efficient lookups and deduplication.

**Invariants**:
- IDs are monotonically increasing and never reused within a broker lifetime
- Zero is never used as a valid ID (starts at 1)

---

## 3. Code Flow

### 3.1 Primary Flow: Event Registration and Processing

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   User Code     │────▶│  EventBroker    │────▶│    Registry     │
│ (sonos-state)   │     │                 │     │                 │
└─────────────────┘     └────────┬────────┘     └─────────────────┘
                                 │
         ┌───────────────────────┼───────────────────────┐
         │                       │                       │
         ▼                       ▼                       ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ Subscription    │     │   Firewall      │     │   Polling       │
│   Manager       │     │  Coordinator    │     │  Scheduler      │
│ broker.rs:420   │     │ broker.rs:406   │     │ broker.rs:449   │
└────────┬────────┘     └────────┬────────┘     └────────┬────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 ▼
                        ┌─────────────────┐
                        │ EventProcessor  │
                        │ processor.rs:51 │
                        └────────┬────────┘
                                 ▼
                        ┌─────────────────┐
                        │ EventIterator   │
                        │ iterator.rs:56  │
                        └─────────────────┘
```

**Step-by-step**:

1. **Registration** (`src/broker.rs`): User calls `register_speaker_service()` which:
   - Registers the speaker/service pair in the registry
   - **Returns immediately if the pair was already registered** (see below)
   - Checks if this is the first subscription for this device
   - On the first subscription, warns via `warn_if_speaker_unreachable` when the
     speaker is on no subnet we hold an address on (see below)
   - Triggers firewall detection if enabled
   - Creates a UPnP subscription via SubscriptionManager
   - Evaluates whether to start immediate polling based on firewall status

**Duplicate registration is idempotent, and `was_duplicate` is computed under the
registry's own lock.** `SpeakerServiceRegistry::register_reporting_duplicate` decides the
duplicate verdict inside the critical section that performs the insert, and
`register_speaker_service` returns straight away when it is `true`.

Two defects made this wrong. The verdict was computed as `registry.register(..)` followed
by `registry.is_registered(..)` — asked *after* the insert, so it always answered `true`:
`RegistrationResult::was_duplicate` was `true` for every registration, including
brand-new ones. And nothing short-circuited on it, so a repeat registration re-ran the
whole subscribe path: a second UPnP SUBSCRIBE producing a **new SID**, with
`subscriptions.insert(registration_id, wrapper)` overwriting the wrapper that held the
old one. The superseded SID was then unnameable by any code path yet remained in the
`EventRouter`'s active set for the process lifetime — the router kept accepting and
forwarding its events — and a later `unregister_speaker_service` released only the
newest. That is the duplicate-registration counterpart to the unregistration leak in
§5.2.

Deciding the verdict under the insert's own lock is what makes it race-free: a "check,
then register" pair in the caller would report two genuinely concurrent
first-registrations as both new. The duplicate return reports `polling_reason: None`,
because the call activated nothing; whether the reused registration is *currently*
polling is a separate question answered by `stats()`.

**Callback URL: one authoritative source.** `subscription_callback_url()` returns
`CallbackServer::base_url()` verbatim, and `SubscriptionManager` is constructed
from it. The broker previously ran its own `get_local_ip()` — a second
route-to-8.8.8.8 UDP probe — and rebuilt `http://{ip}:{port}` by hand, duplicating
a derivation `CallbackServer` had already performed. Two copies of one derivation
is what let them drift; whichever was wrong, speakers were handed an address they
could not reach, so **events were silently lost and the firewall detector
misattributed the silence to a firewall**, sending the device to polling. With a
VPN up, the probe returned the tunnel address and this happened for every speaker.
`get_local_ip()` is deleted; the broker never derives an address itself.

`warn_if_speaker_unreachable` calls `CallbackServer::local_ip_for_speaker`, which
does real subnet containment using each interface's actual netmask. A /24
assumption would be wrong here: this project's network is one flat
`192.168.4.0/22`, so a `192.168.5.x` speaker *is* reachable from `192.168.4.32`,
and a /24 check would emit a false warning for half the household. See
`docs/specs/callback-server.md` §4.5.

2. **Firewall Detection** (`src/broker.rs:625-640`): Per-device firewall detection:
   - First subscription triggers proactive detection
   - Subsequent subscriptions use cached status
   - Detection runs concurrently with subscription creation

3. **Subscription Creation** (`src/subscription/manager.rs:181-211`):
   - Creates UPnP subscription using SonosClient
   - Registers subscription ID with EventRouter for event routing
   - Wraps in ManagedSubscriptionWrapper with additional context

4. **Event Arrival** (`src/events/processor.rs`):
   - Callback server receives UPnP NOTIFY message
   - EventProcessor looks up subscription by SID
   - **Reports liveness to the EventDetector via `record_event()`** — this is what
     keeps timeout detection honest (see 3.2)
   - Parses XML using sonos-api event framework
   - Enriches with registration context
   - Sends through unified event channel

5. **Event Consumption** (`src/events/iterator.rs:56-91`):
   - EventIterator receives from unified channel
   - Provides sync or async iteration interfaces
   - Supports filtering by registration, service, or source

### 3.2 Secondary Flow: Polling Fallback Activation

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Firewall      │────▶│  EventDetector  │────▶│   Polling       │
│   Blocked       │     │                 │     │   Scheduler     │
└─────────────────┘     └─────────────────┘     └─────────────────┘
         │                                              │
         │ broker.rs:669-684                           │
         │                                              ▼
         │                                     ┌─────────────────┐
         │                                     │  PollingTask    │
         │                                     │scheduler.rs:105 │
         │                                     └────────┬────────┘
         │                                              │
         └──────────────────────────────────────────────┤
                                                        ▼
                                               ┌─────────────────┐
                                               │ DevicePoller    │
                                               │strategies.rs:309│
                                               └────────┬────────┘
                                                        │
                                                        ▼
                                               ┌─────────────────┐
                                               │ EventProcessor  │
                                               │(as synthetic)   │
                                               └─────────────────┘
```

**Step-by-step**:

1. **Detection Trigger**: Firewall status triggers immediate polling decision
2. **Polling Start** (`src/polling/scheduler.rs`): PollingScheduler creates new PollingTask
3. **State Polling** (`src/polling/scheduler.rs`): PollingTask runs loop with configurable interval
4. **Change Detection** (`src/polling/strategies.rs`): Service-specific pollers detect state changes
5. **Event Generation** (`src/polling/scheduler.rs`): State changes converted to EnrichedEvents with PollingDetection source

#### Polling Fallback Lifecycle (start AND stop)

Polling fallback is a **reversible** state. The full lifecycle is driven by
`EventDetector`, which owns one `polling_reason: Option<PollingReason>` per
registration — `Some` means polling is active and a `Stop` is owed.

```
                     ┌──────────────────────────┐
                     │  UPnP events (healthy)   │  polling_reason = None
                     └────────────┬─────────────┘
       no event for               │              UPnP event arrives
       `event_timeout`, OR        │              (EventProcessor calls
       firewall detected blocked  │               record_event)
                     ▼            │                        ▲
        PollingRequest{Start}     │           PollingRequest{Stop}
                     │            │                        │
                     ▼            ▼                        │
                     ┌──────────────────────────┐          │
                     │  Polling fallback active │──────────┘
                     └──────────────────────────┘  polling_reason = Some(reason)
```

**Entering polling** — `polling_reason` is set to `Some(..)` in exactly two places, and
setting it is what suppresses duplicate `Start` requests:
- `start_monitoring()`'s timeout sweep, on `event_timeout` elapsing → `EventTimeout`
- `mark_polling_active()`, called by the broker after an eager firewall-driven start
  → `FirewallBlocked` / `NetworkIssues`

It is deliberately *not* set for `force_polling_mode` or subscription-creation failure:
those paths never register with the detector at all, because no UPnP event can ever
arrive to stop them. Such registrations poll unconditionally until unregistered.

**Leaving polling** — `EventProcessor::process_upnp_notification` calls
`EventDetector::record_event()` for **every** UPnP notification. That call:
1. advances `last_event_time`, which is the only thing preventing the timeout sweep
   from firing on a healthy registration; and
2. if `polling_reason` is `Some`, takes it and emits `PollingRequest{Stop}`,
   carrying the original reason for diagnostics.

The broker's polling-request task handles `Stop` by calling
`PollingScheduler::stop_polling()` (which removes the task and awaits its shutdown)
and clearing the subscription's `polling_active` flag.

**Stopping is prompt, and does not block unrelated callers.** Two properties, both
previously violated:

1. *Shutdown is signalled, not just flagged.* `PollingTask`'s shutdown signal is an
   `AtomicBool` **plus a `tokio::sync::Notify`**, and every sleep in the polling loop is
   a `select!` over the sleep and that notify. Previously the flag was read only at the
   *top* of the loop, so a stop had to wait out the whole in-flight iteration:
   `current_interval` (≤5s by default), a full poll (several sequential SOAP calls, each
   with a 5s connect / 10s read timeout), and — worst — an error-backoff sleep capped at
   `max_polling_interval` (30s) that was **not guarded at all**. Against an unreachable
   speaker that totalled roughly 85s. Now only the in-flight poll delays shutdown.
   `notify_one` rather than `notify_waiters` is deliberate: the latter is dropped when no
   task is parked at that instant, so a stop issued mid-poll would let the *next* sleep
   run to completion.
2. *`stop_polling` does not hold `active_tasks` across the await.* `remove()` has already
   taken the task out of the map, so the write guard protects nothing during
   `shutdown().await` — it only blocks every other accessor: `start_polling`,
   `is_polling`, and `stats()`. Because `EventBroker::stats()` calls the last of those, a
   caller merely asking for statistics could hang for the full shutdown window; and since
   polling requests are handled by a single serialized task, every queued `Start`/`Stop`
   behind it stalled too. The guard is now released before awaiting, which is safe
   precisely because the map no longer references the task: no concurrent caller can
   observe or re-enter it, and a racing `start_polling` for the same registration
   correctly sees "not polling" and spawns a fresh task rather than blocking on the
   outgoing one. `shutdown_all()` drains under the lock and releases it before awaiting,
   for the same reason.

**Why `record_event` is load-bearing.** Both properties above depend on it being
called on the hot path. When it was not (it had no production caller at all), every
registration's `last_event_time` was frozen at registration time, so *every*
registration was declared timed out after `event_timeout` and began polling on top
of perfectly working UPnP events — and because `Stop` was never constructed, it
never stopped.

Any future refactor that moves event handling must preserve this call.
`events::processor::tests::test_processor_records_event_with_detector` guards it by
driving `process_notification_for_registration` — the real path — with a real UPnP
event body, and has been verified to fail when the `record_event` call is deleted. Note
that a test calling a thin one-line delegator instead would *not* catch that deletion;
the guard is only meaningful because it exercises the production call site.

**Failure handling.** If a requested `Start` fails, the broker calls
`clear_polling_active()`. Without that the registration would keep
`polling_reason = Some(..)` forever, permanently suppressing timeout detection and
denying it any future fallback.

**Staleness rule for queued `Start`s.** Polling requests are handled in order by a
single task, and a `Stop` can still block that task: shutdown now interrupts the loop's
sleeps promptly (see "Stopping is prompt" above), but it must still await the *in-flight
poll*, which is several sequential SOAP calls each with a 5s connect / 10s read timeout.
Against an unreachable speaker that remains seconds, not milliseconds — so the queue can
still fall behind and the staleness rule below is still required.

A `Start` queued behind such a `Stop` can therefore drain *after* its registration was
unregistered. `PollingScheduler::start_polling` validates only "already polling" and
the concurrency cap, so it would happily spawn a task for a registration that exists
nowhere — and nothing could stop it: no subscription means no UPnP event can arrive,
the detector has no entry, and `unregister_speaker_service` returns `NotFound` before
reaching the scheduler. Only `shutdown_all()` would reap it. The handler therefore
re-checks `registry.get_pair(..)` immediately before starting and drops stale requests.

### 3.3 Error Flow

```
[SOAP Error] ──▶ [PollingError::Network] ──▶ [BrokerError::Polling] ──▶ [Consumer]
[XML Parse]  ──▶ [EventProcessingError]  ──▶ [Stats Updated]         ──▶ [Logged]
[Timeout]    ──▶ [SubscriptionError]     ──▶ [Polling Activated]     ──▶ [Continues]
```

**Error handling philosophy**: Errors are categorized by recoverability. Network errors trigger fallback mechanisms rather than propagating failures. The system prioritizes continued operation over perfect accuracy.

---

## 4. Features

### 4.1 Feature: Proactive Firewall Detection

#### What

Immediately determines whether the host's firewall blocks incoming HTTP connections by integrating with callback-server's FirewallDetectionCoordinator.

#### Why

Traditional UPnP event systems wait 30+ seconds for event timeouts before discovering network issues. This wastes user time and creates poor UX. Proactive detection identifies blocked firewalls within 15 seconds of first subscription.

#### How

The system uses a per-device detection model (`src/broker.rs:164-181`):

```rust
// On first subscription for a device
let firewall_coordinator = Arc::new(FirewallDetectionCoordinator::new(config));
let status = coordinator.on_first_subscription(device_ip).await;
```

Detection works by:
1. Creating a UPnP subscription
2. Waiting for the first event with configurable timeout (default 15s)
3. Caching the result per-device for subsequent subscriptions

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Per-device caching | Global firewall status | Different devices may have different reachability |
| 15-second timeout | 5-second timeout | Balance between responsiveness and network latency |
| Immediate polling on blocked | Wait for user confirmation | Better UX with automatic fallback |

### 4.2 Feature: Transparent Event/Polling Switching

#### What

Consumers receive events from a single unified stream regardless of whether they came from UPnP notifications or polling. The `EventSource` enum indicates the origin.

#### Why

Applications should not need to implement separate code paths for UPnP and polling. This complexity belongs in the infrastructure layer, not the application layer.

#### How

Events from both sources flow through the same channel (`src/broker.rs:139-140`):

```rust
let (event_sender, event_receiver) = mpsc::unbounded_channel();
// Both UPnP processor and polling scheduler send to event_sender
```

Event source is preserved for debugging and optimization:
```rust
pub enum EventSource {
    UPnPNotification { subscription_id: String },
    PollingDetection { poll_interval: Duration },
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Single unified channel | Separate channels per source | Simpler consumer code |
| Unbounded channel | Bounded with backpressure | Events should never be dropped |
| Source attribution | Source hiding | Debugging requires visibility |

### 4.3 Feature: Adaptive Polling Intervals

#### What

Polling intervals automatically adjust based on device activity. Frequent changes trigger faster polling; idle devices poll less frequently.

#### Why

Fixed polling intervals waste resources on idle devices while potentially missing rapid changes on active devices.

#### How

Adaptive intervals calculated in `src/polling/scheduler.rs:322-340`:

```rust
fn calculate_adaptive_interval(
    current_interval: Duration,
    max_interval: Duration,
    last_change_time: SystemTime,
) -> Duration {
    let time_since_change = SystemTime::now()
        .duration_since(last_change_time)
        .unwrap_or(Duration::ZERO);

    if time_since_change < Duration::from_secs(30) {
        // Recent activity - poll faster
        (current_interval / 2).max(Duration::from_secs(2))
    } else if time_since_change > Duration::from_secs(300) {
        // No recent activity - poll slower
        (current_interval * 2).min(max_interval)
    } else {
        current_interval
    }
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Time-based adaptation | Change-rate-based | Simpler implementation, predictable behavior |
| 2-second minimum | 1-second minimum | Avoid overwhelming devices |
| Configurable max interval | Fixed 60-second max | Different use cases need different limits |

### 4.4 Feature: Service-Specific Polling Strategies

#### What

Each UPnP service type has a dedicated polling strategy that knows how to query state and detect changes.

#### Why

Different services have different state structures, APIs, and change patterns. A generic approach would miss important changes or generate false positives.

#### How

Service pollers implement the `ServicePoller` trait (`src/polling/strategies.rs:49-60`):

```rust
#[async_trait]
pub trait ServicePoller: Send + Sync {
    async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String>;
    async fn parse_for_changes(&self, old_state: &str, new_state: &str) -> Vec<StateChange>;
    fn service_type(&self) -> Service;
}
```

Implemented for: AVTransport, RenderingControl, ZoneGroupTopology (stub), GroupManagement (stub)

**Observation stamping.** The polling loop captures an `Instant` immediately before
`poll_device_state` and passes it to `EnrichedEvent::observed_at`, so the synthetic event is
ordered by when the device was *read*, not when the response came back. Stamping on return made
a poll look one round trip newer than the state it described — seconds against a slow speaker,
enough to wrongly supersede a fresher UPnP event or local write. The capture stays below the
interval sleep: hoisting it above would backdate a legitimately newer poll into being dropped as
stale, which loses real data rather than merely mis-ordering it.

---

## 5. Data Model

### 5.1 Core Data Structures

#### `BrokerConfig`

```rust
pub struct BrokerConfig {
    /// Port range for callback server (default: 3400-3500)
    pub callback_port_range: (u16, u16),
    /// Timeout before considering UPnP events failed (default: 30s)
    pub event_timeout: Duration,
    /// Currently has no effect — retained for API compatibility (see 14.2)
    pub polling_activation_delay: Duration,
    /// Base polling interval (default: 5s)
    pub base_polling_interval: Duration,
    /// Maximum adaptive polling interval (default: 30s)
    pub max_polling_interval: Duration,
    /// UPnP subscription timeout (default: 1800s/30min)
    pub subscription_timeout: Duration,
    /// Enable proactive firewall detection (default: true)
    pub enable_proactive_firewall_detection: bool,
    /// Timeout for firewall detection (default: 15s)
    pub firewall_event_wait_timeout: Duration,
    /// Maximum registrations (default: 1000)
    pub max_registrations: usize,
    /// Force polling mode — skip UPnP subscriptions entirely (default: false)
    /// Useful for testing firewall fallback behavior without a real firewall
    pub force_polling_mode: bool,
    // ... additional fields
}
```

**Lifecycle**:
1. **Creation**: Built via `Default`, `new()`, or preset methods (`fast_polling()`, `resource_efficient()`)
2. **Validation**: `validate()` called during broker creation
3. **Usage**: Immutable after broker creation

#### `EventData`

```rust
pub enum EventData {
    AVTransportEvent(AVTransportEvent),
    RenderingControlEvent(RenderingControlEvent),
    DevicePropertiesEvent(DevicePropertiesEvent),
    ZoneGroupTopologyEvent(ZoneGroupTopologyEvent),
    GroupManagementEvent(GroupManagementEvent),
}
```

**Lifecycle**:
1. **Creation**: Parsed from UPnP XML or constructed from polling state
2. **Mutation**: Never mutated after creation
3. **Destruction**: Dropped after consumer processes the event

### 5.2 State Transitions

```
┌─────────────┐   register()    ┌─────────────┐
│ Unregistered│────────────────▶│  Registered │
└─────────────┘                 └──────┬──────┘
                                       │
      ┌─────────────────────────┬──────┴──────┬─────────────────────┐
      │                         │             │                     │
      ▼                         ▼             ▼                     ▼
┌───────────┐            ┌───────────┐  ┌───────────┐        ┌───────────┐
│ UPnP Only │            │  Polling  │  │   Both    │        │  Failed   │
│(Accessible)│            │   Only    │  │(Switching)│        │           │
└─────┬─────┘            └─────┬─────┘  └─────┬─────┘        └─────┬─────┘
      │                         │             │                     │
      │ timeout                 │ event       │                     │
      │                         │ received    │                     │
      └─────────────────────────┴──────┬──────┴─────────────────────┘
                                       │
                                       ▼
                              ┌─────────────────┐
                              │   unregister()  │
                              └─────────────────┘
```

**Invariants per state**:
- **Unregistered**: No registry entry, no subscription, no polling, SID not in EventRouter
- **Registered/UPnP Only**: Active UPnP subscription, no polling task, EventDetector
  `polling_reason == None`. **Exactly one SID per registration is in the EventRouter.**
  A repeat `register_speaker_service` for an already-registered pair returns the existing
  registration without subscribing again, so it cannot add a second SID and orphan the
  first (see 3.1).
- **Registered/Polling Only**: Active polling task. Two sub-cases, which differ in
  whether the fallback is reversible:
  - *Reversible* (UPnP subscription exists, events merely stopped or the firewall was
    detected as blocking): the registration has an EventDetector entry with
    `polling_reason == Some(reason)`, and a resumed UPnP event stops polling.
  - *Irreversible* (`force_polling_mode`, or subscription creation failed): there is
    **no** EventDetector entry at all, so no `polling_reason` exists and polling
    continues until the registration is unregistered. No UPnP event can arrive to end
    it, which is exactly why these paths skip detector registration.
- **Registered/Both**: Transitioning states, temporary condition
- **Failed**: Registration exists but no event source active

The UPnP-Only ↔ Polling transition is bidirectional for the reversible sub-case only,
and is driven entirely by `EventDetector::record_event`; see "Polling Fallback
Lifecycle" in 3.2.

**Unregistration ordering** (`EventBroker::unregister_speaker_service`): the UPnP
subscription ID must be read *before* `remove_subscription()` drops the
subscription, because it is needed to call `EventRouter::unregister()`. Skipping
that call leaks the SID in the router's active set for the process lifetime.

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `callback-server` | HTTP server for UPnP callbacks | Handles complex HTTP/firewall detection |
| `sonos-api` | UPnP operations and event parsing | Type-safe Sonos API, shared with consumers |
| `sonos-discovery` | Device discovery utilities | Not directly used, but referenced in examples |
| `soap-client` | Low-level SOAP transport | Indirect via sonos-api |
| `tokio` | Async runtime | Background tasks, channels, timers |
| `dashmap` | Concurrent HashMap | Lock-free concurrent access patterns |
| `crossbeam` | Lock-free data structures | High-performance event processing |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| `sonos-state` | Creates EventBroker, processes events | Primary consumer; API changes coordinated |
| `sonos-event-manager` | Reference-counted subscription orchestration | Uses EnrichedEvent and RegistrationId |

### 6.3 External Systems

```
┌─────────────────┐         ┌─────────────────┐
│  sonos-stream   │◀───────▶│  Sonos Device   │
│                 │  HTTP   │   (UPnP/SOAP)   │
└─────────────────┘  :1400  └─────────────────┘
        │
        │ Listens on
        │ :3400-3500
        ▼
┌─────────────────┐
│ callback-server │
│ (HTTP Server)   │
└─────────────────┘
```

**Protocol**: UPnP/SOAP over HTTP (port 1400 for Sonos, configurable for callbacks)

**Authentication**: None (UPnP is designed for local networks)

**Error handling**: Network errors trigger polling fallback; device errors logged but not fatal

**Retry strategy**: UPnP subscriptions auto-renew; polling uses exponential backoff on errors

---

## 7. Error Handling

### 7.1 Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("Registry error: {0}")]
    Registry(#[from] RegistryError),

    #[error("Subscription error: {0}")]
    Subscription(#[from] SubscriptionError),

    #[error("Polling error: {0}")]
    Polling(#[from] PollingError),

    #[error("Event processing error: {0}")]
    EventProcessing(String),

    #[error("Callback server error: {0}")]
    CallbackServer(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Firewall detection error: {0}")]
    FirewallDetection(String),
}

#[derive(Debug, thiserror::Error)]
pub enum PollingError {
    #[error("Network error during polling: {0}")]
    Network(String),

    #[error("Service not supported for polling: {service:?}")]
    UnsupportedService { service: Service },

    #[error("Too many consecutive errors: {error_count}")]
    TooManyErrors { error_count: u32 },
}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Graceful degradation | Polling fallback on event failures | Continuous operation preferred over failure |
| Transparent to consumers | Errors handled internally where possible | Consumer code stays simple |
| Detailed logging | All errors logged with context | Post-mortem debugging possible |
| Typed error hierarchy | Nested error enums with `#[from]` | Clear error origin tracing |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `SubscriptionError::CreationFailed` | Yes | Automatic polling fallback |
| `PollingError::Network` | Yes | Exponential backoff, max 5 retries |
| `PollingError::TooManyErrors` | No | Task stops, registration remains |
| `BrokerError::Configuration` | No | Fail fast during initialization |
| `EventProcessingError::Parsing` | Yes | Log and continue |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    ┌───────────────────┐
                    │  Integration/E2E  │  Manual with real devices
                    └─────────┬─────────┘
              ┌───────────────┴───────────────┐
              │       Component Tests         │  Examples serve as integration tests
              └───────────────┬───────────────┘
    ┌─────────────────────────┴─────────────────────────┐
    │                   Unit Tests                       │  Inline #[cfg(test)] modules
    └────────────────────────────────────────────────────┘
```

### 8.2 Unit Tests

**Location**: Inline `#[cfg(test)]` modules in each source file

**What to test**:
- [x] Configuration validation (`src/config.rs`)
- [x] Registration duplicate detection (`src/registry.rs`)
- [x] Event type creation and service mapping (`src/events/types.rs`)
- [x] Iterator statistics tracking (`src/events/iterator.rs`)
- [x] Adaptive interval calculation (`src/polling/scheduler.rs`)
- [x] Change detection for AVTransport/RenderingControl (`src/polling/strategies.rs`)
- [x] Polling fallback lifecycle, all offline (see 3.2). One test per production call
      site, each verified to fail when that call site is deleted:
  - `events::processor::tests::test_processor_records_event_with_detector` — the
    processor reports event liveness, so a healthy registration is never declared
    timed out
  - `subscription::event_detector::tests::test_stop_emitted_when_events_resume_during_polling`
    — `PollingAction::Stop` is emitted exactly once when events resume
  - `broker::tests::test_unregister_releases_router_sid` — unregistration removes
    the SID from the EventRouter
  - `broker::tests::test_eager_polling_is_recorded_with_detector` — firewall-driven
    polling is recorded, so a later event can stop it
  - `broker::tests::test_stale_start_does_not_spawn_polling_task` — a `Start` whose
    registration has been unregistered is dropped rather than spawning an unstoppable
    poller
  - `broker::tests::test_failed_start_clears_polling_marker` — a failed `Start` clears
    the marker so timeout detection is not suppressed forever
- [x] Duplicate registration and shutdown promptness, all offline (RFC 5737 TEST-NET-3
      addresses; each verified to fail with its fix reverted):
  - `registry::tests::test_register_reports_duplicate_only_for_repeats` — the duplicate
    verdict distinguishes a first registration from a repeat
  - `broker::tests::test_was_duplicate_distinguishes_first_registration_from_repeat` —
    drives the real `register_speaker_service`, so it catches the "ask `is_registered`
    *after* `register`" ordering bug that made `was_duplicate` always `true`. Note the
    pre-existing `test_registration_result` does **not** catch it: it constructs a
    `RegistrationResult` literal and asserts the field reads back, never invoking the
    computation.
  - `broker::tests::test_duplicate_registration_does_not_resubscribe` — a duplicate
    short-circuits before the subscribe path, so no second SID is created to orphan the
    first. The discriminator is `polling_reason`: offline, re-entering the subscribe path
    reports `Some(SubscriptionFailed)` while the short-circuit reports `None`. Because
    SUBSCRIBE cannot succeed without a real speaker, this asserts *that the second
    subscribe never happens* rather than the growth of the router's SID set — with no
    live SID there is no non-vacuous set assertion available offline.
  - `polling::scheduler::tests::test_stop_polling_does_not_hold_lock_across_shutdown` —
    a concurrent `stats()`/`is_polling()` completes while a slow shutdown is in flight;
    asserts `!stopper.is_finished()` first, so it cannot pass vacuously by the shutdown
    having already completed
  - `polling::scheduler::tests::test_shutdown_interrupts_pending_sleep` — a stop cuts
    short a 60s interval sleep instead of waiting it out
  - `polling::scheduler::tests::test_shutdown_interrupts_error_backoff` — a stop also
    cuts short the *error-backoff* sleep, the offender that was previously unguarded
    entirely. Distinct from the previous test: reverting only the backoff guard leaves
    that one passing. Timing-sensitive by nature (it distinguishes "returns at once" from
    "waits ~9s" with a 6s bound), so the timings are chosen with margin on both sides.

**Mutation-testing note.** These guards are only meaningful because they drive the real
call sites. A test that exercises a thin extracted helper directly can pass with the
production call to that helper deleted — which is the same "code that is never called"
failure this crate already shipped once. When adding coverage here, delete the call site
and confirm the test actually fails.

**Example**:
```rust
#[tokio::test]
async fn test_duplicate_detection() {
    let registry = SpeakerServiceRegistry::new(100);
    let ip: IpAddr = "192.168.1.100".parse().unwrap();
    let service = sonos_api::Service::AVTransport;

    let reg_id1 = registry.register(ip, service).await.unwrap();
    let reg_id2 = registry.register(ip, service).await.unwrap();

    assert_eq!(reg_id1, reg_id2);
    assert_eq!(registry.count().await, 1);
}
```

### 8.3 Integration Tests

**Location**: `examples/` directory

**Prerequisites**:
- [x] Sonos device on local network
- [x] Network allows HTTP callbacks (for UPnP tests) or firewall blocking (for polling tests)

**What to test**:
- [x] Basic event streaming (`examples/basic_usage.rs`)
- [x] Firewall handling scenarios (`examples/firewall_handling.rs`)
- [x] Filtering and batch processing (`examples/filtering_and_batch.rs`)
- [x] Async real-time processing (`examples/async_realtime.rs`)

### 8.4 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| `SonosClient` | Real client in tests | No mocking needed for unit tests |
| `CallbackServer` | Skipped in unit tests | Broker creation may fail gracefully |
| Network | Test with real devices | Examples require real Sonos |
| `ManagedSubscription` | Not constructible offline | Only `ManagedSubscription::create()` builds one, and it performs a real UPnP SUBSCRIBE. Logic that would otherwise need one is extracted into functions taking plain arguments (`EventProcessor::record_event_liveness`, `broker::release_router_sid`) so it can be unit-tested with no network. |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Event latency (UPnP) | <100ms from device to consumer | Real-time responsiveness |
| Event latency (polling) | base_interval + processing time | Configurable trade-off |
| Memory per registration | <10KB | Support 1000+ registrations |
| Concurrent polling tasks | 50 (configurable) | Balance device load and responsiveness |

### 9.2 Critical Paths

1. **UPnP Event Processing** (`src/events/processor.rs:51-126`)
   - **Complexity**: O(1) for subscription lookup, O(n) for XML parsing
   - **Bottleneck**: XML parsing of large metadata
   - **Optimization**: Uses sonos-api's optimized event framework

2. **Registry Lookup** (`src/registry.rs:221-229`)
   - **Complexity**: O(1) HashMap lookup
   - **Bottleneck**: Write lock contention under high registration churn
   - **Optimization**: Uses bidirectional HashMap for O(1) both directions

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| HTTP connections | Via SonosClient singleton | On request completion | Yes - shared soap-client |
| UPnP subscriptions | On registration | On unregistration/shutdown | No - per-registration |
| EventRouter SIDs | On subscription creation | On unregistration (`EventRouter::unregister`) | No - per-subscription |
| Polling tasks | On fallback trigger | On `PollingAction::Stop` (events resumed), unregistration, error, or shutdown | No - per-registration |
| Event channels | On broker creation | On broker shutdown | No - single channel |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious UPnP events | Low | Medium | Validate subscription IDs; ignore unknown |
| Network sniffing | Medium | Low | UPnP is unencrypted by design; local network only |
| Denial of service | Low | Medium | Rate limiting via configurable poll limits |
| Resource exhaustion | Low | Medium | Max registrations limit; polling task limits |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Device IPs | Low | Logged for debugging only |
| Track metadata | Low | Passed through, not stored |
| Subscription IDs | Low | Internal identifiers only |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| UPnP XML | Subscription ID matching | `src/events/processor.rs:62-71` |
| Configuration | Range and type validation | `src/config.rs:156-199` |
| Registration requests | IP validation, service enum | `src/broker.rs:385-489` |

---

## 11. Observability

### 11.1 Logging

| Level | What's Logged | Example |
|-------|--------------|---------|
| `error` | Unrecoverable failures | "Failed to create subscription: {}" |
| `warn` | Recoverable issues | "Subscription renewal failed, will retry" |
| `info` | Significant lifecycle events | "Starting EventBroker", "Firewall detected" |
| `debug` | Detailed state transitions | "Event processed: {} {:?}" |
| `trace` | Full event payloads | XML content (not enabled by default) |

Note: Current implementation uses `eprintln!` extensively for visibility during development. Production should migrate to `tracing` macros.

### 11.2 Statistics

All major components expose `stats()` methods:

- `BrokerStats`: Overall broker state
- `RegistryStats`: Registration counts by service
- `SubscriptionStats`: Active subscriptions, firewall status, renewals
- `PollingSchedulerStats`: Active tasks, intervals, error counts
- `EventProcessorStats`: Events processed by source
- `EventIteratorStats`: Events received/delivered, timeouts

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `callback_port_range` | `(u16, u16)` | `(3400, 3500)` | Port range for callback server |
| `event_timeout` | `Duration` | `30s` | Time before considering events failed |
| `base_polling_interval` | `Duration` | `5s` | Initial polling interval |
| `max_polling_interval` | `Duration` | `30s` | Maximum adaptive interval |
| `enable_proactive_firewall_detection` | `bool` | `true` | Enable immediate firewall detection |
| `firewall_event_wait_timeout` | `Duration` | `15s` | Time to wait for first event |
| `max_registrations` | `usize` | `1000` | Maximum speaker/service pairs |
| `max_concurrent_polls` | `usize` | `50` | Maximum simultaneous polling tasks |
| `adaptive_polling` | `bool` | `true` | Enable interval adaptation |

### 12.2 Configuration Presets

```rust
// Default: Balanced settings
BrokerConfig::default()

// Fast polling: For unreliable networks
BrokerConfig::fast_polling()

// Resource efficient: For large deployments
BrokerConfig::resource_efficient()

// No firewall detection: For controlled environments
BrokerConfig::no_firewall_detection()

// Firewall simulation: Force polling mode for testing fallback behavior
// Skips UPnP subscriptions entirely and polls at 2s base / 10s max intervals
BrokerConfig::firewall_simulation()
```

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `EventBroker::new()` | Stable | Async constructor, takes BrokerConfig |
| `EventBroker::register_speaker_service()` | Stable | Returns detailed RegistrationResult |
| `EventBroker::event_iterator()` | Stable | Can only be called once |
| `EnrichedEvent` | Stable | All fields public |
| `EventData` | Evolving | New variants may be added |

### 13.2 Breaking Changes

**Policy**: Internal crate follows workspace versioning. Changes coordinated with sonos-state.

**Current deprecations**: None

### 13.3 Version History

| Version | Changes | Migration Guide |
|---------|---------|-----------------|
| `0.1.0` | Initial release | N/A |

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| ZoneGroupTopology polling is stubbed | Topology changes only via UPnP | Ensure firewall allows callbacks | Add GetZoneGroupState polling |
| Single EventIterator per broker | Can't fan-out events | Create wrapper channel | Consider multi-consumer support |
| Blocking SOAP client in polling | Thread pool usage | Uses tokio::task::spawn_blocking | Migrate to async SOAP client |
| DeviceProperties service not fully supported | Limited device property events | Use ZoneGroupTopology fallback | Add DeviceProperties service |
| **One callback URL for all subscriptions** | A household spanning genuinely different subnets gets a URL only one half can reach; the rest logs a warning and falls back to polling | Polling still delivers state, just less promptly | **Named follow-up**: per-subscription callback URL. `SubscriptionManager::callback_url` is a single `String`, so this needs a signature change from `create_subscription` down. `CallbackServer::local_ip_for_speaker` is the piece to consume. Moot on the current dev network (one flat /22), but multi-subnet households are **not** supported today. |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| `BrokerConfig::polling_activation_delay` is unused | `config.rs` | Low | Its only reader was `EventDetector::should_stop_polling`, a dead time-based heuristic replaced by the explicit `polling_reason` state machine (3.2). The field is still public and settable but has no effect; remove it in a follow-up that owns `config.rs`. |
| eprintln! instead of tracing | Throughout | Low | Replace with tracing macros |
| Incomplete position info polling | `strategies.rs:84-90` | Medium | Add get_position_info_operation call |
| Hardcoded error thresholds | `scheduler.rs:287` | Low | Move to BrokerConfig |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Async SOAP client | P1 | Eliminate blocking calls | sonos-api changes |
| Multi-consumer iterator | P2 | Fan-out to multiple processors | Architecture change |
| Metrics export (Prometheus) | P2 | Production monitoring | New dependency |
| Connection health monitoring | P2 | Proactive reconnection | callback-server changes |

### 15.2 Open Questions

- [ ] **Should EventIterator support cloning?** Currently single-consumer only. Fan-out would require internal broadcast channel.
- [ ] **How to handle device disappearance?** Currently registration persists. Should we auto-unregister after extended failures?

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| **Registration** | A speaker IP + service type combination tracked by the broker |
| **Enriched Event** | Raw event data combined with context (source, timing, registration ID) |
| **Proactive Detection** | Determining firewall status before events timeout |
| **Adaptive Polling** | Dynamically adjusting poll intervals based on activity |

### B. References

- [UPnP Device Architecture 2.0](http://upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v2.0.pdf)
- [Sonos API Documentation (unofficial)](https://github.com/SoCo/SoCo/wiki)
- [callback-server crate](../callback-server)
- [sonos-api crate](../sonos-api)

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2025-01-14 | Claude Code | Initial specification |
| 2026-08-15 | Claude Code | Deleted `broker::get_local_ip` (a duplicate route-to-8.8.8.8 probe) and made the broker consume `CallbackServer::base_url()` as the single authoritative callback URL (3.1). Added a first-subscription reachability warning based on real netmasks. Recorded the single-callback-URL multi-subnet limitation as a named follow-up (14.1). |
| 2026-08-15 | Claude Code | Documented the polling-fallback lifecycle as reversible (3.2, 5.2): `record_event` liveness reporting, `PollingAction::Stop` on event resumption, and EventRouter SID release on unregistration. Noted `polling_activation_delay` is now unread. |
| 2026-08-16 | Claude Code | Closed the duplicate-registration SID leak (3.1, 5.2): `register_speaker_service` now short-circuits an already-registered pair instead of re-subscribing and orphaning the previous SID, and `was_duplicate` is computed by `SpeakerServiceRegistry::register_reporting_duplicate` under the insert's own lock — it was previously an `is_registered` call made *after* `register`, so it always answered `true`. Made polling shutdown prompt and non-blocking (3.2): the shutdown signal now carries a `Notify` that interrupts both the interval sleep and the previously unguarded error-backoff sleep, and `stop_polling`/`shutdown_all` release the `active_tasks` guard before awaiting shutdown so `stats()`, `is_polling` and `start_polling` are no longer blocked. Refreshed stale line references. |
