# sonos-state Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

Building reactive applications that display real-time Sonos device state requires solving several complex challenges:

1. **UPnP Subscription Management**: Each property type (volume, playback state, track info) requires subscribing to specific UPnP services. Managing these subscriptions manually is error-prone and leads to either resource leaks (forgotten unsubscribes) or missing events (forgotten subscribes).

2. **State Synchronization**: UPnP events arrive as raw XML from different services. Converting these into typed, application-usable state while maintaining consistency across multiple devices is non-trivial.

3. **Change Detection**: Applications need to know when state changes to update their UI. Without a reactive system, developers must implement polling or manual change tracking.

4. **Resource Efficiency**: Multiple UI components may need the same property (e.g., volume displayed in multiple places). Without shared subscriptions, this leads to duplicate network traffic and memory usage.

Without `sonos-state`, application developers would need to:
- Manually track which UPnP services to subscribe to for each property
- Implement their own event parsing and state storage
- Build custom change notification mechanisms
- Handle subscription lifecycle (renewal, cleanup) manually
- Duplicate this code across every Sonos application

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Demand-driven subscription management | UPnP subscriptions should be created automatically when properties are watched and cleaned up when no longer needed, eliminating resource leaks and manual management |
| P0 | Type-safe property access | Properties must be strongly typed with compile-time guarantees, preventing runtime errors from type mismatches |
| P0 | Reactive change notifications | Applications must receive immediate notifications when any property changes, enabling responsive UIs |
| P1 | Resource sharing via reference counting | Multiple watchers for the same property should share a single UPnP subscription to minimize network traffic |
| P1 | Thread-safe concurrent access | All APIs must be safe to use from multiple threads without external synchronization |
| P2 | Synchronous API support | CLI applications and non-async contexts should be able to access state without async/await |
| P2 | TUI framework integration | Provide patterns optimized for ratatui and similar terminal UI frameworks |

### 1.3 Non-Goals

- **Device Control Operations**: This crate manages state, not commands. Use `sonos-api` for play/pause/volume control operations.
- **Persistent State Storage**: State exists only in memory. Historical data or persistence is out of scope.
- **Network Discovery**: Device discovery is handled by `sonos-discovery`. This crate assumes devices are already known.
- **Direct UPnP Communication**: Low-level SOAP/UPnP operations are delegated to `sonos-api` and `sonos-stream`.

### 1.4 Success Criteria

- [x] Properties automatically trigger correct UPnP service subscriptions when watched
- [x] Multiple watchers share the same subscription (verified via reference counting)
- [x] Dropping the last watcher automatically cleans up the subscription
- [x] State updates propagate to all watchers within milliseconds of event receipt
- [x] Type-safe property access with compile-time service association
- [x] Thread-safe operation with interior mutability

---

## 2. Architecture

### 2.1 High-Level Design

```
                                +-----------------------+
                                |    Application        |
                                |  (TUI, Dashboard, etc)|
                                +-----------+-----------+
                                            |
                        watch_property<P>() | get_property<P>()
                                            v
+-----------------------------------------------------------------------------------+
|                              StateManager (reactive.rs)                            |
|  +-----------------------+  +---------------------------+  +-------------------+   |
|  | PropertySubscription  |  | CoreStateManager          |  | Event Processor   |   |
|  | Manager               |  | (state_manager.rs)        |  | (background task) |   |
|  | - Reference counting  |  | - Decoder orchestration   |  | - Converts events |   |
|  | - SonosEventManager   |  | - Event routing           |  | - Feeds decoders  |   |
|  +-----------------------+  +---------------------------+  +-------------------+   |
+-----------------------------------------------------------------------------------+
                                            |
                                            v
+-----------------------------------------------------------------------------------+
|                              StateStore (store.rs)                                 |
|  +-------------------+  +-------------------+  +-------------------+               |
|  | speaker_props     |  | group_props       |  | system_props      |               |
|  | HashMap<SpeakerId,|  | HashMap<GroupId,  |  | PropertyBag       |               |
|  |   PropertyBag>    |  |   PropertyBag>    |  | (single instance) |               |
|  +-------------------+  +-------------------+  +-------------------+               |
|                                                                                    |
|  PropertyBag: HashMap<TypeId, watch::Sender<Option<P>>>                           |
+-----------------------------------------------------------------------------------+
            ^                       ^                       ^
            |                       |                       |
   RenderingControl        AVTransport               ZoneGroupTopology
       Decoder                Decoder                   Decoder
            ^                       ^                       ^
            |                       |                       |
+-----------------------------------------------------------------------------------+
|                         RawEvent (decoder.rs)                                      |
|  - speaker_ip: IpAddr                                                             |
|  - service: Service                                                               |
|  - data: EventData (RenderingControl | AVTransport | ZoneGroupTopology | ...)     |
+-----------------------------------------------------------------------------------+
            ^
            |  convert_enriched_to_raw_event()
            |
+-----------------------------------------------------------------------------------+
|                    sonos-stream (EnrichedEvent)                                    |
|                    sonos-event-manager (subscription orchestration)                |
+-----------------------------------------------------------------------------------+
```

