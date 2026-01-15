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
- [x] Thread-safe operations with lock-free reference counting where possible
- [x] Clean integration with sonos-stream's EventBroker

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         sonos-state                                      │
│  (Public API: StateManager, PropertyWatcher, watch_property<P>())       │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │
                                 │ Uses
                                 ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      sonos-event-manager                                 │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                     SonosEventManager                             │   │
│  │  ┌────────────────┐  ┌──────────────────┐  ┌─────────────────┐   │   │
│  │  │   Device       │  │   Reference      │  │   EventBroker   │   │   │
│  │  │   Registry     │  │   Counting       │  │   (wrapped)     │   │   │
│  │  │  HashMap<IP,   │  │  DashMap<Key,    │  │                 │   │   │
│  │  │    Device>     │  │    AtomicUsize>  │  │                 │   │   │
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
└── error.rs            # Error types (EventManagerError)
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `lib.rs` | Public API, re-exports from dependencies, prelude module | `pub` |
| `manager.rs` | SonosEventManager struct and all subscription management logic | `pub` |
| `error.rs` | EventManagerError enum and Result type alias | `pub` |

### 2.3 Key Types

#### `SonosEventManager`

```rust
pub struct SonosEventManager {
    /// The underlying event broker from sonos-stream
    broker: EventBroker,

    /// Map of device IP addresses to device information
    devices: Arc<RwLock<HashMap<IpAddr, Device>>>,

    /// Reference counting for service subscriptions: (device_ip, service) -> ref_count
    service_refs: Arc<DashMap<(IpAddr, Service), AtomicUsize>>,
}
```

**Purpose**: Central facade that coordinates device registration, subscription lifecycle, and event stream access.

**Invariants**:
- Reference counts are always non-negative
- A subscription exists in EventBroker if and only if the reference count is > 0
- Device map entries are never removed (devices can be added but not explicitly removed)

**Ownership**: Created once per application, typically owned by `sonos-state::StateManager`. Wrapped in `Arc<RwLock<>>` for shared access.

#### `EventManagerError`

```rust
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
}
```

**Purpose**: Comprehensive error type covering all failure modes in subscription management.

---

## 3. Code Flow

### 3.1 Primary Flow: Service Subscription (First Watcher)

```
┌───────────────────┐     ┌────────────────────┐     ┌─────────────────┐
│  sonos-state      │────▶│  SonosEventManager │────▶│  EventBroker    │
│  watch_property() │     │  ensure_service_   │     │  register_      │
│                   │     │  subscribed()      │     │  speaker_service│
└───────────────────┘     └────────────────────┘     └─────────────────┘
       │                          │                          │
       ▼                          ▼                          ▼
   User calls             Check ref count              Create UPnP
   watch<Volume>()        (count == 0?)                subscription
```

**Step-by-step**:

1. **Entry** (`src/manager.rs:78`): `ensure_service_subscribed()` is called with device IP and service
2. **Key Creation** (`src/manager.rs:79`): Create tuple key `(device_ip, service)`
3. **Reference Check** (`src/manager.rs:82-83`): Get or create atomic counter, fetch-and-add atomically
4. **Conditional Registration** (`src/manager.rs:86-102`): If old count was 0, call `broker.register_speaker_service()`
5. **Logging** (`src/manager.rs:106-109`): Debug log the reference count transition

### 3.2 Secondary Flow: Service Subscription (Subsequent Watchers)

When reference count > 0, the flow is much simpler:

1. **Entry** (`src/manager.rs:78`): Same entry point
2. **Atomic Increment** (`src/manager.rs:83`): `fetch_add(1, SeqCst)` returns old count > 0
3. **Skip Registration** (`src/manager.rs:86`): Condition `old_count == 0` is false, no network call
4. **Return** (`src/manager.rs:111`): Success without any broker interaction

### 3.3 Tertiary Flow: Subscription Release

```
┌───────────────────┐     ┌────────────────────┐     ┌─────────────────┐
│  PropertyWatcher  │────▶│  SonosEventManager │────▶│  EventBroker    │
│  drop()           │     │  release_service_  │     │  (cleanup if    │
│                   │     │  subscription()    │     │   count == 0)   │
└───────────────────┘     └────────────────────┘     └─────────────────┘
```

**Step-by-step**:

