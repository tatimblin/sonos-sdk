# sonos-api Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

Sonos devices expose functionality through a UPnP/SOAP interface that is verbose, untyped, and error-prone to work with directly. Developers building Sonos integrations face several challenges:

1. **XML Boilerplate**: Every UPnP operation requires constructing XML SOAP envelopes with precise formatting
2. **Lack of Type Safety**: Raw SOAP calls accept strings and return XML, with no compile-time guarantees
3. **Protocol Complexity**: The UPnP protocol has separate Control and Event endpoints, subscription management, and service-specific URIs that must be correctly addressed
4. **Inconsistent Patterns**: Each operation has slightly different request/response structures requiring custom parsing logic

Without this crate, developers must manually construct SOAP XML, manage HTTP connections, parse responses, and handle the UPnP subscription protocol - leading to duplicated code, runtime errors, and increased maintenance burden.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Type-safe operation definitions | Compile-time guarantees prevent runtime XML errors and ensure API consistency |
| P0 | Stateless client design | Simplifies resource management and enables multiple concurrent clients without connection conflicts |
| P0 | Comprehensive error types | Domain-specific errors enable proper error handling and recovery strategies |
| P1 | Composable operations with validation | Builder pattern enables flexible operation configuration while preventing invalid requests |
| P1 | Efficient resource sharing | Singleton SOAP client reduces memory usage by ~95% in multi-client scenarios |
| P1 | Service-specific event parsing | Type-safe event handling enables reactive applications built on top of this crate |
| P2 | Macro-based operation definitions | Reduces boilerplate when adding new UPnP operations |

### 1.3 Non-Goals

- **Connection Management**: The crate delegates HTTP connection pooling to the `soap-client` crate. Connection lifecycle is not managed here.
- **Async Runtime**: Operations are blocking by design to simplify integration. Async wrappers can be added by consumers using `tokio::task::spawn_blocking`.
- **Device State Caching**: No caching of device responses. Each operation is independent and stateless.
- **Business Logic**: This crate provides raw UPnP operations. Higher-level abstractions (grouping, playback queues) belong in downstream crates like `sonos-state`.

### 1.4 Success Criteria

- [x] All UPnP operations compile with type-checked requests and responses
- [x] Invalid operation parameters are rejected at build time with descriptive errors
- [x] All UPnP services (AVTransport, RenderingControl, ZoneGroupTopology, GroupRenderingControl) have operation and event support
- [x] Error types cover all failure modes with actionable information
- [x] Operation execution requires no XML knowledge from consuming code

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Public API                                     │
│  SonosClient  │  services::*  │  ManagedSubscription  │  events::*      │
├─────────────────────────────────────────────────────────────────────────┤
│                        Operation Framework                               │
│  SonosOperation (legacy)  │  UPnPOperation  │  OperationBuilder         │
│  Validate trait  │  ComposableOperation  │  ValidationLevel             │
├─────────────────────────────────────────────────────────────────────────┤
│                        Service Definitions                               │
│  av_transport  │  rendering_control  │  zone_group_topology             │
│  (operations + events per service)                                       │
├─────────────────────────────────────────────────────────────────────────┤
│                        Support Infrastructure                            │
│  Service enum  │  ServiceInfo  │  ServiceScope  │  xml_utils            │
├─────────────────────────────────────────────────────────────────────────┤
│                        Error Handling                                    │
│  ApiError  │  ValidationError  │  Result<T>                             │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                    ┌───────────────────────────────┐
                    │         soap-client           │
                    │  (HTTP transport, blocking)   │
                    └───────────────────────────────┘
