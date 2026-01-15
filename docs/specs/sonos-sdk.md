# sonos-sdk Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

The lower-level crates in the Sonos SDK workspace (sonos-api, sonos-state, sonos-discovery) provide powerful capabilities but require developers to understand and coordinate multiple subsystems to build applications. Developers must:

1. **Manually discover devices** using sonos-discovery and track their IPs
2. **Construct typed requests** for each UPnP operation
3. **Manage state separately** by integrating with sonos-state for reactive updates
4. **Coordinate API calls with state updates** when fetching fresh values

Without this crate, developers face a fragmented API surface where simple operations like "get the volume of a speaker" require understanding three different crates and their integration patterns. The sonos-sdk crate solves this by providing a unified, DOM-like API that feels natural to developers familiar with web development patterns.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | DOM-like property access pattern | Enable intuitive API like `speaker.volume.get()` that mirrors browser DOM patterns developers already know |
| P0 | Unified get/fetch/watch triad | Provide consistent access patterns: cached reads, API fetches, and reactive subscriptions through the same property handle |
| P1 | Transparent state management | Automatically synchronize API fetches with the reactive state system without manual coordination |
| P1 | Resource efficiency through sharing | Share StateManager and API client instances across all speakers to minimize resource usage |
| P2 | Extensible property model | Use macros to minimize boilerplate when adding new properties |

### 1.3 Non-Goals

- **Low-level UPnP control**: Direct SOAP operations should use sonos-api; this crate is for high-level access patterns
- **Custom subscription management**: Subscription lifecycle is handled automatically; manual control requires lower-level crates
- **Polling-based updates**: The `watch()` method uses UPnP events exclusively; polling is abstracted in sonos-stream
- **Speaker control actions**: This initial version focuses on reading properties; write operations (play, pause, set volume) are deferred

### 1.4 Success Criteria

- [x] Access speaker properties via `speaker.property.get()` syntax
- [x] Fresh API calls via `speaker.property.fetch().await` that update reactive state
- [x] UPnP event streaming via `speaker.property.watch().await`
- [x] Single entry point via `SonosSystem::new().await`
- [x] Automatic device discovery on initialization

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────────┐
│                         sonos-sdk (Public)                          │
├─────────────────────────────────────────────────────────────────────┤
│  SonosSystem                                                        │
│    ├── StateManager (shared Arc)                                    │
│    ├── SonosClient (shared)                                         │
│    └── HashMap<name, Speaker>                                       │
├─────────────────────────────────────────────────────────────────────┤
│  Speaker                                                            │
│    ├── id: SpeakerId                                                │
│    ├── name: String                                                 │
│    ├── ip: IpAddr                                                   │
│    ├── volume: VolumeHandle ──────────┐                             │
│    └── playback_state: PlaybackStateHandle ─┐                       │
├─────────────────────────────────────────────┼───────────────────────┤
│  PropertyHandle<P>                          │ (macro-generated)     │
│    ├── get()   → Option<P>          [cached value from StateStore]  │
│    ├── fetch() → Result<P>          [API call + state update]       │
│    └── watch() → PropertyWatcher<P> [UPnP event subscription]       │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    Internal Crate Dependencies                       │
├─────────────────────────────────────────────────────────────────────┤
│  sonos-state                                                        │
│    └── StateManager (reactive state + UPnP subscriptions)           │
├─────────────────────────────────────────────────────────────────────┤
│  sonos-api                                                          │
│    └── SonosClient (direct UPnP SOAP operations)                    │
├─────────────────────────────────────────────────────────────────────┤
│  sonos-discovery                                                    │
│    └── get() (SSDP device discovery)                                │
└─────────────────────────────────────────────────────────────────────┘
```

**Design Rationale**: The architecture follows the Facade pattern, providing a simplified interface to the complex subsystem of state management, API operations, and device discovery. The DOM-like property access pattern (`speaker.volume.get()`) was chosen because:

1. It is familiar to web developers who work with DOM properties
2. It groups related operations (get/fetch/watch) on a single handle
3. It enables IDE autocomplete to guide API discovery
4. It encapsulates the complexity of coordinating multiple crates

### 2.2 Module Structure

```
sonos-sdk/src/
├── lib.rs              # Public API surface, re-exports, module documentation
├── system.rs           # SonosSystem entry point with discovery and speaker registry
├── speaker.rs          # Speaker struct with property handles
├── error.rs            # SdkError enum
└── property/           # Property handle implementations
    ├── mod.rs          # Re-exports VolumeHandle, PlaybackStateHandle
    └── handles.rs      # Macro-generated property handles
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `system` | System initialization, discovery, speaker registry | `pub` (SonosSystem) |
| `speaker` | Speaker representation with property handles | `pub` (Speaker) |
| `error` | SDK-specific error types | `pub` (SdkError) |
| `property` | Property handle implementations | `pub` (handles only) |
| `property::handles` | Macro-generated handle types | `pub(crate)` (macro), `pub` (types) |