1. **Entry** (`src/manager.rs:118`): `release_service_subscription()` called
2. **Counter Lookup** (`src/manager.rs:121`): Check if key exists in DashMap
3. **Atomic Decrement** (`src/manager.rs:122-123`): `fetch_sub(1, SeqCst)`, compute new count
4. **Conditional Cleanup** (`src/manager.rs:131-140`): If new count == 0, remove from DashMap
5. **Broker Unregistration** (`src/manager.rs:135-140`): TODO marker indicates cleanup not fully implemented

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

Atomic reference counting tracks how many consumers need each (device_ip, service) subscription. The count automatically manages subscription creation and cleanup.

#### Why

Without reference counting, either:
- Each watcher creates its own subscription (wasteful)
- A single shared subscription with manual lifecycle management (error-prone)
- Complex pub/sub routing per consumer (over-engineered)

Reference counting provides automatic, correct lifecycle management.

#### How

```rust
// First watcher - creates subscription
manager.ensure_service_subscribed(device_ip, Service::RenderingControl).await?;
// count: 0 -> 1, registers with EventBroker

// Second watcher - increments count only
manager.ensure_service_subscribed(device_ip, Service::RenderingControl).await?;
// count: 1 -> 2, no network call

// Second watcher dropped
manager.release_service_subscription(device_ip, Service::RenderingControl).await?;
// count: 2 -> 1, subscription remains

// First watcher dropped
manager.release_service_subscription(device_ip, Service::RenderingControl).await?;
// count: 1 -> 0, triggers cleanup
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| `AtomicUsize` for counts | `RwLock<usize>` | Lock-free performance for the common case (increment/decrement) |
| `DashMap` for key storage | `Mutex<HashMap>` | Fine-grained locking per entry instead of global lock |
| `SeqCst` ordering | `Relaxed` | Correctness over performance; subscription lifecycle must be strictly ordered |

### 4.2 Feature: Device Registry

#### What

A thread-safe mapping from IP addresses to discovered device information.

#### Why

Higher-level code works with device identifiers, but network operations need IP addresses. The registry provides this translation.

#### How

```rust
// Add devices from discovery
let devices = sonos_discovery::get();
event_manager.add_devices(devices).await?;

// Query devices
let all_devices = event_manager.devices().await;
let specific = event_manager.device_by_ip(ip).await;
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| No device removal API | Full CRUD operations | Simpler model; device removal is rare and can be handled by restarting |
| `Arc<RwLock<HashMap>>` | `DashMap` | Simpler; device registry is rarely modified after initial setup |

### 4.3 Feature: Multiplexed Event Stream

#### What

A single `EventIterator` provides access to ALL events from ALL registered devices and services.

#### Why

- Simplifies consumer code (one event loop, not N per device)
- Each event is tagged with `speaker_ip` and `service` for routing
- Matches the state management model where one processor handles all updates

#### How

```rust
let mut events = event_manager.get_event_iterator()?;
while let Some(enriched_event) = events.next_async().await {
    // enriched_event.speaker_ip and enriched_event.service for routing
    match enriched_event.service {
        Service::RenderingControl => handle_volume_mute(enriched_event),
        Service::AVTransport => handle_playback(enriched_event),
        _ => {}
    }
}
```

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

**Memory considerations**: Each entry is ~24 bytes (16 bytes for IpAddr + 8 bytes for Service enum + AtomicUsize overhead in DashMap).

#### Device Entry

```rust
// From sonos_discovery::Device
pub struct Device {
    pub id: String,
    pub name: String,
    pub ip_address: String,
    pub port: u16,
    pub model_name: String,
    pub room_name: String,
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
- **Unsubscribed**: No entry in `service_refs` DashMap, no active EventBroker registration
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
| `tokio` | Async runtime | Required for async/await, RwLock, channels |
| `dashmap` | Concurrent HashMap | Lock-free reference counting storage |
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

**Location**: `src/manager.rs` inline `#[cfg(test)]` module

**What to test**:
- [x] Initial subscription state (not subscribed)
- [x] Device management (add, query, lookup)
- [ ] Reference count increment/decrement
- [ ] First-subscription triggers registration
- [ ] Last-release triggers cleanup