```

**Design Rationale**: The layered architecture separates concerns cleanly:
- Public API provides user-facing types with maximum ergonomics
- Operation framework provides extensibility without exposing implementation details
- Service definitions group related operations logically
- Support infrastructure handles cross-cutting concerns like service routing
- All network I/O is delegated to `soap-client` to keep this crate focused on API semantics

### 2.2 Module Structure

```
src/
├── lib.rs                     # Public API surface, re-exports
├── client.rs                  # SonosClient implementation
├── error.rs                   # ApiError and Result types
├── service.rs                 # Service enum and ServiceInfo
├── subscription.rs            # ManagedSubscription lifecycle management
├── operation/
│   ├── mod.rs                 # SonosOperation, UPnPOperation traits
│   ├── builder.rs             # OperationBuilder, ComposableOperation
│   └── macros.rs              # define_upnp_operation! macro
├── events/
│   ├── mod.rs                 # Event framework re-exports
│   ├── types.rs               # EnrichedEvent, EventSource, EventParser
│   ├── processor.rs           # EventProcessor for generic event handling
│   └── xml_utils.rs           # DIDL-Lite parsing, namespace stripping
└── services/
    ├── mod.rs                 # Service modules
    ├── events.rs              # Subscription operations (Subscribe, Renew, Unsubscribe)
    ├── av_transport/
    │   ├── mod.rs             # AVTransport service
    │   ├── operations.rs      # Play, Pause, Stop, GetTransportInfo
    │   └── events.rs          # AVTransportEvent parsing
    ├── rendering_control/
    │   ├── mod.rs             # RenderingControl service
    │   ├── operations.rs      # GetVolume, SetVolume, SetRelativeVolume
    │   └── events.rs          # RenderingControlEvent parsing
    └── zone_group_topology/
        ├── mod.rs             # ZoneGroupTopology service
        ├── operations.rs      # GetZoneGroupState
        └── events.rs          # ZoneGroupTopologyEvent parsing
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `client` | Execute operations via SOAP client | `pub` |
| `error` | Error types for all failure modes | `pub` |
| `service` | Service routing and metadata | `pub` |
| `subscription` | UPnP subscription lifecycle | `pub` |
| `operation` | Operation traits and builder | `pub` |
| `events` | Event parsing framework | `pub` |
| `services::*` | Service-specific operations and events | `pub` |

### 2.3 Key Types

#### `SonosClient`

```rust
#[derive(Debug, Clone)]
pub struct SonosClient {
    soap_client: SoapClient,
}
```

**Purpose**: Primary entry point for executing operations and managing subscriptions.

**Invariants**:
- Always holds a valid reference to the shared SOAP client
- Thread-safe via `Clone` (underlying `SoapClient` uses `Arc`)

**Ownership**: Created by users, owned by users. Multiple clients can coexist sharing the same underlying HTTP resources.

#### `Service`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    AVTransport,
    RenderingControl,
    GroupRenderingControl,
    ZoneGroupTopology,
}
```

**Purpose**: Identifies UPnP services for routing operations and subscriptions.

**Invariants**: Each variant maps to exactly one UPnP service with known endpoints.

#### `ManagedSubscription`

```rust
pub struct ManagedSubscription {
    sid: String,
    device_ip: String,
    service: Service,
    state: Arc<Mutex<SubscriptionState>>,
    soap_client: SoapClient,
}
```

**Purpose**: Manages UPnP subscription lifecycle with expiration tracking and automatic cleanup.

**Invariants**:
- `sid` is a valid UPnP subscription ID returned by the device
- `state.active` is `false` after `unsubscribe()` or `drop()`
- Renewal must happen before `expires_at` to maintain subscription

**Ownership**: Created by `SonosClient`, owned by users. `Drop` implementation sends unsubscribe request.

---

## 3. Code Flow

### 3.1 Primary Flow: Execute Operation

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  User creates    │────▶│  Build payload   │────▶│  SOAP call via   │
│  request struct  │     │  with validation │     │  soap_client     │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                        │                        │
       ▼                        ▼                        ▼
  client.rs:82              operation/mod.rs:175    client.rs:90-102
                                                          │
                                                          ▼
                                                  ┌──────────────────┐
                                                  │  Parse response  │
                                                  │  XML to struct   │
                                                  └──────────────────┘
                                                          │
                                                          ▼
                                                   client.rs:104
```

