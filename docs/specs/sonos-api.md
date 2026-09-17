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
- [x] All UPnP services (AVTransport, RenderingControl, ZoneGroupTopology, GroupRenderingControl, GroupManagement) have operation and event support
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
│  UPnPOperation  │  OperationBuilder  │  ComposableOperation            │
│  Validate trait  │  ValidationLevel  │  SonosOperation (dead, see §14)  │
├─────────────────────────────────────────────────────────────────────────┤
│                        Service Definitions                               │
│  av_transport  │  rendering_control  │  zone_group_topology             │
│  group_rendering_control  │  group_management                            │
│  (operations + events + state per service)                               │
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
├── types.rs                   # SpeakerId, GroupId newtypes
├── operation/
│   ├── mod.rs                 # UPnPOperation, Validate, ValidationLevel,
│   │                          #   response helpers, SonosOperation (dead)
│   ├── builder.rs             # OperationBuilder, ComposableOperation
│   └── macros.rs              # define_upnp_operation!,
│                              #   define_operation_with_response!
├── events/
│   ├── mod.rs                 # Event framework re-exports
│   ├── types.rs               # EnrichedEvent, EventSource, EventParser
│   ├── processor.rs           # EventProcessor for generic event handling
│   └── xml_utils.rs           # quick-xml/serde helpers, DIDL-Lite structs
└── services/
    ├── mod.rs                 # Service modules
    ├── events.rs              # Subscription operations (Subscribe, Renew, Unsubscribe)
    ├── av_transport/          # 30 operations
    │   ├── mod.rs             # AVTransport service
    │   ├── operations.rs      # Play, Pause, Stop, Seek, queue, alarms, ...
    │   ├── events.rs          # AVTransportEvent parsing
    │   └── state.rs           # AVTransportState + poll()
    ├── rendering_control/     # 11 operations
    │   ├── mod.rs
    │   ├── operations.rs      # GetVolume, SetVolume, mute, bass, treble, loudness
    │   ├── events.rs
    │   └── state.rs
    ├── group_rendering_control/   # 6 operations
    │   ├── mod.rs
    │   ├── operations.rs      # group volume / mute / snapshot
    │   ├── events.rs
    │   └── state.rs
    ├── group_management/      # 4 operations
    │   ├── mod.rs             # SERVICE + subscribe helpers live here, not in operations.rs
    │   ├── operations.rs      # AddMember, RemoveMember, ...
    │   ├── events.rs
    │   └── state.rs           # no poll(): the service is action-only
    └── zone_group_topology/   # 1 operation
        ├── mod.rs
        ├── operations.rs      # GetZoneGroupState
        ├── events.rs
        └── state.rs
```

Every service directory has the same four-file shape: `mod.rs`, `operations.rs`, `events.rs`,
`state.rs`. There are exactly five of them.

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
// src/service.rs:6 — five variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    AVTransport,            // :8
    RenderingControl,       // :11
    GroupRenderingControl,  // :14
    ZoneGroupTopology,      // :17
    GroupManagement,        // :20
}
```

**Purpose**: identifies UPnP services for routing operations and subscriptions.

**Invariants**: each variant maps to exactly one UPnP service with known endpoints.

**Methods**: `name()` (`:52`), `info()` (`:66`, returning `ServiceInfo { endpoint, service_uri,
event_endpoint }`, `:25`), and `scope()` (`:101`, returning `ServiceScope`, `:38`). The scope
mapping is what downstream crates use to decide whether a subscription is per-speaker
(`RenderingControl`), per-network (`ZoneGroupTopology`) or per-coordinator (`AVTransport`,
`GroupRenderingControl`, `GroupManagement`).