### 2.3 Key Types

#### `SonosSystem`

```rust
pub struct SonosSystem {
    state_manager: Arc<StateManager>,    // Shared reactive state management
    api_client: SonosClient,             // Shared SOAP client
    speakers: Arc<RwLock<HashMap<String, Speaker>>>,  // Name -> Speaker registry
}
```

**Purpose**: Main entry point that initializes the entire SDK, discovers devices, and provides access to speakers.

**Invariants**:
- After construction, all discovered speakers are registered in the map
- StateManager is initialized with all discovered devices
- The speakers map is never empty if discovery found devices

**Ownership**: Created once per application; owns the StateManager and speaker registry.

#### `Speaker`

```rust
pub struct Speaker {
    pub id: SpeakerId,                   // Unique speaker identifier
    pub name: String,                    // Human-readable name ("Living Room")
    pub ip: IpAddr,                      // Network address
    pub volume: VolumeHandle,            // Property handle for volume
    pub playback_state: PlaybackStateHandle,  // Property handle for playback
}
```

**Purpose**: Represents a single Sonos speaker with typed property handles for DOM-like access.

**Invariants**:
- All property handles share the same StateManager and SonosClient
- The speaker's IP address is valid and reachable at construction time

**Ownership**: Cloneable; contains Arc references to shared resources.

#### `VolumeHandle` / `PlaybackStateHandle` (macro-generated)

```rust
pub struct VolumeHandle {
    speaker_id: SpeakerId,
    speaker_ip: IpAddr,
    state_manager: Arc<StateManager>,
    api_client: SonosClient,
}
```

**Purpose**: Provides get/fetch/watch triad for a specific property type.

**Invariants**:
- The speaker_id and speaker_ip are always consistent
- All methods use the same shared resources

**Ownership**: Cloneable; references are Arc-cloned from Speaker.

---

## 3. Code Flow

### 3.1 Primary Flow: System Initialization

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  SonosSystem::   │────▶│  sonos-discovery │────▶│  StateManager::  │
│  new()           │     │  ::get()         │     │  add_devices()   │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                         │                        │
       │                         │                        │
       ▼                         ▼                        ▼
   system.rs:18            discovery crate          state_manager
                                                   registers devices
       │
       ▼
┌──────────────────┐
│  Create Speaker  │
│  instances with  │
│  property handles│
└──────────────────┘
       │
       ▼
   system.rs:29-41
```

**Step-by-step**:

1. **Entry** (`src/system.rs:18`): `SonosSystem::new()` is called, creating the async initialization context
2. **State initialization** (`src/system.rs:20`): Creates `StateManager::new().await` which initializes the reactive state system and event processing
3. **API client creation** (`src/system.rs:21`): Creates `SonosClient::new()` which uses shared SOAP client singleton
4. **Discovery** (`src/system.rs:24`): Calls `sonos_discovery::get()` to find all Sonos devices on the network via SSDP
5. **Device registration** (`src/system.rs:25`): Registers discovered devices with StateManager for reactive updates
6. **Speaker creation** (`src/system.rs:29-41`): Creates `Speaker` instances for each device with property handles
7. **Return** (`src/system.rs:44-48`): Returns the initialized `SonosSystem` with all speakers registered

### 3.2 Secondary Flow: Property Fetch (API Call + State Update)

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  speaker.volume  │────▶│  SonosClient::   │────▶│  StateManager::  │
│  .fetch()        │     │  execute_enhanced│     │  update_property │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                         │                        │
       ▼                         ▼                        ▼
   handles.rs:53           sonos-api              StateStore::set()
                                                  triggers watchers
```

**Step-by-step**:

1. **Entry** (`src/property/handles.rs:53`): `fetch()` is called on a property handle
2. **Operation construction** (`src/property/handles.rs:55-57`): Uses builder pattern to construct the UPnP operation
3. **API execution** (`src/property/handles.rs:60-62`): Executes operation via `SonosClient::execute_enhanced()`
4. **Response conversion** (`src/property/handles.rs:65`): Converts API response to property type using closure
5. **State update** (`src/property/handles.rs:68`): Updates StateManager, triggering any active watchers
6. **Return** (`src/property/handles.rs:70`): Returns the fresh property value