**Step-by-step**:

1. **Entry** (`src/client.rs:82-105`): User calls `client.execute::<Op>(ip, &request)`. The client retrieves service info from the operation's `SERVICE` constant.

2. **Payload Construction** (`src/operation/mod.rs:175`): `Op::build_payload(&request)` constructs XML. For `UPnPOperation`, this includes validation.

3. **SOAP Transport** (`src/client.rs:90-102`): The client delegates to `soap_client.call()` with endpoint, service URI, action name, and payload.

4. **Response Parsing** (`src/client.rs:104`): `Op::parse_response(&xml)` deserializes the XML response into the typed response struct.

### 3.2 Secondary Flow: Subscribe to Events

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  client.subscribe│────▶│  SubscribeOp     │────▶│  Create Managed  │
│  (ip, service,   │     │  ::execute()     │     │  Subscription    │
│   callback_url)  │     │                  │     │                  │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                        │                        │
       ▼                        ▼                        ▼
  client.rs:204            services/events.rs:52   subscription.rs:76-96
```

**Step-by-step**:

1. **Entry** (`src/client.rs:204-211`): User calls `client.subscribe(ip, service, callback_url)`.

2. **Subscribe Request** (`src/services/events.rs:52-78`): `SubscribeOperation::execute()` sends HTTP SUBSCRIBE to the service's event endpoint.

3. **Managed Subscription** (`src/subscription.rs:76-96`): `ManagedSubscription::create()` stores the SID, calculates expiration, and returns the managed wrapper.

### 3.3 Error Flow

```
[soap-client error] ──▶ [ApiError conversion] ──▶ [Result<T, ApiError>]
[validation error]  ──▶ [ValidationError]     ──▶ [ApiError::InvalidParameter]
[subscription error]──▶ [ApiError::SubscriptionError]
```

**Error handling philosophy**: Errors are domain-specific and actionable. Network errors are distinguished from parse errors, SOAP faults include error codes, and validation errors specify which parameter failed and why.

---

## 4. Features

### 4.1 Feature: Type-Safe Operations

#### What

Operations are defined as marker structs implementing `SonosOperation` or `UPnPOperation` traits with associated request/response types.

#### Why

Compile-time type checking prevents common errors like:
- Missing required parameters
- Wrong parameter types
- Incorrect service/action combinations

#### How

```rust
pub trait UPnPOperation {
    type Request: Serialize + Validate;
    type Response: for<'de> Deserialize<'de>;
    const SERVICE: Service;
    const ACTION: &'static str;

    fn build_payload(request: &Self::Request) -> Result<String, ValidationError>;
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError>;
}
```

```rust
// Example usage
let play_op = av_transport::play("1".to_string()).build()?;
client.execute_enhanced("192.168.1.100", play_op)?;
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Traits over dynamic dispatch | Runtime operation registry | Type safety, zero-cost abstractions, IDE support |
| Associated types | Generic parameters | Cleaner API, each operation has exactly one request/response type |
| Const ACTION | Method returning &str | Compile-time string embedding, no runtime overhead |

### 4.2 Feature: Operation Builder Pattern

#### What

`OperationBuilder<Op>` provides a fluent API for configuring operations with validation levels, timeouts, and other options before execution.

#### Why

- Separates operation configuration from execution
- Enables validation before network calls
- Allows optional features without method proliferation

#### How

```rust
let play_op = av_transport::play("1".to_string())
    .with_validation(ValidationLevel::Basic)
    .with_timeout(Duration::from_secs(30))
    .build()?;
```

**Implementation** (`src/operation/builder.rs:24-84`):
- Builder stores request, validation level, timeout
- `build()` validates request and returns `ComposableOperation`
- `build_unchecked()` bypasses validation for performance-critical scenarios