There is no `Display` impl, no `FromStr`, and no `all()` iterator.

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
│  Convenience fn  │────▶│  .build()        │────▶│ execute_enhanced │
│  -> Builder      │     │  validates       │     │                  │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                        │                        │
       ▼                        ▼                        ▼
  macros.rs:74             builder.rs:72           client.rs:123
                                                          │
                                        ┌─────────────────┴────────────────┐
                                        ▼                                  ▼
                              ┌──────────────────┐             ┌──────────────────┐
                              │ build_payload    │             │  soap_client     │
                              │ (revalidates)    │             │  .call()         │
                              └──────────────────┘             └──────────────────┘
                                 builder.rs:153                    client.rs:146
                                                                          │
                                                                          ▼
                                                                 ┌──────────────────┐
                                                                 │  parse_response  │
                                                                 │  XML to struct   │
                                                                 └──────────────────┘
                                                                    builder.rs:164
```

**Step-by-step**:

1. **Construct** (`src/operation/macros.rs:74`): a macro-generated convenience function — e.g.
   `av_transport::play_operation(..)` — returns an `OperationBuilder<Op>`.

2. **Build** (`src/operation/builder.rs:72`): `.build()` runs `Validate::validate` at the
   configured `ValidationLevel` and freezes the result into a `ComposableOperation<Op>`.
   `build_unchecked()` (`:92`) skips that check and sets the level to `None`.

3. **Entry** (`src/client.rs:123`): the caller invokes
   `client.execute_enhanced::<Op>(ip, operation)`. This is the only execution path real call
   sites use — for example `sonos-sdk/src/speaker.rs:287`,
   `sonos-sdk/src/property/handles.rs:585`, and each service's `state::poll` such as
   `sonos-api/src/services/rendering_control/state.rs:55`.

4. **Payload Construction** (`src/operation/builder.rs:153`): `ComposableOperation::build_payload()`
   calls `Op::build_payload(&request)`, which the macros generate to revalidate at
   `ValidationLevel::Basic` before emitting XML.

5. **SOAP Transport** (`src/client.rs:146`): the client resolves `Op::SERVICE.info()` and
   delegates to `soap_client.call()` with endpoint, service URI, action name, and payload.

6. **Response Parsing** (`src/operation/builder.rs:164`): `parse_response(&xml)` forwards to
   `Op::parse_response`, deserializing the raw envelope text into the typed response struct.

### 3.2 Secondary Flow: Subscribe to Events

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  client.subscribe│────▶│  SubscribeOp     │────▶│  Create Managed  │
│  (ip, service,   │     │  ::execute()     │     │  Subscription    │
│   callback_url)  │     │                  │     │                  │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                        │                        │
       ▼                        ▼                        ▼
  client.rs:191            services/events.rs:52   subscription.rs:72-99
```

**Step-by-step**:

1. **Entry** (`src/client.rs:191`): the caller invokes `client.subscribe(ip, service, callback_url)`,
   which hard-codes a 1800-second timeout and delegates to `create_managed_subscription()`
   (`:259`). `subscribe_with_timeout()` (`:212`) is the variant that takes the timeout.

2. **Subscribe Request** (`src/services/events.rs:52`): `SubscribeOperation::execute()` sends
   HTTP SUBSCRIBE to the service's event endpoint. This and its siblings
   (`UnsubscribeOperation::execute`, `:106`; `RenewOperation::execute`, `:161`) take a raw
   `&SoapClient` and implement neither operation trait — the UPnP subscription verbs are not
   SOAP actions.

3. **Managed Subscription** (`src/subscription.rs:72`): `ManagedSubscription::create()` — which
   is `pub(crate)`, so the client is the only way in — stores the SID, calculates expiration,
   and returns the managed wrapper.

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

Operations are unit structs implementing `UPnPOperation` (`src/operation/mod.rs:153`) with
associated request and response types. 52 operations implement it across the five services:
47 generated by `define_upnp_operation!` / `define_operation_with_response!`
(`src/operation/macros.rs:27`, `:140`) and 5 hand-written where the response needs parsing the
macros cannot express — `AddURIToQueueOperation`
(`src/services/av_transport/operations.rs:460`), `GetMuteOperation`
(`src/services/rendering_control/operations.rs:127`), `GetLoudnessOperation` (`:329`),
`GetGroupMuteOperation` (`src/services/group_rendering_control/operations.rs:129`) and
`AddMemberOperation` (`src/services/group_management/operations.rs:50`).