### 3.3 Secondary Flow: Property Watch (UPnP Event Subscription)

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  speaker.volume  │────▶│  StateManager::  │────▶│  PropertyWatcher │
│  .watch()        │     │  watch_property  │     │  returned        │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                         │                        │
       ▼                         ▼                        ▼
   handles.rs:74        ensures subscription      async event stream
                        via event manager
```

**Step-by-step**:

1. **Entry** (`src/property/handles.rs:74`): `watch()` is called on a property handle
2. **Delegation** (`src/property/handles.rs:75-78`): Calls `StateManager::watch_property()` with speaker ID
3. **Subscription management**: StateManager ensures UPnP subscription exists (reference counted)
4. **Watcher creation**: Returns `PropertyWatcher<P>` wrapping a `tokio::sync::watch` receiver
5. **Usage**: Caller uses `watcher.changed().await` and `watcher.current()` for reactive updates

### 3.4 Error Flow

```
[sonos-state::StateError] ──▶ SdkError::StateError ──▶ User
[sonos-api::ApiError]     ──▶ SdkError::ApiError   ──▶ User
[IP parse failure]        ──▶ SdkError::InvalidIpAddress ──▶ User
[Speaker not found]       ──▶ SdkError::SpeakerNotFound ──▶ User
```

**Error handling philosophy**: Errors are wrapped into `SdkError` to provide a unified error type at the SDK level while preserving the original error information through `#[from]` attributes. This allows users to:
1. Handle all SDK errors uniformly
2. Pattern match on specific error variants when needed
3. Access underlying errors for debugging

---

## 4. Features

### 4.1 Feature: DOM-like Property Access

#### What

Properties are accessed as fields on the Speaker struct, each providing `get()`, `fetch()`, and `watch()` methods.

#### Why

This pattern was chosen to:
1. Make the API discoverable through IDE autocomplete
2. Group related operations on a single handle
3. Provide a familiar pattern to web developers
4. Enable compile-time type safety for property access

#### How