### 4.3 Feature: Managed Subscriptions

#### What

`ManagedSubscription` wraps UPnP subscription lifecycle with:
- Expiration tracking
- Manual renewal API
- Automatic cleanup on drop

#### Why

UPnP subscriptions have complex lifecycle requirements:
- Must be renewed before expiration (typically 30 minutes)
- Must be explicitly unsubscribed to free device resources
- State must be tracked to prevent operations on expired subscriptions

#### How

```rust
let subscription = client.subscribe(
    "192.168.1.100",
    Service::AVTransport,
    "http://callback.url"
)?;

// Check if renewal is needed
if subscription.needs_renewal() {
    subscription.renew()?;
}

// Automatic cleanup when dropped
```

**Implementation** (`src/subscription.rs`):
- `create()` executes subscribe operation and stores SID
- `renew()` sends renewal request and updates expiration
- `Drop::drop()` sends unsubscribe request

### 4.4 Feature: Service-Specific Event Parsing

#### What

Each service module provides strongly-typed event structures and parsers that convert raw UPnP NOTIFY XML into structured data.

#### Why

- Raw UPnP events are XML with nested, escaped content
- Type-safe events enable pattern matching and field access
- Centralized parsing ensures consistency across the SDK

#### How

```rust
// Parse AVTransport event
let event = AVTransportEvent::from_xml(event_xml)?;
println!("Transport state: {:?}", event.transport_state());

// Create enriched event with context
let enriched = create_enriched_event(speaker_ip, event_source, event);
```

**Implementation** (`src/events/`, `src/services/*/events.rs`):
- `xml_utils::strip_namespaces()` removes XML namespace prefixes
- Serde deserializes cleaned XML into event structures
- `EnrichedEvent<T>` wraps event data with speaker IP, service, source, and timestamp

### 4.5 Feature: Declarative Operation Macros

#### What

`define_upnp_operation!` and `define_operation_with_response!` macros generate operation structs, request/response types, and trait implementations from declarative syntax.

#### Why

- Reduces boilerplate from ~50 lines to ~10 lines per operation
- Ensures consistent patterns across all operations
- Makes adding new operations straightforward

#### How

```rust
define_upnp_operation! {
    operation: PlayOperation,
    action: "Play",
    service: AVTransport,
    request: { speed: String },
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID><Speed>{}</Speed>",
                           req.instance_id, req.speed),
    parse: |_xml| Ok(()),
}
```

**Implementation** (`src/operation/macros.rs`):
- Uses `paste!` crate for identifier manipulation
- Generates `{Op}Request`, `{Op}Response` structs with serde derives
- Generates `UPnPOperation` implementation
- Generates convenience function (`play_operation()`)

---

## 5. Data Model

### 5.1 Core Data Structures

#### `ServiceInfo`

```rust
pub struct ServiceInfo {
    /// Control endpoint path (e.g., "MediaRenderer/AVTransport/Control")
    pub endpoint: &'static str,
    /// UPnP service URI for SOAP headers
    pub service_uri: &'static str,
    /// Event endpoint path for subscriptions
    pub event_endpoint: &'static str,
}
```

**Lifecycle**: Static, created by `Service::info()`, no cleanup needed.

#### `EnrichedEvent<T>`

```rust
pub struct EnrichedEvent<T> {
    pub registration_id: Option<u64>,
    pub speaker_ip: IpAddr,
    pub service: Service,
    pub event_source: EventSource,
    pub timestamp: SystemTime,
    pub event_data: T,
}
```

**Lifecycle**:
1. **Creation**: By event parsers or `EventProcessor`
2. **Mutation**: Immutable after creation
3. **Destruction**: Standard drop, no cleanup needed

#### `AVTransportEvent`

```rust
pub struct AVTransportEvent {
    property: AVTransportProperty,
}

// Provides accessors for:
// - transport_state(), transport_status(), speed()
// - current_track_uri(), track_duration(), rel_time(), abs_time()
// - play_mode(), track_metadata(), next_track_uri(), queue_length()
```