#### Why

Compile-time type checking prevents common errors like:
- Missing required parameters
- Wrong parameter types
- Incorrect service/action combinations

#### How

```rust
// src/operation/mod.rs:153
pub trait UPnPOperation {
    type Request: Serialize + Validate;              // :155
    type Response: for<'de> Deserialize<'de>;        // :158
    const SERVICE: Service;                          // :161
    const ACTION: &'static str;                      // :164

    fn build_payload(request: &Self::Request) -> Result<String, ValidationError>;  // :176
    fn parse_response(xml: &str) -> Result<Self::Response, ApiError>;              // :188

    // Provided
    fn dependencies() -> &'static [&'static str] { &[] }        // :197
    fn can_batch_with<T: UPnPOperation>() -> bool { true }      // :211
    fn metadata() -> OperationMetadata { /* ... */ }            // :218
}
```

Two required methods; the three provided ones describe an operation rather than executing it.
The macros do not generate a `Validate` impl (`src/operation/macros.rs:47`, `:240`), so each
operation module hand-writes its own — which is where per-parameter range and enum checks live.

A safety rail sits in the no-mapping macro arm: it emits
`assert_derivable_arg_name(stringify!($field))` (`src/operation/macros.rs:266`), a `const fn`
(`src/operation/mod.rs:380`) that makes a multi-word request field a **compile error** unless
`request_xml_mapping:` is supplied.

`parse_response` receives the **raw response body as text**, not a parsed DOM.
`soap-client` deliberately returns `String`: it owns transport and SOAP-fault detection,
so an `Ok` body is guaranteed to be a fault-free `<{action}Response>` envelope, but
response *shape* is service-specific and is this crate's business. That split is what
lets `soap-client` stay free of any per-service knowledge and lets this crate use one
XML library (`quick-xml`) end to end.

```rust
// Example usage
let play_op = av_transport::play_operation("1".to_string()).build()?;
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
let play_op = av_transport::play_operation("1".to_string())
    .with_validation(ValidationLevel::Basic)
    .with_timeout(Duration::from_secs(30))
    .build()?;
```

**Implementation** (`src/operation/builder.rs:17-115`):
- `OperationBuilder<Op>` (`:17`) stores request, validation level, and timeout
- `build()` (`:72`) validates the request and returns a `ComposableOperation<Op>` (`:120`)
- `build_unchecked()` (`:92`) bypasses that validation and sets the stored level to
  `ValidationLevel::None`; `build_payload()` still revalidates at execute time

`ValidationLevel` (`src/operation/mod.rs:119`) has exactly two variants: `None` and `Basic`
(the `#[default]`).

Despite the "composable" naming and the trait's `dependencies()` / `can_batch_with()` hooks,
there is no chaining, batching or sequencing API — no `and_then`, no `Batch`, no `Sequence`.
Those hooks describe operations; nothing consumes them yet.

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

**Implementation** (`src/subscription.rs:47`):
- `create()` (`:72`, `pub(crate)`) executes the subscribe operation and stores the SID
- `renew()` (`:183`) sends the renewal request and updates the expiration
- `unsubscribe()` (`:220`) ends it explicitly; `Drop` (`:237`) does the same on a best-effort
  basis

Both `renew()` and `unsubscribe()` take `&self`, not `&mut self` — the mutable state lives
behind an `Arc<Mutex<SubscriptionState>>` (`:55`), so a subscription can be renewed from a
shared handle. The read-only accessors are `subscription_id()` (`:122`), `is_active()` (`:127`),
`needs_renewal()` (`:136`), `time_until_renewal()` (`:144`) and `expires_at()` (`:167`).

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
- `quick_xml::de::from_str` deserializes the raw NOTIFY body straight into the event struct
- `xml_utils` supplies the shared pieces: `parse()` (deserialize + wrap the error as
  `ApiError::ParseError`), `deserialize_nested` / `deserialize_zone_group_state` for the
  escaped-XML-inside-an-element pattern, `ValueAttribute` for `<Foo val="..."/>`,
  `NestedAttribute<T>` for a `val` attribute holding escaped XML, and the `DidlLite` /
  `DidlItem` / `DidlResource` structs