```rust
// Get speaker and access properties directly
let speaker = system.get_speaker_by_name("Living Room").await?;

// Cached read (instant, no network)
let volume = speaker.volume.get();

// Fresh API call (network, updates cache)
let fresh_volume = speaker.volume.fetch().await?;

// Reactive subscription (UPnP events)
let mut watcher = speaker.volume.watch().await?;
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Property as struct field | Method like `speaker.get_property::<Volume>()` | Better discoverability and IDE support |
| Separate get/fetch/watch | Single method with enum parameter | Clearer intent and simpler types |
| Macro-generated handles | Manual implementation per property | Reduces boilerplate and ensures consistency |

### 4.2 Feature: Automatic State Synchronization

#### What

When `fetch()` is called, the fresh value is automatically pushed to the reactive state system, triggering any active watchers.

#### Why

Without automatic synchronization, developers would need to manually coordinate API calls with state updates, leading to:
1. Stale cache values after API calls
2. Watchers not receiving updates from API fetches
3. Inconsistent application state

#### How

```rust
pub async fn fetch(&self) -> Result<$property_type, SdkError> {
    // ... API call ...
    let property_value = $convert_expr(response);

    // Automatic state update - triggers watchers
    self.state_manager.update_property(&self.speaker_id, property_value.clone());

    Ok(property_value)
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Automatic update on fetch | Manual state update required | Prevents state inconsistency |
| Clone value before return | Return reference | Value types are small; cloning is simpler |

### 4.3 Feature: Macro-Generated Property Handles

#### What

Property handles are generated using a declarative macro that specifies the operation, request, and response conversion.

#### Why

Each property handle follows the same pattern with only the operation details differing. The macro:
1. Eliminates boilerplate code duplication
2. Ensures consistent implementation across all properties
3. Makes adding new properties straightforward
4. Reduces the chance of implementation errors

#### How

```rust
define_property_handle! {
    /// Handle for speaker volume (0-100)
    VolumeHandle for Volume {
        operation: GetVolumeOperation,
        request: rendering_control::get_volume_operation("Master".to_string()),
        convert_response: |response: GetVolumeResponse| Volume::new(response.current_volume),
    }
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Declarative macro | Derive macro or trait-based | Simpler, no proc-macro crate needed |
| Inline request builder | Separate builder struct | Less boilerplate per property |
| Closure for conversion | Trait method | More flexible, handles edge cases |

---

## 5. Data Model

### 5.1 Core Data Structures

#### `SdkError`

```rust
#[derive(Error, Debug)]
pub enum SdkError {
    /// Wraps errors from reactive state management
    #[error("State management error: {0}")]
    StateError(#[from] sonos_state::StateError),

    /// Wraps errors from API operations
    #[error("API error: {0}")]
    ApiError(#[from] sonos_api::ApiError),

    /// Speaker lookup failed
    #[error("Speaker not found: {0}")]
    SpeakerNotFound(String),

    /// IP address parsing failed
    #[error("Invalid IP address")]
    InvalidIpAddress,

    /// Property watcher channel closed
    #[error("Property watcher closed")]
    WatcherClosed,
}
```

**Lifecycle**:
1. **Creation**: Errors are created by `?` operator with `#[from]` conversions or explicitly
2. **Mutation**: Errors are immutable once created
3. **Destruction**: Standard drop semantics

**Memory considerations**: Error variants are small; String allocations only for error messages.

### 5.2 Re-exported Types

The SDK re-exports commonly used types from sonos-state to provide a complete API:

| Type | Source | Purpose |
|------|--------|---------|
| `Volume` | sonos-state | Represents speaker volume (0-100) |
| `PlaybackState` | sonos-state | Enum: Playing, Paused, Stopped, Transitioning |
| `PropertyWatcher<P>` | sonos-state | Async watcher for property changes |
| `SpeakerId` | sonos-state | Unique speaker identifier wrapper |

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `sonos-state` | Reactive state management, PropertyWatcher | Core reactive system; no alternative within workspace |
| `sonos-api` | Direct UPnP SOAP operations | Type-safe API operations; no alternative within workspace |
| `sonos-discovery` | SSDP device discovery | Automatic device finding; no alternative within workspace |
| `tokio` | Async runtime | Required for async/await; standard choice for Rust async |
| `thiserror` | Error type derivation | Ergonomic error definitions; workspace standard |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| End-user applications | Primary SDK entry point | API considered unstable (v0.1.0) |
| Examples (`basic_usage`) | Demonstrates API patterns | Used for documentation |

### 6.3 External Systems

```
┌─────────────────┐              ┌─────────────────┐
│   sonos-sdk     │◀────────────▶│  Sonos Speakers │
│   (via deps)    │   UPnP/SOAP  │  (on network)   │
└─────────────────┘              └─────────────────┘
```

**Protocol**: UPnP/SOAP over HTTP (port 1400) via sonos-api

**Authentication**: None (local network trust model)

**Error handling**: Network errors wrapped in `SdkError::ApiError`

**Retry strategy**: None at SDK level; applications should implement retries

---

## 7. Error Handling

### 7.1 Error Types

```rust
#[derive(Error, Debug)]
pub enum SdkError {
    #[error("State management error: {0}")]
    StateError(#[from] sonos_state::StateError),

    #[error("API error: {0}")]
    ApiError(#[from] sonos_api::ApiError),

    #[error("Speaker not found: {0}")]
    SpeakerNotFound(String),

    #[error("Invalid IP address")]
    InvalidIpAddress,

    #[error("Property watcher closed")]
    WatcherClosed,
}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Error wrapping | `#[from]` for upstream errors | Preserves original error info while providing unified type |
| Descriptive messages | Contextual error strings | Aids debugging without exposing internals |
| Typed variants | Enum with specific variants | Enables pattern matching for recovery logic |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `StateError` | Sometimes | May retry initialization; check underlying cause |
| `ApiError` | Yes | Retry operation with exponential backoff |
| `SpeakerNotFound` | Yes | Re-run discovery or check speaker name |
| `InvalidIpAddress` | No | Bug in discovery or device configuration |
| `WatcherClosed` | Yes | Create new watcher; subscription may have expired |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    ┌───────────────────┐
                    │  Integration/E2E  │  Requires real Sonos devices
                    └─────────┬─────────┘
              ┌───────────────┴───────────────┐
              │       Example Validation      │  basic_usage.rs
              └───────────────┬───────────────┘
    ┌─────────────────────────┴─────────────────────────┐
    │                   Unit Tests                       │  Macro expansion, type safety
    └────────────────────────────────────────────────────┘
```

### 8.2 Unit Tests

**Location**: Inline `#[cfg(test)]` modules (to be added)

**What to test**:
- [ ] Macro-generated handle types compile correctly
- [ ] Error type conversions work as expected
- [ ] Property type constraints are enforced

### 8.3 Integration Tests

**Location**: `examples/basic_usage.rs` (functional test)

**Prerequisites**:
- [ ] Sonos device(s) on the local network
- [ ] Network access to port 1400

**What to test**:
- [x] System initialization discovers devices
- [x] `get()` returns cached or None
- [x] `fetch()` retrieves fresh values
- [x] `watch()` creates valid PropertyWatcher

### 8.4 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| `sonos-discovery` | Mock device list | Not yet implemented |
| `sonos-api` | Mock SOAP responses | Not yet implemented |
| `sonos-state` | In-memory StateStore | Not yet implemented |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| `get()` latency | < 1ms | Cache read should be instant |
| `fetch()` latency | < 100ms | Network round-trip plus parsing |
| Memory per speaker | < 10KB | Handles are lightweight Arc wrappers |

### 9.2 Critical Paths

1. **Property get()** (`src/property/handles.rs:48-50`)
   - **Complexity**: O(1) - direct StateStore lookup
   - **Bottleneck**: None (memory read)
   - **Optimization**: Uses StateStore's optimized key-value storage

2. **Property fetch()** (`src/property/handles.rs:53-71`)
   - **Complexity**: O(1) network call
   - **Bottleneck**: Network latency to Sonos device
   - **Optimization**: Shared SOAP client with connection pooling

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| StateManager | System::new() | System drop | Yes - shared Arc |
| SonosClient | System::new() | System drop | Yes - shared singleton |
| PropertyWatcher | watch() call | Watcher drop | Reference counted subscriptions |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious device on network | Low | Medium | Trust local network; no auth on UPnP by design |
| SOAP response injection | Low | Low | XML parsing with strict schemas |
| Denial of service via discovery | Low | Low | Discovery is bounded by network timeout |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Speaker IPs | Low | Local network only |
| Speaker names | Low | User-configured values |
| Volume/playback state | Low | Non-sensitive device state |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| Discovery responses | IP address parsing | `src/system.rs:31` |
| API responses | XML schema validation | sonos-api crate |
| Speaker names | None (trusted from device) | N/A |

---

## 11. Observability

### 11.1 Logging

Currently minimal logging; relies on underlying crates:

| Level | What's Logged | Location |
|-------|--------------|---------|
| `error` | State/API failures | Via error propagation |
| `debug` | Event processing | sonos-state reactive.rs |
| `trace` | UPnP event details | sonos-stream |

### 11.2 Tracing

**Span structure** (inherited from sonos-state):
```
[state_manager]
  └── [event_processor]
      └── [property_update]
```

---

## 12. Configuration

### 12.1 Configuration Options

The SDK currently has no runtime configuration options. All settings are determined at compile time or from device discovery.

### 12.2 Environment Variables

None required.

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `SonosSystem::new()` | Unstable | May add configuration options |
| `speaker.volume.get()` | Stable | Core pattern, unlikely to change |
| `speaker.volume.fetch()` | Stable | Core pattern, unlikely to change |
| `speaker.volume.watch()` | Stable | Core pattern, unlikely to change |

### 13.2 Breaking Changes

**Policy**: Pre-1.0, breaking changes may occur in minor versions. Post-1.0, semantic versioning will be followed.

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
| Read-only properties | Cannot control playback | Use sonos-api directly | Future version |
| Two properties only | Limited functionality | Use sonos-state for more | Add as sonos-api operations available |
| No manual discovery | Cannot add speakers post-init | Recreate SonosSystem | Future version |
| Blocking discovery | Startup may be slow | Accept delay | Async discovery option |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| TODO comments for missing properties | `src/speaker.rs:17-23` | Low | Add as operations implemented |
| Missing unit tests | All modules | Medium | Add test coverage |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Write operations (set volume, play/pause) | P0 | Complete the API surface | sonos-api operations |
| Mute property | P1 | Commonly needed | sonos-api GetMute operation |
| Position/CurrentTrack properties | P1 | Media control use cases | sonos-api operations |
| Async discovery | P2 | Non-blocking initialization | sonos-discovery changes |
| Speaker groups | P2 | Multi-room audio control | sonos-state group support |

### 15.2 Open Questions

- [ ] **Should fetch() return old value on error?**: Currently returns error; could return cached value
- [ ] **Should property handles support set() operations?**: Waiting on write operation implementation pattern
- [ ] **Should SonosSystem support dynamic speaker addition?**: Requires rethinking initialization

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| Property Handle | Struct providing get/fetch/watch methods for a single property type |
| DOM-like API | API pattern where properties are accessed as fields, similar to browser DOM |
| PropertyWatcher | Async iterator for receiving property change events |
| StateManager | Central reactive state management component from sonos-state |
| UPnP | Universal Plug and Play protocol used by Sonos for device communication |

### B. References

- [CLAUDE.md](/Users/tristantimblin/repos/sonos-sdk/CLAUDE.md) - Project development guide
- [sonos-state specification](./sonos-state.md) - Reactive state management details
- [sonos-api specification](./sonos-api.md) - UPnP operation details

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2026-01-14 | Claude Opus 4.5 | Initial specification created |