**Example**:
```rust
#[tokio::test]
async fn test_device_management() {
    let manager = SonosEventManager::new().await.unwrap();
    assert!(manager.devices().await.is_empty());

    let devices = vec![Device { /* ... */ }];
    manager.add_devices(devices).await.unwrap();

    assert_eq!(manager.devices().await.len(), 1);
}
```

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
| `EventBroker` | Real broker in tests (no mocking) | N/A - uses actual sonos-stream |
| `Device` | Inline struct construction | `src/manager.rs:218-226` |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Reference count operation | < 1us | Hot path; should not block event processing |
| First subscription latency | < 500ms | Includes UPnP network round-trip |
| Memory per subscription | < 100 bytes | Support many devices without excessive memory |

### 9.2 Critical Paths

1. **Reference Count Update** (`src/manager.rs:83`)
   - **Complexity**: O(1) amortized (DashMap + AtomicUsize)
   - **Bottleneck**: DashMap shard lock acquisition
   - **Optimization**: `AtomicUsize` avoids locking for increment/decrement

2. **Event Iteration** (`src/manager.rs:156-158`)
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
| Reference count manipulation | Very Low | Low | Atomic operations; internal API only |
| DoS via subscription spam | Low | Medium | sonos-stream has max_registrations limit |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Device IP addresses | Low | Local network only; not transmitted externally |
| Device names | Low | User-chosen names; not sensitive |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| Device IP from discovery | IP address parsing | `src/manager.rs:51-54` |
| Service enum | Type-safe enum from sonos-api | Compile-time |

---

## 11. Observability

### 11.1 Logging

| Level | What's Logged | Example |
|-------|--------------|---------|
| `debug` | Reference count transitions | "Service reference count for 192.168.1.100 RenderingControl: 0 -> 1" |
| `debug` | Registration with EventBroker | "Registered RenderingControl for device 192.168.1.100" |
| `debug` | Manager drop statistics | "SonosEventManager dropping, 3 active service subscriptions" |
| `warn` | Release without reference | "Attempted to release subscription but no references found" |

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
| `callback_port_range` | `Range<u16>` | `8000..8100` | Port range for HTTP callback server |
| `base_polling_interval` | `Duration` | 30s | Polling interval when events unavailable |
| `enable_proactive_firewall_detection` | `bool` | `true` | Whether to detect firewall blocking |

```rust
let config = BrokerConfig::default()
    .with_callback_port_range(8000..8100)
    .with_polling_interval(Duration::from_secs(30));

let manager = SonosEventManager::with_config(config).await?;
```

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `SonosEventManager::new()` | Unstable | Internal crate; may change |
| `ensure_service_subscribed()` | Unstable | Core API but internal |
| `release_service_subscription()` | Unstable | May be replaced with RAII guard |
| `get_event_iterator()` | Unstable | Single-use; may change to repeated access |

### 13.2 Breaking Changes

**Policy**: As an internal crate, breaking changes are coordinated with `sonos-state` and do not follow semver guarantees for external consumers.

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
| No EventBroker unregistration | Subscriptions may not be fully cleaned up | Manager drop clears everything | TODO in `release_service_subscription()` |
| Single event iterator | Can only call `get_event_iterator()` once | Design intentional | None - architectural choice |
| No device removal | Cannot remove devices once added | Recreate manager | Evaluate need based on usage |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| Missing broker unregister | `src/manager.rs:135-140` | Medium | Extend EventBroker API or track registration IDs |
| Blocking IP lookup in Drop | `sonos-state/src/reactive.rs:134-141` | Low | Acceptable for Drop; consider caching |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Full subscription cleanup | P1 | Prevent resource leaks | EventBroker unregister API |
| Subscription health monitoring | P2 | Detect stale subscriptions | Metrics infrastructure |
| Device removal API | P2 | Support dynamic device changes | Usage analysis |

### 15.2 Open Questions

- [ ] **Should reference counting be at the property level instead of service level?** Currently, watching Volume and Mute both increment the RenderingControl count. This is correct but coarse-grained. Property-level counting would be more precise but add complexity.

- [ ] **Should we expose subscription state changes as events?** UI could show "Connected to Speaker A" status. Would require additional event type.

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
- [sonos-stream crate](../sonos-stream) - Underlying event infrastructure
- [sonos-state crate](../sonos-state) - Primary consumer of this crate

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2025-01-14 | Claude Code | Initial specification |