- `EnrichedEvent<T>` wraps event data with speaker IP, service, source, and timestamp

#### Why there is no namespace preprocessing

UPnP event XML is heavily namespaced (`e:propertyset`, `e:property`, `dc:title`,
`upnp:albumArtURI`), and it is tempting to strip prefixes with a hand-written tokenizer
before deserializing. That step is **redundant**: quick-xml's serde deserializer matches on
the element's *local* name, so `<e:property>` deserializes as `property` with no
preprocessing at all.

It is also unsafe to attempt. A tokenizer that treats `<!...>` as "copy until the first `>`"
truncates any CDATA section, comment or DOCTYPE internal subset containing a `>` — a
`<dc:title><![CDATA[3 > 2]]></dc:title>` corrupts the whole body. Letting the real parser
handle namespaces avoids that class of bug entirely.

**Consequence for anyone adding an event type**: write `#[serde(rename = "...")]` values
*without* prefixes. `rename = "dc:title"` cannot match anything. There is a regression
test for the CDATA/comment case in `src/events/xml_utils.rs`.

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

#### Request element names must be explicit

UPnP argument names come from each device's SCPD, not from a naming convention. Their
casing is **not recoverable from snake_case field names**:

| Request field | Correct UPnP element | Naive first-char capitalization |
|---------------|---------------------|---------------------------------|
| `object_id` | `ObjectID` | `Object_id` |
| `update_id` | `UpdateID` | `Update_id` |
| `starting_index` | `StartingIndex` | `Starting_index` |
| `number_of_tracks` | `NumberOfTracks` | `Number_of_tracks` |
| `enqueued_uri` | `EnqueuedURI` | `Enqueued_uri` |
| `enqueued_uri_meta_data` | `EnqueuedURIMetaData` | `Enqueued_uri_meta_data` |
| `channel` | `Channel` | `Channel` (correct — single word) |

`define_operation_with_response!` therefore accepts an optional `request_xml_mapping:`
block that mirrors the existing `xml_mapping:` idiom used for response fields:

```rust
define_operation_with_response! {
    operation: SaveQueueOperation,
    action: "SaveQueue",
    service: AVTransport,
    request: { title: String, object_id: String },
    response: SaveQueueResponse { assigned_object_id: String },
    request_xml_mapping: {
        title: "Title",
        object_id: "ObjectID",
    },
    xml_mapping: { assigned_object_id: "AssignedObjectID" },
}
```

**Why optional**: the macro is `#[macro_export]` and has ~16 call sites, all of which
predate this block. Making it an additional macro arm keeps the change additive and
non-breaking.

**Why safe**: the block-less arm now asserts at compile time
(`operation::assert_derivable_arg_name`) that every request field is a single word.
A multi-word field without a `request_xml_mapping:` entry is a build failure rather
than a malformed request discovered against a device. When the block is present it
must list every request field — the generated `build_payload` destructures the request
struct exhaustively, so an omission also fails to compile. The block's order
determines argument order in the SOAP body.

**Escaping**: both macro arms route every argument value through `xml_escape`.
Hand-written `UPnPOperation` impls must call it explicitly; see §10.3.

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

#### `RenderingControlEvent`

```rust
pub struct RenderingControlEvent {
    property: RenderingControlProperty,
}

// Provides accessors for:
// - master_volume(), lf_volume(), rf_volume()
// - master_mute(), lf_mute(), rf_mute()
// - bass(), treble(), loudness(), balance()
// - other_channels()
```