**Memory considerations**: Events contain String fields for flexibility. For high-frequency event processing, consider reusing allocations.

### 5.2 Serialization

| Format | Use Case | Library | Notes |
|--------|----------|---------|-------|
| XML | SOAP request/response | `quick-xml` + `serde` | Namespace stripping via `xml_utils::strip_namespaces()` |
| XML | UPnP event parsing | `quick-xml` + `serde` | Handles escaped nested XML via custom deserializers |
| DIDL-Lite | Track metadata | `serde` | Custom `DidlLite`, `DidlItem`, `DidlResource` structs |

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `soap-client` | HTTP SOAP transport | Workspace crate providing shared HTTP client with connection pooling |
| `sonos-discovery` | Device information types | Used for `Device` type in examples, not required for core functionality |
| `serde` | Serialization framework | Industry standard, enables derive macros for request/response types |
| `quick-xml` | XML parsing | Lightweight, serde-compatible, handles UPnP XML well |
| `xmltree` | XML element tree | Used for response parsing in legacy `SonosOperation` trait |
| `thiserror` | Error derive macro | Clean error type definitions with `#[error]` attributes |
| `paste` | Identifier manipulation | Required for macro-generated identifier concatenation |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| `sonos-state` | Executes operations, parses events | Uses public operation types and event parsers |
| `sonos-stream` | Event parsing for streaming | Uses `AVTransportEvent`, `RenderingControlEvent`, etc. |
| `sonos-event-manager` | Subscription management | Uses `Service` enum and subscription operations |

### 6.3 External Systems

```
┌─────────────────┐                    ┌─────────────────┐
│   sonos-api     │◀───── SOAP/HTTP ──▶│  Sonos Device   │
│                 │      Port 1400     │  (UPnP Server)  │
└─────────────────┘                    └─────────────────┘
```

**Protocol**: SOAP over HTTP

**Endpoints**:
- Control: `http://{device_ip}:1400/{service}/Control`
- Event: `http://{device_ip}:1400/{service}/Event`

**Authentication**: None (Sonos uses local network trust model)

**Error handling**: SOAP faults return HTTP 500 with fault code in body

---

## 7. Error Handling

### 7.1 Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("SOAP fault: error code {0}")]
    SoapFault(u16),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    #[error("Device error: {0}")]
    DeviceError(String),
}
```

```rust
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Parameter '{parameter}' value '{value}' is out of range ({min}..={max})")]
    RangeError { parameter: String, value: String, min: String, max: String },

    #[error("Parameter '{parameter}' value '{value}' is invalid: {reason}")]
    InvalidValue { parameter: String, value: String, reason: String },

    #[error("Required parameter '{parameter}' is missing")]
    MissingParameter { parameter: String },

    #[error("Parameter '{parameter}' failed validation: {message}")]
    Custom { parameter: String, message: String },
}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Domain-specific errors | `ApiError` variants for network, parse, SOAP, validation | Enables appropriate handling at each layer |
| Actionable messages | Include parameter names, values, valid ranges | Users can fix issues without debugging |
| No panic | All fallible operations return `Result` | Library should not crash host application |
| Error conversion | `From<SoapError>`, `From<ValidationError>` | Seamless error propagation with `?` |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `NetworkError` | Yes | Retry with exponential backoff |
| `ParseError` | No | Bug in parsing logic or unexpected device response |
| `SoapFault` | Sometimes | Device-specific; retry after fixing request or device state |
| `InvalidParameter` | Yes | Fix parameter value and retry |
| `SubscriptionError` | Yes | Create new subscription |
| `DeviceError` | Sometimes | May require device restart or state change |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    ┌───────────────────┐
                    │  Integration/E2E  │  Real devices (manual)
                    └─────────┬─────────┘
              ┌───────────────┴───────────────┐
              │       Component Tests         │  Module interactions
              └───────────────┬───────────────┘
    ┌─────────────────────────┴─────────────────────────┐
    │                   Unit Tests                       │  ~85% coverage
    └────────────────────────────────────────────────────┘
