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
    pub event_data: EventData,
}
```

**Purpose**: Unified event structure that combines raw event data with full context.

**Invariants**:
- `registration_id` always maps to a valid registration in the registry
- `timestamp` reflects when the event was processed, not when it occurred on the device
- `event_source` accurately identifies whether this came from UPnP or polling

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

1. **Registration** (`src/broker.rs:385-489`): User calls `register_speaker_service()` which:
   - Registers the speaker/service pair in the registry
   - Checks if this is the first subscription for this device
   - Triggers firewall detection if enabled
   - Creates a UPnP subscription via SubscriptionManager
   - Evaluates whether to start immediate polling based on firewall status

2. **Firewall Detection** (`src/broker.rs:406-417`): Per-device firewall detection:
   - First subscription triggers proactive detection
   - Subsequent subscriptions use cached status
   - Detection runs concurrently with subscription creation

3. **Subscription Creation** (`src/subscription/manager.rs:181-211`):
   - Creates UPnP subscription using SonosClient
   - Registers subscription ID with EventRouter for event routing
   - Wraps in ManagedSubscriptionWrapper with additional context

4. **Event Arrival** (`src/events/processor.rs:51-126`):
   - Callback server receives UPnP NOTIFY message
   - EventProcessor looks up subscription by SID
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
         │ broker.rs:441-458                           │
         │                                              ▼
         │                                     ┌─────────────────┐
         │                                     │  PollingTask    │
         │                                     │ scheduler.rs:51 │
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

1. **Detection Trigger** (`src/broker.rs:441-458`): Firewall status triggers immediate polling decision
2. **Polling Start** (`src/polling/scheduler.rs:466-504`): PollingScheduler creates new PollingTask
3. **State Polling** (`src/polling/scheduler.rs:104-259`): PollingTask runs loop with configurable interval
4. **Change Detection** (`src/polling/strategies.rs:97-160`): Service-specific pollers detect state changes
5. **Event Generation** (`src/polling/scheduler.rs:183-209`): State changes converted to EnrichedEvents with PollingDetection source

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

Adaptive intervals calculated in `src/polling/scheduler.rs:262-280`:

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
    /// Delay before activating polling after firewall detection (default: 5s)
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
- **Unregistered**: No registry entry, no subscription, no polling
- **Registered/UPnP Only**: Active UPnP subscription, no polling task
- **Registered/Polling Only**: May have failed subscription, active polling task
- **Registered/Both**: Transitioning states, temporary condition
- **Failed**: Registration exists but no event source active

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
- [x] Configuration validation (`src/config.rs:231-289`)
- [x] Registration duplicate detection (`src/registry.rs:305-316`)
- [x] Event type creation and service mapping (`src/events/types.rs:318-389`)
- [x] Iterator statistics tracking (`src/events/iterator.rs:569-582`)
- [x] Adaptive interval calculation (`src/polling/scheduler.rs:660-674`)
- [x] Change detection for AVTransport/RenderingControl (`src/polling/strategies.rs:432-498`)

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

2. **Registry Lookup** (`src/registry.rs:191-199`)
   - **Complexity**: O(1) HashMap lookup
   - **Bottleneck**: Write lock contention under high registration churn
   - **Optimization**: Uses bidirectional HashMap for O(1) both directions

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| HTTP connections | Via SonosClient singleton | On request completion | Yes - shared soap-client |
| UPnP subscriptions | On registration | On unregistration/shutdown | No - per-registration |
| Polling tasks | On fallback trigger | On explicit stop or error | No - per-registration |
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

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| eprintln! instead of tracing | Throughout | Low | Replace with tracing macros |
| Incomplete position info polling | `strategies.rs:84-90` | Medium | Add get_position_info_operation call |
| Hardcoded error thresholds | `scheduler.rs:239` | Low | Move to BrokerConfig |

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