**Per-channel state variables**: In UPnP RenderingControl every state variable in the
`LastChange` document is scoped to a channel (`Master`, `LF`, `RF`, ...). A device may
emit the same variable several times in one event — stereo pairs and home theater setups
routinely report `Loudness` and `Bass` per channel. Every such variable is therefore
modelled as a `Vec` of channel/value pairs rather than as a single value.

**Why this matters**: a single-value model makes serde reject the event with
`duplicate field`, and because `from_xml()` returns `Err` the *entire* event is discarded
by `sonos-stream`'s event processor. Any unrelated volume or mute change riding in the
same event is lost with it, so volume and mute silently stop updating on any device that
reports per-channel EQ. Collections keep the event parseable so unaffected fields survive.

**Master selection semantics**: accessors return the value for the `Master` channel via
exact match, mirroring `get_volume_for_channel()`. When no `Master` entry is present the
accessor returns `None` rather than falling back to whichever channel came first —
reporting an `LF` value as though it were the master value would misrepresent the
device's state. An element with **no** `channel` attribute (the scalar form
`<Bass val="2"/>`, which some devices emit) is treated as the master value for
backwards compatibility.

**Stability**: the accessors keep their `Option<String>` signatures, and
`RenderingControlInstance` is private, so this modelling is not a public API change.

### 5.2 Serialization

| Format | Use Case | Library | Notes |
|--------|----------|---------|-------|
| XML | SOAP request/response | `quick-xml` + `serde` | `parse_response(&str)` reads the raw body; matching is on element *local* names, so no namespace preprocessing |
| XML | UPnP event parsing | `quick-xml` + `serde` | Handles escaped nested XML via custom deserializers (`deserialize_nested`) |
| XML | Outbound payload escaping | `quick_xml::escape::escape` | `operation::xml_escape` delegates; see §10.3 |
| DIDL-Lite | Track metadata | `serde` | Custom `DidlLite`, `DidlItem`, `DidlResource` structs |

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `soap-client` | HTTP SOAP transport | Workspace crate providing shared HTTP client with connection pooling |
| `sonos-discovery` | Device information types | Used for `Device` type in examples, not required for core functionality |
| `serde` | Serialization framework | Industry standard, enables derive macros for request/response types |
| `quick-xml` | XML parsing **and** escaping | Lightweight, serde-compatible, handles UPnP XML well. The single XML dependency: it deserializes responses and events, and `escape::escape` backs `operation::xml_escape` |
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

**Location**: inline `#[cfg(test)]` modules in each source file — 218 tests in total, and no
`tests/` directory. Heaviest: `src/services/rendering_control/operations.rs` (36),
`src/services/av_transport/operations.rs` (32),
`src/services/group_rendering_control/operations.rs` (23),
`src/services/group_management/operations.rs` (16), `src/operation/mod.rs` (13).

**What to test**:
- [x] Payload construction for each operation
- [x] Response parsing with valid XML
- [x] Validation logic (valid and invalid inputs)
- [x] Error type conversions
- [x] Service info retrieval
- [x] Event XML parsing

**Example** (`src/services/av_transport/operations.rs:802-822`):
```rust
#[test]
fn test_play_operation_builder() {
    let op = play_operation("1".to_string()).build().unwrap();
    assert_eq!(op.request().speed, "1");
    assert_eq!(op.metadata().action, "Play");
}

#[test]
fn test_play_validation() {
    let request = PlayOperationRequest {
        instance_id: 0,
        speed: "".to_string(),
    };
    assert!(request.validate_basic().is_err());

    let request = PlayOperationRequest {
        instance_id: 0,
        speed: "1".to_string(),
    };
    assert!(request.validate_basic().is_ok());
}
```

### 8.3 Component Tests

**Location**: `src/events/processor.rs` tests module

**What to test**:
- [x] Event processor handles all service types
- [x] XML parsing with real UPnP event structures
- [x] Enriched event creation with all fields

### 8.4 Integration Tests