**Design Rationale**: This layered architecture separates concerns clearly:

1. **StateManager** (public API) orchestrates the reactive system, hiding complexity from users
2. **StateStore** (data layer) provides type-erased storage with `tokio::sync::watch` channels for reactivity
3. **Decoders** (transformation layer) convert UPnP-specific data into typed properties
4. **PropertySubscriptionManager** (resource layer) implements reference-counted subscription sharing

The use of `TypeId` for type-erased storage allows heterogeneous properties in a single container while maintaining type safety through the `Property` trait bounds.

### 2.2 Module Structure

```
src/
+-- lib.rs                  # Public API surface and re-exports
+-- reactive.rs             # StateManager (main entry point)
+-- state_manager.rs        # CoreStateManager (internal)
+-- store.rs                # StateStore, PropertyBag, StateChange
+-- property.rs             # Property trait and built-in properties
+-- decoder.rs              # EventDecoder trait, RawEvent, EventData
+-- decoders/               # Service-specific decoders
|   +-- mod.rs              # default_decoders()
|   +-- rendering.rs        # RenderingControlDecoder
|   +-- transport.rs        # AVTransportDecoder
|   +-- topology.rs         # TopologyDecoder
+-- model/                  # Core data types
|   +-- mod.rs              # Re-exports
|   +-- id_types.rs         # SpeakerId, GroupId
|   +-- speaker.rs          # Speaker/SpeakerInfo
+-- watcher.rs              # SyncWatcher for non-async contexts
+-- change_iterator.rs      # ChangeStream, ChangeFilter, WidgetStateManager
+-- error.rs                # StateError, Result type
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `reactive` | High-level reactive API with automatic subscriptions | `pub` |
| `state_manager` | Internal event processing coordination | `pub(crate)` |
| `store` | Type-erased property storage with watch channels | `pub` |
| `property` | Property trait definition and built-in properties | `pub` |
| `decoder` | Event decoding abstractions and types | `pub` |
| `decoders/*` | Service-specific decoder implementations | `pub` |
| `model` | Identity types and speaker metadata | `pub` |
| `watcher` | Synchronous API wrapper | `pub` |
| `change_iterator` | Application-level change streams | `pub` |
| `error` | Error types | `pub` |

### 2.3 Key Types

#### `StateManager` (reactive.rs:199)

```rust
pub struct StateManager {
    core_state_manager: CoreStateManager,
    event_manager: Arc<RwLock<SonosEventManager>>,
    subscription_manager: Arc<PropertySubscriptionManager>,
    _event_processor: JoinHandle<()>,
}
```

**Purpose**: The primary public interface for reactive state management. Orchestrates device registration, property watching, and automatic subscription lifecycle.

**Invariants**:
- Event processor task runs continuously while StateManager exists
- All registered devices have corresponding entries in subscription_manager.speaker_ips

**Ownership**: Created by application code, typically wrapped in `Arc` for sharing across tasks.

#### `StateStore` (store.rs:209)

```rust
pub struct StateStore {
    speaker_props: Arc<RwLock<HashMap<SpeakerId, PropertyBag>>>,
    group_props: Arc<RwLock<HashMap<GroupId, PropertyBag>>>,
    system_props: Arc<RwLock<PropertyBag>>,
    speakers: Arc<RwLock<HashMap<SpeakerId, SpeakerInfo>>>,
    groups: Arc<RwLock<HashMap<GroupId, GroupInfo>>>,
    ip_to_speaker: Arc<RwLock<HashMap<IpAddr, SpeakerId>>>,
    changes_tx: broadcast::Sender<StateChange>,
}
```

**Purpose**: Central repository for all Sonos state. Provides both instant queries (`get`) and reactive subscriptions (`watch`).

**Invariants**:
- Every speaker in `speakers` has a corresponding entry in `ip_to_speaker`
- Property values only change through `set` methods (which notify watchers)
- `changes_tx` broadcasts all state mutations

**Ownership**: Owned by CoreStateManager but can be cloned (shares underlying data via Arc).

#### `Property` Trait (property.rs:49)

```rust
pub trait Property: Clone + Send + Sync + PartialEq + 'static {
    const KEY: &'static str;
    const SCOPE: Scope;
    const SERVICE: Service;
}
```

**Purpose**: Marker trait connecting property types to their storage scope and UPnP service. Enables compile-time association between properties and their event sources.

**Invariants**:
- `KEY` must be unique within a scope
- `SERVICE` correctly identifies which UPnP service provides this property

#### `PropertyWatcher<P>` (reactive.rs:50)

```rust
pub struct PropertyWatcher<P: Property> {
    property_receiver: watch::Receiver<Option<P>>,
    speaker_id: SpeakerId,
    service: Service,
    subscription_manager: Arc<PropertySubscriptionManager>,
}
```

**Purpose**: Handle for watching a specific property. Implements Drop to release subscription reference count.

**Invariants**:
- Dropping a PropertyWatcher decrements the subscription reference count
- `property_receiver` always connected to a valid watch channel

---

## 3. Code Flow

### 3.1 Primary Flow: Watching a Property

The most common operation traces from `watch_property<P>()` through subscription creation to receiving updates.

```
+-------------------+     +------------------------+     +-------------------+
| Application calls |---->| PropertySubscription   |---->| SonosEventManager |
| watch_property<P> |     | Manager.ensure_        |     | .ensure_service_  |
|                   |     | subscription()         |     | subscribed()      |
+-------------------+     +------------------------+     +-------------------+
        |                          |                            |
        v                          v                            v
  reactive.rs:410            reactive.rs:145              (sonos-event-manager)
        |                          |
        |                          |  ref_count++ or create
        v                          |
+-------------------+              |
| StateStore.watch  |<-------------+
| ::<P>(&speaker_id)|
+-------------------+
        |
        v
  store.rs:286
        |
        |  Creates watch::Receiver<Option<P>>
        v
+-------------------+
| PropertyWatcher<P>|
| returned to app   |
+-------------------+
```

**Step-by-step**:

1. **Entry** (`src/reactive.rs:410`): `watch_property<P>()` retrieves speaker IP from subscription manager
2. **Subscription Lookup** (`src/reactive.rs:145`): `ensure_subscription()` checks reference count for (speaker_ip, service) key
3. **Create or Increment** (`src/reactive.rs:149-157`): If count is 0, creates UPnP subscription via SonosEventManager; increments reference count
4. **Watch Channel** (`src/store.rs:286-290`): `StateStore.watch<P>()` creates or returns existing watch channel receiver
5. **Return Watcher** (`src/reactive.rs:428-432`): Creates PropertyWatcher wrapping the receiver and subscription manager reference

### 3.2 Secondary Flow: Event Processing

Events flow from UPnP through decoders to the state store, notifying watchers.

```
+-------------------+     +-------------------+     +-------------------+
| sonos-stream      |---->| Event Processor   |---->| CoreStateManager  |
| EnrichedEvent     |     | convert_enriched_ |     | .process()        |
|                   |     | to_raw_event()    |     |                   |
+-------------------+     +-------------------+     +-------------------+
        |                          |                        |
        v                          v                        v
  (external)              reactive.rs:289           state_manager.rs:96
                                   |                        |
                                   |                        |  for each decoder
                                   |                        v
                                   |              +-------------------+
                                   |              | Decoder.decode()  |
                                   |              | returns Vec<      |
                                   |              | PropertyUpdate>   |
                                   |              +-------------------+
                                   |                        |
                                   |                        v
                                   |              decoder.rs:274
                                   |                        |
                                   |                        |  update.apply(store)
                                   |                        v
                                   |              +-------------------+
                                   |              | StateStore.set()  |
                                   |              | - Updates value   |
                                   |              | - Broadcasts change|
                                   |              +-------------------+
                                   |                        |
                                   |                        v
                                   |              store.rs:310-323
                                   |                        |
                                   |                        |  watch::Sender.send_replace()
                                   |                        v
                                   |              +-------------------+
                                   |              | PropertyWatcher   |
                                   +------------->| .changed().await  |
                                                  | receives update   |
                                                  +-------------------+
```

**Step-by-step**:

1. **Event Receipt** (`src/reactive.rs:254`): Background task receives EnrichedEvent from sonos-stream
2. **Conversion** (`src/reactive.rs:289-373`): `convert_enriched_to_raw_event()` transforms stream format to internal RawEvent
3. **Processing** (`src/state_manager.rs:96-111`): `process()` routes event to matching decoders based on service
4. **Decoding** (e.g., `src/decoders/rendering.rs:22-96`): Decoder extracts property values and creates PropertyUpdate closures
5. **Application** (`src/decoder.rs:274`): Each PropertyUpdate closure calls `StateStore.set()`
6. **Notification** (`src/store.rs:317-323`): `set()` uses `send_replace()` on watch channel, notifying all receivers
7. **Change Broadcast** (`src/store.rs:318`): `changes_tx.send()` broadcasts StateChange for change stream subscribers

### 3.3 Error Flow

```
[UPnP/Network Error] --> [sonos-event-manager] --> [StateError::SubscriptionFailed]
                                                            |
[Invalid IP Address] --> [StateManager.add_devices] ------->| StateError::InvalidIpAddress
                                                            |
[Unknown Speaker] --> [watch_property] ------------------->| StateError::SpeakerNotFound
                                                            |
[Parse Failure] --> [Decoder] --> [Empty Vec<PropertyUpdate>] (silent skip)
```

**Error handling philosophy**: Errors are categorized by recoverability:
- **Fatal errors** (initialization failures) prevent StateManager creation
- **Recoverable errors** (subscription failures) are returned to caller for retry
- **Silent failures** (parse errors in decoders) log warnings but don't propagate, allowing partial event processing

---

## 4. Features

### 4.1 Feature: Demand-Driven Subscriptions

#### What

UPnP service subscriptions are automatically created when a property is first watched and cleaned up when all watchers are dropped.

#### Why

Manual subscription management is the #1 source of bugs in UPnP applications:
- Forgetting to subscribe leads to missing events
- Forgetting to unsubscribe leads to resource leaks
- Renewing subscriptions at the right time is complex

By tying subscription lifecycle to property usage, these bugs are eliminated.

#### How

The `PropertySubscriptionManager` maintains a `HashMap<SubscriptionKey, usize>` mapping (speaker_ip, service) pairs to reference counts:

```rust
// src/reactive.rs:145-158
async fn ensure_subscription(&self, key: &SubscriptionKey) -> Result<()> {
    let mut refs = self.subscription_refs.write().await;
    let current_count = refs.get(key).copied().unwrap_or(0);

    if current_count == 0 {
        // First reference - create the subscription
        let mut event_manager = self.event_manager.write().await;
        event_manager.ensure_service_subscribed(key.speaker_ip, key.service).await?;
    }

    refs.insert(key.clone(), current_count + 1);
    Ok(())
}
```

The `PropertyWatcher::Drop` implementation (src/reactive.rs:91-105) spawns an async task to decrement the reference count and clean up when it reaches zero.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Reference counting per (ip, service) | Per-property counting | Multiple properties from same service should share subscription |
| Async cleanup in Drop | Blocking cleanup | Cannot block in Drop; spawned task handles async operations |
| Silent cleanup failures | Propagate errors | Drop cannot return errors; logging is sufficient for cleanup |

### 4.2 Feature: Type-Safe Property System

#### What

Properties are defined as types implementing the `Property` trait, with compile-time association to their scope and service.

#### Why

The UPnP specification defines many properties across multiple services. Without type safety:
- Developers must remember which service provides which property
- Type mismatches cause runtime panics
- Refactoring is error-prone

#### How

```rust
// src/property.rs:70-77
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Volume(pub u8);

impl Property for Volume {
    const KEY: &'static str = "volume";
    const SCOPE: Scope = Scope::Speaker;
    const SERVICE: Service = Service::RenderingControl;
}
```

The `StateStore` uses `TypeId::of::<P>()` for type-erased storage while maintaining type safety:

```rust
// src/store.rs:132-144
fn get_or_create_sender<P: Property>(&mut self) -> &watch::Sender<Option<P>> {
    let type_id = TypeId::of::<P>();
    if !self.channels.contains_key(&type_id) {
        let (tx, _rx) = watch::channel::<Option<P>>(None);
        self.channels.insert(type_id, Box::new(tx));
    }
    self.channels.get(&type_id)
        .and_then(|boxed| boxed.downcast_ref::<watch::Sender<Option<P>>>())
        .expect("PropertyBag: type mismatch (this is a bug)")
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| TypeId-based storage | Generic structs per scope | Allows unlimited property types without code generation |
| Associated constants | Method-based | Constants enable zero-cost abstraction and const evaluation |
| Option<P> in channels | Separate "has value" flag | Natural representation of "not yet received" state |

### 4.3 Feature: Global Change Stream

#### What

A broadcast channel emits `StateChange` events for all property mutations, with filtering and rerender hint support.

#### Why

Applications (especially TUIs) need to know when any state changes to trigger re-renders. Per-property watching doesn't scale when the UI must display many properties.

#### How

```rust
// src/change_iterator.rs:122-166
pub struct ChangeStream {
    receiver: broadcast::Receiver<StateChange>,
    filter: Option<ChangeFilter>,
}

impl ChangeStream {
    pub async fn next(&mut self) -> Option<ChangeEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(state_change) => {
                    let change_event = Self::convert_state_change(state_change);
                    if let Some(ref filter) = self.filter {
                        if !filter.matches(&change_event) {
                            continue;
                        }
                    }
                    return Some(change_event);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }
}
```

The `ChangeContext` provides smart rerender hints based on property type:

```rust
// src/change_iterator.rs:211-248
let (requires_rerender, rerender_scope, description) = match property_key {
    "volume" => (true, RerenderScope::Device(speaker_id.clone()), ...),
    "position" => (false, RerenderScope::Device(speaker_id.clone()), ...), // Throttled
    "current_track" => (true, RerenderScope::Device(speaker_id.clone()), ...),
    _ => (true, RerenderScope::Device(speaker_id.clone()), ...),
};
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| broadcast channel | mpsc per subscriber | Simpler API, automatic fan-out, lagged message handling |
| Rerender hints in ChangeContext | External hint registry | Centralized knowledge, easier to maintain |
| Filter in ChangeStream | External filter wrapper | Avoids unnecessary event allocation for filtered events |

### 4.4 Feature: Synchronous API Support

#### What

`SyncWatcher<P>` wraps async watch channels for use in non-async contexts.

#### Why

CLI applications and some UI frameworks cannot use async/await. Blocking on a tokio runtime handle provides synchronous access.

#### How

```rust
// src/watcher.rs:58-61
pub fn wait(&mut self) -> Option<P> {
    self.rt.block_on(self.rx.changed()).ok()?;
    self.rx.borrow().clone()
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Require runtime handle | Create internal runtime | Explicit dependency, avoids hidden overhead |
| Clone on read | Reference with lifetime | Simpler API, avoids lifetime complexity |

---

## 5. Data Model

### 5.1 Core Data Structures

#### `SpeakerId` / `GroupId` (model/id_types.rs)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpeakerId(String);

impl SpeakerId {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let normalized = id.strip_prefix("uuid:").unwrap_or(&id);
        Self(normalized.to_string())
    }
}
```

**Lifecycle**:
1. **Creation**: From device discovery (UUID) or topology events
2. **Mutation**: Immutable after creation
3. **Destruction**: When speaker is removed from StateStore

**Memory considerations**: Small fixed-size (typically 20-30 characters), frequently cloned for HashMap keys.

#### `SpeakerInfo` (model/speaker.rs)

```rust
pub struct Speaker {
    pub id: SpeakerId,
    pub name: String,
    pub room_name: String,
    pub ip_address: IpAddr,
    pub port: u16,
    pub model_name: String,
    pub software_version: String,
    pub satellites: Vec<SpeakerId>,
}
```

**Lifecycle**:
1. **Creation**: From device discovery or topology events
2. **Mutation**: Updated via `StateStore.add_speaker()` (full replacement)
3. **Destruction**: Via `StateStore.remove_speaker()`

#### `PropertyBag` (store.rs:118)

```rust
struct PropertyBag {
    channels: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}
```

**Lifecycle**:
1. **Creation**: Lazily when first property is set/watched for a speaker
2. **Mutation**: Watch channels created on first access per property type
3. **Destruction**: When speaker is removed

**Memory considerations**: One watch channel (Sender + initial Receiver) per property type accessed. Channel buffer is single-element (watch semantics).

### 5.2 State Transitions

```
                    add_devices()
                         |
                         v
+-------------+    +--------------+    watch_property<P>()    +------------------+
|  Untracked  |--->|  Registered  |-------------------------->| Subscribed       |
|  (no state) |    |  (in store)  |                           | (events flowing) |
+-------------+    +--------------+                           +------------------+
                         ^                                            |
                         |              last watcher dropped          |
                         |                                            |
                         +--------------------------------------------+
                                    (subscription released)
```

**Invariants per state**:
- **Untracked**: Device exists on network but not in StateStore
- **Registered**: Device in StateStore.speakers, IP mapped, no active property subscriptions
- **Subscribed**: At least one PropertyWatcher exists, UPnP subscription active for relevant service(s)

### 5.3 Serialization

| Format | Use Case | Library | Notes |
|--------|----------|---------|-------|
| Serde JSON/YAML | Property values, SpeakerId, GroupId | `serde` | Derive macros on all public types |
| DIDL-Lite XML | Track metadata parsing | Manual parsing | `parse_didl_*` functions in decoder.rs |

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `sonos-api` | Service enum, ApiError type | Canonical definition of Sonos UPnP services |
| `sonos-stream` | EnrichedEvent type | Provides real-time events with firewall fallback |
| `sonos-event-manager` | Subscription orchestration | Handles UPnP subscription lifecycle, renewal |
| `sonos-discovery` | Device type | Compatible device representation for add_devices() |
| `tokio` | Async runtime, sync primitives | watch channels for reactivity, spawn for background task |
| `serde` | Serialization traits | Property types need serialization support |
| `quick-xml` | XML parsing | DIDL-Lite metadata parsing |
| `tracing` | Logging | Consistent with workspace logging strategy |
| `thiserror` | Error derive | Structured error types |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| Application code | Primary consumer | StateManager, Property types are stable API |
| `integration-example` | Demo usage | (Currently disabled during refactoring) |

### 6.3 External Systems

```
+------------------+         +------------------+
|   sonos-state    |<------->|  Sonos Speakers  |
|                  |  UPnP   |  (via sonos-     |
|                  |  SOAP   |   stream/api)    |
+------------------+         +------------------+
```

**Protocol**: UPnP/SOAP over HTTP (port 1400)

**Authentication**: None required (local network only)

**Error handling**: Network errors surface as StateError::SubscriptionFailed; events from unreachable speakers stop arriving

**Retry strategy**: Delegated to sonos-event-manager (automatic subscription renewal, polling fallback)

---

## 7. Error Handling

### 7.1 Error Types

```rust
// src/error.rs:10-46
pub enum StateError {
    Init(String),
    Parse(String),
    Api(sonos_api::ApiError),
    AlreadyRunning,
    ShutdownFailed,
    LockError(String),
    SpeakerNotFound(SpeakerId),
    InvalidUrl(String),
    InitializationFailed(String),
    DeviceRegistrationFailed(String),
    SubscriptionFailed(String),
    InvalidIpAddress(String),
}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Structured errors | Enum variants with context | Enables pattern matching and specific handling |
| Error wrapping | `From<ApiError>`, `From<url::ParseError>` | Preserves error chain for debugging |
| Graceful degradation | Decoders return empty Vec on parse failure | Partial events shouldn't crash the system |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `InitializationFailed` | No | Application must exit or retry StateManager::new() |
| `SubscriptionFailed` | Yes | Retry watch_property(); check network connectivity |
| `SpeakerNotFound` | Yes | Ensure device is registered via add_devices() |
| `InvalidIpAddress` | No | Fix device data before calling add_devices() |
| `Parse` | Silent | Events with parse errors are skipped; others processed |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    +-------------------+
                    |  Integration/E2E  |  10% - Real Sonos devices
                    +--------+----------+
           +------------------+------------------+
           |       Component Tests              |  30% - Module interactions
           +------------------+------------------+
    +------+------+------+------+------+------+------+
    |                   Unit Tests                   |  60% - Individual functions
    +------------------------------------------------+
```

### 8.2 Unit Tests

**Location**: Inline `#[cfg(test)]` modules in each source file

**What to test**:
- [x] Property clamping (Volume 0-100, Bass -10 to +10)
- [x] PlaybackState parsing from UPnP strings
- [x] Position time string parsing
- [x] DIDL-Lite metadata extraction
- [x] SpeakerId normalization (uuid: prefix stripping)
- [x] PropertyBag type-erased storage
- [x] StateStore change detection (same value = no notification)
- [x] ChangeFilter matching logic

**Example** (src/property.rs:426-430):
```rust
#[test]
fn test_volume_clamping() {
    assert_eq!(Volume::new(50).value(), 50);
    assert_eq!(Volume::new(150).value(), 100);
    assert_eq!(Volume::new(0).value(), 0);
}
```

### 8.3 Component Tests

**Location**: Inline `#[tokio::test]` for async, `#[test]` for sync

**What to test**:
- [x] StateStore watch + set notification flow (store.rs:664-683)
- [x] Decoder → StateStore update chain (state_manager.rs:282-308)
- [x] ChangeStream filtering (change_iterator.rs:1018-1066)
- [x] SyncWatcher blocking behavior (watcher.rs:164-223)

### 8.4 Integration Tests

**Location**: `examples/` (manual verification)

**Prerequisites**:
- [x] At least one Sonos device on local network
- [x] Network allows UPnP multicast (SSDP) and callbacks

**What to test**:
- [x] `reactive_dashboard` - Subscription lifecycle
- [x] `live_dashboard` - Real-time property updates

### 8.5 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| `StateStore` | Real instance (lightweight) | Created inline with `create_test_store()` |
| `SpeakerInfo` | Factory functions | `create_test_speaker()` in test modules |
| Network events | RawEvent construction | Direct instantiation in decoder tests |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Event processing latency | < 10ms | Real-time UI responsiveness |
| Memory per property watcher | < 1KB | Support hundreds of watchers |
| Subscription overhead | 1 per (speaker, service) | Minimize network traffic |

### 9.2 Critical Paths

1. **Event Processing** (`src/reactive.rs:247-283`)
   - **Complexity**: O(d * p) where d = decoders, p = properties in event
   - **Bottleneck**: Decoder iteration and property update application
   - **Optimization**: Decoders short-circuit on service mismatch

2. **Property Set** (`src/store.rs:310-324`)
   - **Complexity**: O(1) hash lookup + O(1) watch send
   - **Bottleneck**: RwLock contention under high event rates
   - **Optimization**: `send_replace()` never blocks regardless of receiver count

3. **IP to Speaker Lookup** (`src/store.rs:479-481`)
   - **Complexity**: O(1) hash lookup
   - **Bottleneck**: Called on every event
   - **Optimization**: Dedicated `ip_to_speaker` HashMap avoids full speaker scan

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| UPnP subscriptions | On first watch_property for service | When ref count reaches 0 | Yes - shared per (speaker, service) |
| Watch channels | On first get/watch for property | Never (persist with speaker) | No - one per property type |
| Background task | StateManager::new() | StateManager drop | No - single per StateManager |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious UPnP events | Low (requires local network) | Medium (incorrect state) | Events only accepted from registered speaker IPs |
| Denial of service via event flooding | Low | Low (bounded channels) | broadcast channel with lag handling |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Speaker IPs | Low (local network) | Not logged at INFO level |
| Track metadata | Low (public info) | No special handling |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| Speaker IP from discovery | Must be valid IpAddr | `src/reactive.rs:387-388` |
| UPnP event XML | Graceful parsing failures | Decoder implementations |
| Property values | Type-specific clamping | Property constructors (e.g., `Volume::new`) |

---

## 11. Observability

### 11.1 Logging

| Level | What's Logged | Example |
|-------|--------------|---------|
| `error` | Subscription failures, initialization errors | "Failed to subscribe to RenderingControl" |
| `warn` | Subscription cleanup failures | "Failed to release service subscription" |
| `info` | Device additions, event processing stats | "Added 3 devices" |
| `debug` | Individual event processing | "Processing RenderingControl event" |
| `trace` | Property update details | "Set RINCON_123 volume to 50" |

### 11.2 Tracing

**Span structure**:
```
[state_manager::process]
  +-- [decoder::RenderingControl]
  +-- [store::set<Volume>]
```

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| Broadcast channel capacity | `usize` | 1000 | StateChange broadcast buffer size (store.rs:235) |

### 12.2 Environment Variables

None. All configuration is programmatic.

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `StateManager::new()`, `add_devices()`, `watch_property()`, `get_property()` | Stable | Core public API |
| `Property` trait and built-in properties | Stable | Adding properties is non-breaking |
| `ChangeStream`, `ChangeFilter` | Stable | May add filter options |
| `CoreStateManager` (state_manager.rs) | Internal | May change without notice |

### 13.2 Breaking Changes

**Policy**: Semantic versioning. Breaking changes require major version bump.

**Current deprecations**: None

### 13.3 Version History

| Version | Changes | Migration Guide |
|---------|---------|-----------------|
| 0.1.0 | Initial release | N/A |

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Topology conversion incomplete | Group membership may be stale | Poll ZoneGroupTopology manually | v0.2.0 |
| No subscription renewal handling | Subscriptions expire after 30min | Restart StateManager | Delegate to sonos-event-manager |
| Initial state requires first event | Properties start as None | Fetch initial values via sonos-api | Consider on-demand API fetch |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| Event processor eprintln! debug output | `src/reactive.rs:248-282` | Low | Replace with tracing macros |
| Hardcoded software_version "unknown" | `src/reactive.rs:401` | Low | Extract from device description |
| TODO comment in topology conversion | `src/reactive.rs:361` | Medium | Implement full topology mapping |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Initial state fetch on watch | P1 | Avoid waiting for first event | sonos-api GetVolume, GetTransportInfo |
| Subscription health monitoring | P1 | Detect stale subscriptions | Metrics system |
| Property change batching | P2 | Reduce UI re-render frequency | Timer-based coalescing |
| Custom decoder registration | P2 | Support additional services | None |

### 15.2 Open Questions

- [ ] **Should StateManager auto-discover devices?**: Currently requires explicit add_devices(). Auto-discovery would simplify API but reduces control.
- [ ] **Property history/time-series?**: Some applications want to track changes over time. Out of scope currently but frequently requested.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| Property | A typed value associated with a speaker, group, or system (e.g., Volume, PlaybackState) |
| Scope | Where a property is stored: Speaker (per-device), Group (per-zone), System (global) |
| PropertyBag | Type-erased container holding watch channels for multiple property types |
| Decoder | Component that converts UPnP events into PropertyUpdate closures |
| PropertyWatcher | Handle for receiving reactive updates to a specific property |
| ChangeStream | Broadcast receiver for all state changes with optional filtering |

### B. References

- [UPnP Device Architecture 1.0](http://upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v1.0.pdf)
- [Sonos Control API Documentation](https://developer.sonos.com/reference/control-api/)
- [tokio::sync::watch documentation](https://docs.rs/tokio/latest/tokio/sync/watch/index.html)
- [CLAUDE.md Project Instructions](/Users/tristantimblin/repos/sonos-sdk/CLAUDE.md)

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2026-01-14 | Claude Opus 4.5 | Initial specification created |