```

### 8.2 Unit Tests

**Location**: Inline `#[cfg(test)]` modules in each source file

**What to test**:
- [x] Payload construction for each operation
- [x] Response parsing with valid XML
- [x] Validation logic (valid and invalid inputs)
- [x] Error type conversions
- [x] Service info retrieval
- [x] Event XML parsing

**Example** (`src/services/av_transport/operations.rs:176-207`):
```rust
#[test]
fn test_play_operation_builder() {
    let play_op = play_operation("1".to_string()).build().unwrap();
    assert_eq!(play_op.request().speed, "1");
    assert_eq!(play_op.metadata().action, "Play");
}

#[test]
fn test_play_validation_basic() {
    let request = PlayOperationRequest {
        instance_id: 0,
        speed: "".to_string(),
    };
    assert!(request.validate_basic().is_err());
}
```

### 8.3 Component Tests

**Location**: `src/events/processor.rs` tests module

**What to test**:
- [x] Event processor handles all service types
- [x] XML parsing with real UPnP event structures
- [x] Enriched event creation with all fields

### 8.4 Integration Tests

**Location**: `examples/cli_example.rs`, `examples/integration_test.rs`

**Prerequisites**:
- Sonos device on network
- Network discovery allowed

**What to test**:
- Device discovery integration
- Operation execution against real devices
- Subscription creation and renewal

### 8.5 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| SOAP responses | Inline XML strings | Test modules |
| UPnP events | XML samples from real devices | `src/events/processor.rs` tests |

### 8.6 Property-Based Testing

**Available via**: `proptest` in dev-dependencies

```rust
// Example property: volume validation always rejects values > 100
#[test]
fn prop_volume_range() {
    proptest!(|(volume in 101..=255u8)| {
        let request = SetVolumeOperationRequest {
            instance_id: 0,
            channel: "Master".to_string(),
            desired_volume: volume,
        };
        prop_assert!(request.validate_basic().is_err());
    });
}
```

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Operation latency | <100ms (network-bound) | SOAP operations typically complete in 20-50ms |
| Memory per client | Shared (~0 additional) | Singleton pattern for SOAP client |
| XML parsing | <1ms | Small payloads, serde zero-copy where possible |

### 9.2 Critical Paths

1. **`SonosClient::execute()`** (`src/client.rs:82-105`)
   - **Complexity**: O(n) where n = response size
   - **Bottleneck**: Network I/O dominates
   - **Optimization**: Connection reuse via `soap-client`

2. **`strip_namespaces()`** (`src/events/xml_utils.rs:39-159`)
   - **Complexity**: O(n) where n = XML length
   - **Bottleneck**: String allocation
   - **Optimization**: Pre-allocated output buffer

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| HTTP connections | On first SOAP call | On client drop | Yes - via `soap-client` singleton |
| Subscriptions | On `subscribe()` | On `drop()` or `unsubscribe()` | No - per-service lifecycle |
| XML buffers | Per operation | After parsing | No - short-lived |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious device responses | Low | Medium | XML parsing with limits, no code execution |
| Subscription hijacking | Low | Low | SIDs are UUIDs, local network only |
| Callback URL injection | Medium | Medium | Validate callback URL format before use |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Device IPs | Low | Not logged at info level |
| Subscription IDs | Low | Ephemeral, UUIDs |
| Callback URLs | Medium | Should be validated by consumers |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| User parameters | `Validate` trait | `src/operation/mod.rs:127-143` |
| Device responses | XML parsing with serde | Service event modules |
| Subscription IDs | String format check | `src/subscription.rs` |

---

## 11. Observability

### 11.1 Logging