**Location**: `examples/cli_example.rs`

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

**Available via**: `proptest` in dev-dependencies. It is used in exactly one service today —
`group_management` — across three `proptest!` blocks, each configured with
`ProptestConfig::with_cases(100)`:

| Property | Location |
|----------|----------|
| `prop_event_group_coordinator_is_local_parsing` | `src/services/group_management/events.rs:406` |
| `prop_add_member_bool_parsing` | `src/services/group_management/operations.rs:366` |
| `prop_remove_member_validation_passes` | `src/services/group_management/operations.rs:413` |

```rust
// src/services/group_management/operations.rs:413
proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_remove_member_validation_passes(member_id in member_id_strategy()) {
        let request = RemoveMemberOperationRequest { instance_id: 0, member_id };
        prop_assert!(request.validate_basic().is_ok());
    }
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

1. **`SonosClient::execute_enhanced()`** (`src/client.rs:123`)
   - **Complexity**: O(n) where n = response size
   - **Bottleneck**: network I/O dominates
   - **Optimization**: connection reuse via the `soap-client` singleton

2. **`operation::response_text()`** (`src/operation/mod.rs:252`)
   - **Complexity**: O(n) where n = XML length
   - **Bottleneck**: Nothing measurable; UPnP response bodies are small
   - **Design**: a single streaming pass with `quick_xml::Reader`, matching on the
     argument's local name at any depth. No intermediate DOM and no whole-document copy.
     This path is *not* where the time goes, so it is optimized for correctness: nested
     markup is depth-tracked so its end tag cannot terminate the search early, and only
     direct text children are collected.

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
| User parameters | `Validate` trait | `src/operation/mod.rs:128-142` |
| Channel arguments | `validate_channel`: `Master`, `LF` or `RF` only | `src/operation/mod.rs:396` |
| User parameters | `xml_escape` before SOAP interpolation | `src/operation/mod.rs:351` |
| Device responses | XML parsing with serde | Service event modules |
| Subscription IDs | String format check | `src/subscription.rs` |

#### SOAP payload escaping

Every string interpolated into a SOAP payload must pass through
`operation::xml_escape`. Unescaped values are both a correctness and an injection
problem:

- Streaming URIs routinely contain `&` (`x-sonosapi-stream:s2846?sid=254&flags=32`),
  which produces malformed XML that the device rejects.
- `*MetaData` arguments carry DIDL-Lite, which always contains `<`, `>`, and `&`.
- A crafted value can close its element early and inject sibling arguments.

`operation::xml_escape` is a thin wrapper over `quick_xml::escape::escape`, which escapes
all five XML predefined entities — `<`, `>`, `&`, `'` and `"`. Delegating rather than
hand-rolling the character loop means the escaping cannot drift out of step with the parser
that has to read the result back, and it is one less place to get the `&`-first ordering
right. The `test_xml_escape` unit test asserts all five characters, which is what keeps the
guarantee if quick-xml is ever upgraded.

Both `define_operation_with_response!` arms and the `request_xml_mapping:` path escape
automatically. **Only hand-written `UPnPOperation` impls can omit escaping**, so those
require review when added. Current hand-written impls and their status:

| Impl | String arguments | Escaped |
|------|-----------------|---------|
| `AddURIToQueueOperation` | `enqueued_uri`, `enqueued_uri_meta_data` | Yes |
| `AddMemberOperation` (GroupManagement) | `member_id` | Yes |
| `GetMuteOperation` (RenderingControl) | `channel` | No — constrained to `Master`/`LF`/`RF` by `validate_channel` |
| `GetLoudnessOperation` (RenderingControl) | `channel` | No — same constraint |
| `GetGroupMuteOperation` | none | N/A |

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

*Note: this crate does not depend on `tracing` and emits no log records. The only diagnostic
output is an `eprintln!` in `ManagedSubscription::drop` (`src/subscription.rs:244`). Everything
in the table above describes what a consumer would log, not what this crate emits.*

### 11.2 Tracing

`tracing` is **not** a dependency of this crate, so there is no instrumentation. Adding it
would mean taking the dependency first. The natural span structure would be:

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
| Validation level | `ValidationLevel` | `Basic` | Controls request validation depth. Only `None` and `Basic` exist |
| Operation timeout | `Option<Duration>` | `None` | Set via `OperationBuilder::with_timeout`. See §14.1 — it is checked before the SOAP call and so has no effect on it |
| Subscription timeout | `u32` (seconds) | 1800 | UPnP subscription duration. `subscribe()` hard-codes it; `subscribe_with_timeout()` takes it |

### 12.2 Environment Variables

*This crate does not read environment variables directly. Configuration is via code.*

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `SonosClient::execute_enhanced` | Stable | The execution path every call site uses |
| `SonosClient::execute` | Dead | Bounded on `SonosOperation`, which nothing implements, so it cannot be called |
| `UPnPOperation` | Stable | The operation trait |
| `SonosOperation` | Dead | Zero implementors workspace-wide (§14.2) |
| `ApiError` | Stable | **Not** `#[non_exhaustive]`, so adding a variant is breaking |
| Service modules | Stable | Adding operations is non-breaking |
| Event types | Evolving | Fields may be added (non-breaking) |

### 13.2 Breaking Changes

**Policy**: Semantic versioning. Breaking changes require major version bump.

**Current deprecations**: none carry a `#[deprecated]` attribute. `SonosOperation` and
`SonosClient::execute` are dead rather than deprecated — see §14.2.

### 13.3 Version

Published as `sonos-api`, versioned from the workspace (`version.workspace = true`), so it
moves in lockstep with `sonos-sdk`. A per-crate `CHANGELOG.md` sits beside this manifest.

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Blocking I/O only | Cannot be used from an async runtime directly | `spawn_blocking()` wrapper, as `sonos-stream` does | Consider an async variant |
| No retry logic | Network failures require manual retry | Implement retry in the consumer | May add a retry policy |
| Limited operation set | Not every UPnP operation is implemented; there is no DeviceProperties or ContentDirectory service | Add operations via the macros | Expand as needed |
| `OperationBuilder::with_timeout` has no effect | A per-operation timeout is checked before the SOAP call (`src/client.rs:139-143`), where no time has elapsed, so it never fires | Rely on `soap-client`'s fixed 5s connect / 10s read timeouts | Thread the timeout into the transport |
| `execute_enhanced` reports a validation failure as `ApiError::ParseError` | A bad argument looks like a malformed response | Match on the message | Use the `From<ValidationError>` impl (`src/error.rs:82`), which yields `InvalidParameter` |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| `SonosOperation` trait has zero implementors, and `SonosClient::execute` is bounded on it | `src/operation/mod.rs:30`, `src/client.rs:80` | Medium | Delete both; nothing can be calling them |
| `EventParserRegistry` / `EventParser` / `EventParserDyn` have no in-crate consumer — `EventProcessor` dispatches on a hardcoded `match` | `src/events/types.rs:105-180`, `src/events/processor.rs:89` | Low | Adopt the registry in `EventProcessor`, or retire it |
| `EventProcessorStats` is never incremented; `EventProcessor` is a unit struct with no state | `src/events/processor.rs:11`, `:151` | Low | Give the processor state, or move the counters to the caller that owns them |
| `ComposableOperation` is not re-exported from `lib.rs` despite appearing in `execute_enhanced`'s signature | `src/lib.rs:181-183` | Low | Add it to the re-export list |
| `group_management` puts `SERVICE` and the subscribe helpers in `mod.rs`, unlike the other four services | `src/services/group_management/mod.rs:47-67` | Low | Move them to `operations.rs` for consistency |
| `eprintln!` in `Drop` | `src/subscription.rs:244` | Low | Use proper logging |
| No logging at all: `tracing` is not a dependency | Throughout | Medium | Take the dependency and instrument `execute_enhanced` |

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