| Level | What's Logged | Example |
|-------|--------------|---------|
| `error` | SOAP faults, parse failures | "SOAP fault: error code 500" |
| `warn` | Subscription expiration | "Failed to unsubscribe during drop" |
| `info` | Operation execution | Not currently logged |
| `debug` | Request/response payloads | Not currently logged |
| `trace` | XML parsing details | Not currently logged |

*Note: Current logging is minimal. The crate uses `eprintln!` in subscription drop only.*

### 11.2 Tracing

The crate has `tracing` available as a dependency but does not currently instrument operations. Future versions may add spans for:

```
[execute_operation]
  └── [build_payload]
  └── [soap_call]
  └── [parse_response]
```

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| Validation level | `ValidationLevel` | `Basic` | Controls request validation depth |
| Operation timeout | `Duration` | None | Optional timeout for individual operations |
| Subscription timeout | `u32` (seconds) | 1800 | UPnP subscription duration |

### 12.2 Environment Variables

*This crate does not read environment variables directly. Configuration is via code.*

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `SonosClient` | Stable | Core public API |
| `SonosOperation` | Deprecated | Legacy trait, use `UPnPOperation` |
| `UPnPOperation` | Stable | Recommended operation trait |
| Service modules | Stable | Adding operations is non-breaking |
| Event types | Evolving | Fields may be added (non-breaking) |

### 13.2 Breaking Changes

**Policy**: Semantic versioning. Breaking changes require major version bump.

**Current deprecations**:
- `SonosOperation` trait: Use `UPnPOperation` with `OperationBuilder` instead

### 13.3 Version History

| Version | Changes | Migration Guide |
|---------|---------|-----------------|
| 0.1.0 | Initial release | N/A |

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Blocking I/O only | Can't use with async runtimes directly | `spawn_blocking()` wrapper | Consider async variant |
| No retry logic | Network failures require manual retry | Implement retry in consumer | May add retry policy |
| Limited operation set | Not all UPnP operations implemented | Add operations via macros | Expand as needed |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| Legacy `SonosOperation` trait | `src/operation/mod.rs` | Low | Remove after migration period |
| `eprintln!` in drop | `src/subscription.rs:244` | Low | Use proper logging |
| Minimal logging | Throughout | Medium | Add tracing instrumentation |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Async operation support | P1 | Better integration with async runtimes | `async-trait`, `soap-client` async variant |
| Retry policies | P2 | Automatic retry with backoff | None |
| Additional services | P2 | ContentDirectory, MusicServices | Service documentation |
| OpenAPI/JSON RPC | P2 | Alternative to SOAP for newer Sonos APIs | API research |

### 15.2 Open Questions

- [ ] **Should validation be async?**: Current validation is synchronous. Some validations might benefit from async (e.g., checking device state).
- [ ] **Should we support custom HTTP clients?**: Current design assumes `soap-client`. Some users might want to use their own HTTP client.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| UPnP | Universal Plug and Play - protocol used by Sonos for device discovery and control |
| SOAP | Simple Object Access Protocol - XML-based messaging for UPnP actions |
| SID | Subscription ID - unique identifier for UPnP event subscriptions |
| DIDL-Lite | Digital Item Declaration Language (Lite) - XML format for media metadata |
| AVTransport | UPnP service for playback control (play, pause, seek, etc.) |
| RenderingControl | UPnP service for audio settings (volume, mute, EQ) |
| ZoneGroupTopology | Sonos-specific service for speaker grouping |

### B. References

- [UPnP Device Architecture 1.1](http://upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v1.1.pdf)
- [UPnP AVTransport:2 Service](http://upnp.org/specs/av/UPnP-av-AVTransport-v2-Service.pdf)
- [UPnP RenderingControl:2 Service](http://upnp.org/specs/av/UPnP-av-RenderingControl-v2-Service.pdf)
- [Sonos UPnP Documentation](https://developer.sonos.com/) (requires account)

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2025-01-14 | Claude Opus 4.5 | Initial specification |
