# callback-server Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

UPnP devices like Sonos speakers communicate state changes through HTTP callbacks. When a speaker's volume changes, playback state updates, or track information changes, the device sends an HTTP NOTIFY request to a pre-registered callback URL. Applications that want real-time updates must:

1. Run an HTTP server that can receive incoming connections from devices on the local network
2. Parse UPnP-specific headers (SID, NT, NTS) to route events to the correct handlers
3. Manage subscription lifecycles (register/unregister callback handlers)
4. Handle firewall scenarios where devices cannot reach the callback server

Without this crate, each device-specific implementation would need to duplicate HTTP server setup, UPnP header validation, and event routing logic. This would lead to inconsistent implementations and tightly coupled business logic with transport concerns.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Generic, device-agnostic design | Enables reuse across different UPnP device types without modification |
| P0 | Reliable event delivery | Events must be routed correctly to registered handlers without loss |
| P0 | Unified event stream | Single HTTP endpoint handles all speakers and services efficiently |
| P1 | Automatic port selection | Simplifies deployment by finding available ports in a range |
| P1 | Graceful lifecycle management | Clean startup and shutdown without resource leaks |
| P2 | Firewall detection | Proactively identify when devices cannot reach the callback server |

### 1.3 Non-Goals

- **Device-specific parsing**: The crate delivers raw XML payloads; parsing Sonos-specific event formats is handled by consuming crates
- **Subscription creation**: This crate only receives events; creating UPnP subscriptions is handled by `sonos-api`
- **Event persistence**: Events are delivered via channels with no durability guarantees
- **Authentication**: UPnP events on local networks do not require authentication
- **HTTPS support**: UPnP callbacks use plain HTTP by specification

### 1.4 Success Criteria

- [x] HTTP server binds to available port within specified range
- [x] UPnP NOTIFY requests with valid headers are accepted and routed
- [x] Invalid requests (missing SID, wrong NT/NTS) are rejected with appropriate HTTP status codes
- [x] Multiple concurrent subscriptions are handled without interference
- [x] Server shuts down gracefully without dropping in-flight requests
- [x] Firewall status is detected per-device via event delivery monitoring

---

## 2. Architecture

### 2.1 High-Level Design

```
                                    ┌─────────────────────────────────────────┐
                                    │           CallbackServer                 │
                                    │  (HTTP server binding & lifecycle)       │
                                    ├─────────────────────────────────────────┤
    ┌──────────────┐                │                                         │
    │ Sonos Device │ ──NOTIFY──────▶│     warp HTTP Server (port 3400-3500)   │
    │ (Speaker)    │   HTTP POST    │                                         │
    └──────────────┘                └─────────────────┬───────────────────────┘
                                                      │
                                                      ▼
                                    ┌─────────────────────────────────────────┐
                                    │           EventRouter                    │
                                    │  (Subscription registry & routing)       │
                                    ├─────────────────────────────────────────┤
                                    │  HashSet<subscription_id>               │
                                    │  UnboundedSender<NotificationPayload>   │
                                    └─────────────────┬───────────────────────┘
                                                      │
                                                      ▼
                                    ┌─────────────────────────────────────────┐
                                    │      mpsc::UnboundedChannel              │
                                    │  (Async delivery to consumer)            │
                                    └─────────────────┬───────────────────────┘
                                                      │
                                                      ▼
                                    ┌─────────────────────────────────────────┐
                                    │         sonos-stream (consumer)          │
                                    │  (Adds device context, parses events)    │
                                    └─────────────────────────────────────────┘
```

**Design Rationale**: The architecture separates concerns into three distinct layers:

1. **HTTP Transport** (`CallbackServer`): Handles network binding, TLS would go here if needed, and HTTP protocol details. Uses warp for async HTTP handling.

2. **Event Routing** (`EventRouter`): Maintains subscription registry and forwards events. Decoupled from HTTP so it could theoretically be reused with other transports.

3. **Async Delivery** (mpsc channel): Provides backpressure-aware delivery to consumers. Using unbounded channels acknowledges that UPnP events are infrequent and small.

This layered approach allows the HTTP server to remain thin and the routing logic to be independently testable.

### 2.2 Module Structure

```
callback-server/
├── Cargo.toml              # Crate manifest
├── README.md               # Usage documentation
├── src/
│   ├── lib.rs              # Public API surface and module exports
│   ├── server.rs           # CallbackServer implementation
│   ├── router.rs           # EventRouter and NotificationPayload
│   └── firewall_detection.rs  # Per-device firewall detection coordinator
└── tests/
    ├── README.md           # Test documentation
    └── integration_tests.rs  # End-to-end HTTP tests
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `lib` | Re-exports public API, module documentation | `pub` |
| `server` | HTTP server lifecycle, port detection, IP discovery | `pub` (CallbackServer) |
| `router` | Subscription registry, event routing | `pub` |
| `firewall_detection` | Per-device firewall status monitoring | `pub` |

### 2.3 Key Types

#### `CallbackServer`

```rust
pub struct CallbackServer {
    port: u16,                                    // Bound port
    base_url: String,                             // Full callback URL (http://ip:port)
    event_router: Arc<EventRouter>,               // Shared router reference
    shutdown_tx: Option<mpsc::Sender<()>>,        // Graceful shutdown signal
    server_handle: Option<tokio::task::JoinHandle<()>>, // Background server task
}
```

**Purpose**: Manages the HTTP server lifecycle and provides the callback URL for subscription registration.

**Invariants**:
- After construction, `base_url` contains a valid HTTP URL reachable from the local network
- `shutdown_tx` and `server_handle` are `Some` until `shutdown()` is called
- The bound port is available and listening

**Ownership**: Created by the application, typically held for the duration of the program. Consumes `self` on shutdown.

#### `EventRouter`

```rust
pub struct EventRouter {
    subscriptions: Arc<RwLock<HashSet<String>>>,  // Active subscription IDs
    event_sender: mpsc::UnboundedSender<NotificationPayload>, // Output channel
}
```

**Purpose**: Routes incoming events to the unified event stream based on subscription registration.

**Invariants**:
- Only events for registered subscription IDs are forwarded
- Unregistered subscription IDs result in routing failure (returns false)
- Thread-safe for concurrent registration/routing

**Ownership**: Owned by `CallbackServer` via `Arc`, accessible to consumers for registration management.

#### `NotificationPayload`

```rust
pub struct NotificationPayload {
    pub subscription_id: String,  // UPnP SID header value
    pub event_xml: String,        // Raw XML event body
}
```

**Purpose**: Generic container for UPnP event data. Deliberately simple to avoid device-specific assumptions.

**Invariants**:
- `subscription_id` is never empty (validated by router before creation)
- `event_xml` contains the raw HTTP body (may be malformed XML; validation is consumer responsibility)

#### `FirewallDetectionCoordinator`

```rust
pub struct FirewallDetectionCoordinator {
    device_states: Arc<RwLock<HashMap<IpAddr, Arc<RwLock<DeviceFirewallState>>>>>,
    config: FirewallDetectionConfig,
    detection_complete_tx: mpsc::UnboundedSender<DetectionResult>,
    _timeout_task_handle: tokio::task::JoinHandle<()>,
}
```

**Purpose**: Monitors per-device event delivery to detect firewall blocking. Essential for enabling automatic fallback to polling in consuming crates.

**Invariants**:
- Each device IP has at most one active detection state
- Detection completes either via event receipt (Accessible) or timeout (Blocked)
- Background timeout monitor runs continuously until coordinator is dropped

---

## 3. Code Flow

### 3.1 Primary Flow: Receiving a UPnP Event

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ Sonos Device │────▶│  warp HTTP   │────▶│ EventRouter  │────▶│   Consumer   │
│ NOTIFY POST  │     │  Handler     │     │   .route()   │     │   Channel    │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
       │                    │                    │                    │
       ▼                    ▼                    ▼                    ▼
   HTTP Request      server.rs:262         router.rs:157        rx.recv()
```

**Step-by-step**:

1. **HTTP Reception** (`src/server.rs:262-337`): The warp filter receives an HTTP request. It extracts:
   - HTTP method (must be NOTIFY)
   - Path (any path accepted)
   - Headers: `SID`, `NT`, `NTS`
   - Body bytes

2. **Method Validation** (`src/server.rs:279-281`): Non-NOTIFY methods are rejected with 404.

3. **Header Validation** (`src/server.rs:311-314`, `src/server.rs:362-381`): UPnP headers are validated:
   - SID header must be present
   - If NT and NTS are present, they must be `upnp:event` and `upnp:propchange`

4. **Event Routing** (`src/router.rs:157-172`): The router checks if the subscription ID is registered:
   - If registered: creates `NotificationPayload` and sends to channel, returns true
   - If not registered: returns false, server responds with 404

5. **Channel Delivery** (`src/router.rs:167`): The payload is sent via `event_sender.send()`. Errors are ignored (receiver may have dropped).

6. **HTTP Response** (`src/server.rs:326-334`): Returns 200 OK on success, 404 if subscription not found.

### 3.2 Secondary Flow: Server Initialization

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   new()      │────▶│ find_port()  │────▶│ detect_ip()  │────▶│ start_server │
│   Entry      │     │ 3400-3500    │     │ UDP trick    │     │ warp::serve  │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
       │                    │                    │                    │
       ▼                    ▼                    ▼                    ▼
  server.rs:90         server.rs:227       server.rs:244       server.rs:254
```

**Step-by-step**:

1. **Port Discovery** (`src/server.rs:95-101`, `src/server.rs:227-238`): Iterates through port range, attempts TCP bind to find available port.

2. **IP Detection** (`src/server.rs:104-106`, `src/server.rs:244-251`): Creates UDP socket, "connects" to 8.8.8.8:80 (no data sent), reads local address from socket. This determines which interface would be used for outbound traffic.

3. **URL Construction** (`src/server.rs:108`): Combines IP and port into `http://ip:port` format.

4. **Server Spawn** (`src/server.rs:120-125`, `src/server.rs:254-356`): Spawns tokio task running warp server with graceful shutdown support.

5. **Ready Signal** (`src/server.rs:128-130`, `src/server.rs:353`): Server signals readiness via channel before `new()` returns, ensuring the server is actually listening.

### 3.3 Secondary Flow: Firewall Detection

```
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│ on_first_sub()   │────▶│ start_detection  │────▶│ monitor_timeouts │
│ Check cache      │     │ Create state     │     │ Background task  │
└──────────────────┘     └──────────────────┘     └──────────────────┘
       │                          │                        │
       │                          │                        │
       ▼                          ▼                        ▼
firewall_detection.rs:150   firewall_detection.rs:232   firewall_detection.rs:258

                    ┌──────────────────┐
                    │ on_event_received│
                    │ Mark accessible  │◀────── Event arrives
                    └──────────────────┘
                            │
                            ▼
                   firewall_detection.rs:183
```

**Step-by-step**:

1. **First Subscription** (`src/firewall_detection.rs:150-178`): When `sonos-stream` creates first subscription for a device:
   - Check cache for existing status
   - If cached and detection complete, return cached status
   - Otherwise, start new detection

2. **Start Detection** (`src/firewall_detection.rs:232-255`): Creates `DeviceFirewallState` with:
   - Current timestamp as subscription time
   - Status = Unknown
   - Configured timeout duration

3. **Background Monitoring** (`src/firewall_detection.rs:258-295`): Every 1 second, checks all incomplete detections:
   - If elapsed time >= timeout and no event received, mark as Blocked
   - Send `DetectionResult` to notification channel

4. **Event Reception** (`src/firewall_detection.rs:183-209`): When any event arrives:
   - If detection in progress for this device IP, mark as Accessible
   - Record first event time
   - Send `DetectionResult` notification

### 3.4 Error Flow

```
[Invalid HTTP headers] ──▶ [warp::reject::custom(InvalidUpnpHeaders)] ──▶ [400 Bad Request]
                                          │
                                          ▼
                                   handle_rejection (server.rs:393-411)

[Unknown subscription] ──▶ [router.route_event returns false] ──▶ [404 Not Found]
                                          │
                                          ▼
                                   warp::reject::not_found()

[Channel dropped]      ──▶ [event_sender.send() error ignored] ──▶ [No visible error]
```

**Error handling philosophy**: The callback server prioritizes reliability over strict error reporting. Invalid requests receive appropriate HTTP status codes, but channel send errors are silently ignored because:
1. The receiver dropping is a valid shutdown condition
2. UPnP devices don't retry on errors anyway
3. Logging provides sufficient observability

---

## 4. Features

### 4.1 Feature: Unified Event Stream

#### What

A single HTTP endpoint receives events from all Sonos speakers and all UPnP services, routing them to a unified channel based on subscription ID.

#### Why

Running separate HTTP servers per speaker or service would:
- Consume multiple ports (scarce resource, firewall complexity)
- Require complex coordination for IP detection
- Increase memory footprint with duplicate server infrastructure

The unified approach means one server handles all traffic, simplifying deployment and resource usage.

#### How

The `CallbackServer` accepts any path for NOTIFY requests (`src/server.rs:262-263`). The subscription ID from the SID header is the routing key, not the URL path. This allows the same callback URL to be registered for all subscriptions.

```rust
// All subscriptions use the same base URL
let callback_url = server.base_url(); // e.g., "http://192.168.1.50:3400"

// Router distinguishes events by SID header
server.router().register("uuid:speaker1-avtransport".to_string()).await;
server.router().register("uuid:speaker1-rendering".to_string()).await;
server.router().register("uuid:speaker2-avtransport".to_string()).await;
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Single endpoint, SID routing | Path-based routing (e.g., `/speaker1/avtransport`) | SID header is the canonical UPnP identifier; path is arbitrary |
| Unbounded channel | Bounded channel with backpressure | UPnP events are small and infrequent; backpressure would complicate API |

### 4.2 Feature: Automatic Port Selection

#### What

The server automatically finds an available port within a configurable range (default 3400-3500).

#### Why

Hard-coded ports fail when:
- Another application is using the port
- Multiple instances of the SDK run on the same machine
- Development environments with different network configurations

Automatic selection eliminates manual port configuration.

#### How

Sequential scan from start to end of range, attempting TCP bind on each (`src/server.rs:227-238`). First successful bind wins. The bound listener is immediately dropped (just testing availability), then warp binds to the same port.

```rust
fn find_available_port(start: u16, end: u16) -> Option<u16> {
    (start..=end).find(|&port| Self::is_port_available(port))
}

fn is_port_available(port: u16) -> bool {
    TcpListener::bind(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        port,
    ))
    .is_ok()
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Sequential scan | Random selection within range | Predictable behavior; random could theoretically never find port in pathological case |
| Small range (100 ports) | Large range or ephemeral | Keeps ports in expected range for firewall rules; 100 is plenty for typical use |

### 4.3 Feature: Per-Device Firewall Detection

#### What

Monitors event delivery on a per-device basis to detect whether the callback server is reachable from each Sonos speaker.

#### Why

Firewalls, NAT configurations, and network segmentation can block incoming HTTP connections from Sonos devices. Without detection:
- Applications would wait indefinitely for events that never arrive
- Users would have no indication why real-time updates aren't working

Per-device detection allows:
- Automatic fallback to polling for blocked devices
- Clear diagnostics for network troubleshooting
- Mixed environments where some devices work and others don't

#### How

The `FirewallDetectionCoordinator` tracks each device IP separately (`src/firewall_detection.rs:88-100`):

1. On first subscription for a device, start a detection timer
2. If any event arrives from that IP within timeout (default 15s), mark as Accessible
3. If timeout expires with no events, mark as Blocked
4. Cache results for reuse (configurable)

```rust
// Consumer (sonos-stream) integrates like this:
let status = coordinator.on_first_subscription(device_ip).await;
match status {
    FirewallStatus::Unknown => {
        // Detection in progress, will receive result via channel
    }
    FirewallStatus::Accessible => {
        // Use real-time events
    }
    FirewallStatus::Blocked => {
        // Fall back to polling
    }
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Event-based detection | Active probing (HTTP request to device) | Non-intrusive; works with UPnP flow naturally |
| Per-device tracking | Global firewall status | Networks may have asymmetric access; one device working doesn't mean all work |
| 15s default timeout | Shorter timeout | UPnP subscriptions can take time to propagate first event; 15s balances responsiveness with reliability |

### 4.4 Feature: UPnP Header Validation

#### What

Validates UPnP-specific headers (SID, NT, NTS) according to the UPnP Device Architecture specification.

#### Why

- Prevents processing of non-UPnP HTTP requests that might hit the callback endpoint
- Provides clear error responses for malformed requests
- Ensures the subscription ID is always present for routing

#### How

Validation in `validate_upnp_headers` (`src/server.rs:362-381`):

```rust
fn validate_upnp_headers(
    sid: &Option<String>,
    nt: &Option<String>,
    nts: &Option<String>,
) -> bool {
    // SID header is required for event notifications
    if sid.is_none() {
        return false;
    }

    // For UPnP events, NT and NTS headers are typically present
    // If present, validate they have expected values
    if let (Some(nt_val), Some(nts_val)) = (nt, nts) {
        if nt_val != "upnp:event" || nts_val != "upnp:propchange" {
            return false;
        }
    }

    true
}
```

**Key decisions**:
- SID is strictly required (without it, routing is impossible)
- NT/NTS are validated only if both are present (some devices omit them)
- Invalid NT/NTS values result in 400 Bad Request

---

## 5. Data Model

### 5.1 Core Data Structures

#### `NotificationPayload`

```rust
/// Generic notification payload for UPnP event notifications.
#[derive(Debug, Clone)]
pub struct NotificationPayload {
    /// The subscription ID from the UPnP SID header
    pub subscription_id: String,

    /// The raw XML event body
    pub event_xml: String,
}
```

**Lifecycle**:
1. **Creation**: Created in `EventRouter::route_event()` when a valid event arrives (`src/router.rs:161-164`)
2. **Mutation**: Immutable after creation (all fields are `pub` but typically consumed without modification)
3. **Destruction**: Dropped when consumer processes the event

**Memory considerations**: Typical payload is ~1-5KB (XML event body). Clone is explicit, not implicit, so memory is predictable.

#### `DeviceFirewallState`

```rust
#[derive(Debug, Clone)]
pub struct DeviceFirewallState {
    pub device_ip: IpAddr,
    pub status: FirewallStatus,
    pub first_subscription_time: SystemTime,
    pub first_event_time: Option<SystemTime>,
    pub detection_completed: bool,
    pub timeout_duration: Duration,
}
```

**Lifecycle**:
1. **Creation**: Created in `start_detection_for_device()` when first subscription triggers detection
2. **Mutation**: `status`, `first_event_time`, `detection_completed` updated when event arrives or timeout occurs
3. **Destruction**: Removed via `clear_device_cache()` or LRU eviction when cache is full

### 5.2 State Transitions

#### FirewallStatus State Machine

```
                     on_first_subscription()
                            │
                            ▼
                    ┌─────────────┐
                    │   Unknown   │
                    │ (detecting) │
                    └─────────────┘
                     │           │
    on_event_received()         timeout expires
                     │           │
                     ▼           ▼
              ┌──────────┐  ┌──────────┐
              │Accessible│  │ Blocked  │
              │ (cached) │  │ (cached) │
              └──────────┘  └──────────┘
                     │           │
    clear_device_cache()    clear_device_cache()
                     │           │
                     ▼           ▼
              ┌─────────────────────────┐
              │  Entry removed from     │
              │  device_states map      │
              └─────────────────────────┘
```

**Invariants per state**:
- **Unknown**: `detection_completed = false`, `first_event_time = None`
- **Accessible**: `detection_completed = true`, `first_event_time = Some(t)` where t <= first_subscription_time + timeout
- **Blocked**: `detection_completed = true`, `first_event_time = None`, elapsed >= timeout_duration

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `tokio` | Async runtime | Standard async runtime in Rust ecosystem; required for async HTTP server |
| `warp` | HTTP server framework | Lightweight, filter-based API that composes well; excellent for simple REST endpoints |
| `bytes` | Byte buffer handling | Required by warp for efficient body handling |
| `async-trait` | Async trait support | Enables async methods in traits (Rust limitation workaround) |
| `thiserror` | Error type derivation | Reduces boilerplate for error enum definitions |
| `reqwest` | HTTP client (dev) | Used in integration tests for sending test requests |
| `uuid` | UUID generation | Potentially for generating subscription IDs (currently unused in core) |
| `url` | URL parsing | URL validation and manipulation |
| `soap-client` | Internal workspace crate | Dependency exists but appears unused in current code |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| `sonos-stream` | Creates CallbackServer, registers subscriptions, receives NotificationPayload | Core API (CallbackServer, EventRouter, NotificationPayload) should be stable |

### 6.3 External Systems

```
┌─────────────────┐         ┌─────────────────┐
│ Sonos Speaker   │◀───────▶│  callback-server│
│ (UPnP Device)   │   HTTP  │                 │
└─────────────────┘  NOTIFY └─────────────────┘
```

**Protocol**: HTTP 1.1, NOTIFY method (UPnP extension)

**Headers**:
- `SID`: Subscription identifier (required)
- `NT`: Notification type, value `upnp:event` (optional but validated if present)
- `NTS`: Notification sub-type, value `upnp:propchange` (optional but validated if present)
- `Content-Type`: `text/xml`

**Body**: UPnP propertyset XML containing changed property values

**Error handling**: Invalid requests receive HTTP 400 or 404. Network errors on the device side are not our responsibility.

**Retry strategy**: None. UPnP devices do not expect or handle retry from callback servers.

---

## 7. Error Handling

### 7.1 Error Types

The crate uses string-based errors for simplicity:

```rust
// Server creation errors (returned from CallbackServer::new)
"No available port found in range {}-{}"  // Port exhaustion
"Failed to detect local IP address"       // Network detection failure
"Server failed to start"                  // Ready signal not received
```

HTTP-level errors use warp's rejection system:

```rust
#[derive(Debug)]
struct InvalidUpnpHeaders;
impl warp::reject::Reject for InvalidUpnpHeaders {}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Fail fast on startup | Port/IP detection errors abort server creation | Better to fail clearly than run in broken state |
| Graceful degradation at runtime | Channel send errors ignored | Receiver dropping is valid shutdown; no need to propagate |
| HTTP-appropriate responses | 400 for bad headers, 404 for unknown subscription | Standard HTTP semantics for API consumers |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| Port exhaustion | Yes | Widen port range or wait for ports to free |
| IP detection failure | Partial | May indicate no network; retry after network comes up |
| Invalid UPnP headers | Yes | Device issue; subsequent valid requests will succeed |
| Unknown subscription | Yes | Register subscription before events arrive |
| Channel send failure | N/A | Not an error condition; indicates shutdown |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    ┌───────────────────┐
                    │  Integration/E2E  │  Real HTTP server + reqwest client
                    └─────────┬─────────┘
              ┌───────────────┴───────────────┐
              │       Component Tests         │  Router + Server interactions
              └───────────────┬───────────────┘
    ┌─────────────────────────┴─────────────────────────┐
    │                   Unit Tests                       │  Pure function tests
    └────────────────────────────────────────────────────┘
```

The callback server emphasizes integration tests because the core value is HTTP handling, which is best tested with real network operations.

### 8.2 Unit Tests

**Location**: Inline `#[cfg(test)]` modules in each source file

**What to test**:
- [x] Port availability detection (`src/server.rs:417-428`)
- [x] Port range scanning (`src/server.rs:432-437`)
- [x] Local IP detection (`src/server.rs:440-448`)
- [x] UPnP header validation (`src/server.rs:453-488`)
- [x] Event router registration and routing (`src/router.rs:179-233`)
- [x] Firewall detection state transitions (`src/firewall_detection.rs:334-456`)

**Example** (from `src/router.rs:179-198`):
```rust
#[tokio::test]
async fn test_event_router_register_and_route() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let router = EventRouter::new(tx);

    let sub_id = "test-sub-123".to_string();

    // Register subscription
    router.register(sub_id.clone()).await;

    // Route an event
    let event_xml = "<event>test</event>".to_string();
    let routed = router.route_event(sub_id.clone(), event_xml.clone()).await;
    assert!(routed);

    // Verify payload was sent
    let payload = rx.recv().await.unwrap();
    assert_eq!(payload.subscription_id, sub_id);
    assert_eq!(payload.event_xml, event_xml);
}
```

### 8.3 Integration Tests

**Location**: `tests/integration_tests.rs`

**What to test**:
- [x] End-to-end event flow with real HTTP (`test_callback_server_end_to_end`)
- [x] Concurrent subscriptions (`test_multiple_subscriptions_concurrent_events`)
- [x] Dynamic registration/unregistration (`test_dynamic_subscription_management`)
- [x] Server URL and port detection (`test_server_ip_and_url_detection`)
- [x] Error handling for malformed requests (`test_error_handling`)

**Example** (from `tests/integration_tests.rs:12-130`):
```rust
#[tokio::test]
async fn test_callback_server_end_to_end() {
    let (tx, mut rx) = mpsc::unbounded_channel::<NotificationPayload>();
    let server = CallbackServer::new((50000, 50100), tx)
        .await
        .expect("Failed to create callback server");

    // Register a subscription
    let subscription_id = "test-subscription-123".to_string();
    server.router().register(format!("uuid:{}", subscription_id)).await;

    // Send a valid UPnP event notification
    let client = reqwest::Client::new();
    let response = client
        .request(reqwest::Method::from_bytes(b"NOTIFY").unwrap(), &notify_url)
        .header("SID", format!("uuid:{}", subscription_id))
        .header("NT", "upnp:event")
        .header("NTS", "upnp:propchange")
        .body(event_xml.to_string())
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert_eq!(response.status(), 200);
    // ... verify notification received
}
```

### 8.4 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| HTTP client | Real reqwest client | `tests/integration_tests.rs` |
| Sonos device | Simulated via reqwest NOTIFY | `tests/integration_tests.rs` |
| Network | Real localhost network | No mocking |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Event latency | < 10ms from HTTP receipt to channel delivery | Real-time UPnP events should feel instantaneous |
| Concurrent connections | 100+ simultaneous | Support many speakers and services |
| Memory per subscription | < 1KB | Subscription registry should be lightweight |

### 9.2 Critical Paths

1. **Event Routing** (`src/router.rs:157-172`)
   - **Complexity**: O(1) average for HashSet lookup
   - **Bottleneck**: RwLock acquisition for subscriptions set
   - **Optimization**: Using `read()` lock for routing; write lock only for register/unregister

2. **HTTP Handler** (`src/server.rs:262-337`)
   - **Complexity**: O(1) for header extraction and validation
   - **Bottleneck**: Body allocation (String::from_utf8_lossy)
   - **Optimization**: Acceptable for small XML payloads; could use zero-copy with more complexity

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| TCP port | On server creation | On shutdown | No - single port per server |
| Tokio tasks | Server task on creation, timeout task for firewall detection | Graceful shutdown signal | No - long-lived tasks |
| Channel buffers | Unbounded on creation | When receiver drops | No - grows as needed |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious HTTP requests | Medium | Low | Header validation rejects non-UPnP traffic |
| Subscription ID spoofing | Low | Medium | Events only routed to registered subscriptions |
| Denial of Service | Medium | Medium | Unbounded channel could grow; warp handles connection limits |
| XML entity expansion | Low | Medium | XML parsing is consumer responsibility, not ours |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Subscription IDs | Low | UUIDs are not secret; logged for debugging |
| Event XML | Low | Contains playback state, not credentials |
| Local IP address | Low | Necessary for callback URL; logged on startup |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| HTTP method | Must be NOTIFY | `src/server.rs:279-281` |
| SID header | Must be present | `src/server.rs:362-381` |
| NT header | If present, must be `upnp:event` | `src/server.rs:374-376` |
| NTS header | If present, must be `upnp:propchange` | `src/server.rs:374-376` |
| Event body | No validation (passed through) | Consumer responsibility |

---

## 11. Observability

### 11.1 Logging

The crate uses `eprintln!` for logging (should migrate to `tracing`):

| Level | What's Logged | Example |
|-------|--------------|---------|
| Info-equivalent | Server startup, event routing success | `"Unified CallbackServer listening on {addr}"` |
| Debug-equivalent | Incoming request details, headers | `"Method: {}, Path: {}, Body size: {} bytes"` |
| Error-equivalent | Invalid headers, routing failures, firewall detection results | `"Invalid UPnP headers"`, `"Firewall detection: No events from {} within timeout"` |

**Note**: Current logging uses emoji prefixes for visual distinction in development. Production use should migrate to structured logging via `tracing`.

### 11.2 Tracing

**Current state**: No formal tracing integration. Future enhancement should add:

```rust
#[tracing::instrument(skip(body))]
async fn handle_notify(
    sid: Option<String>,
    nt: Option<String>,
    nts: Option<String>,
    body: bytes::Bytes,
) -> Result<impl Reply, Rejection> {
    // ...
}
```

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `port_range` | `(u16, u16)` | `(3400, 3500)` | Range of ports to search for binding |

For firewall detection (`FirewallDetectionConfig`):

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `event_wait_timeout` | `Duration` | `15 seconds` | How long to wait for first event before marking as blocked |
| `enable_caching` | `bool` | `true` | Whether to cache per-device firewall status |
| `max_cached_devices` | `usize` | `100` | Maximum number of devices to track (LRU eviction) |

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `CallbackServer::new()` | Stable | Constructor signature established |
| `CallbackServer::base_url()` | Stable | Core functionality |
| `CallbackServer::router()` | Stable | Router access pattern established |
| `EventRouter::register/unregister` | Stable | Core functionality |
| `EventRouter::route_event` | Internal | Called by server, not typically by consumers |
| `NotificationPayload` | Stable | Simple struct, fields are public |
| `FirewallDetectionCoordinator` | Evolving | API may change as firewall detection matures |

### 13.2 Breaking Changes

**Policy**: As a private workspace crate, breaking changes are acceptable but should be coordinated with dependent crates (currently only `sonos-stream`).

**Current deprecations**: None

### 13.3 Version History

| Version | Changes | Migration Guide |
|---------|---------|-----------------|
| `0.1.0` | Initial implementation | N/A |

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| `eprintln!` logging | No log level control, not structured | Redirect stderr | Migrate to `tracing` |
| Single IP detection method | May fail on complex network setups | Manual callback URL override | Add fallback detection methods |
| Unbounded channel | Memory growth under sustained load | Acceptable for UPnP event rates | Consider bounded with overflow policy |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| Emoji in log messages | `src/server.rs`, `src/firewall_detection.rs` | Low | Replace with tracing spans/events |
| Unused `soap-client` dependency | `Cargo.toml` | Low | Remove if truly unused |
| String-based errors | `src/server.rs` | Low | Consider `thiserror` enum |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| `tracing` integration | P1 | Structured logging, spans for request tracing | None |
| Configurable callback URL | P2 | Support for Docker/NAT environments where auto-detection fails | None |
| Metrics export | P2 | Prometheus-compatible counters for events received, routing success rate | `metrics` crate |

### 15.2 Open Questions

- [ ] **Should firewall detection be moved to `sonos-stream`?** The coordinator is tightly coupled to the stream crate's needs. It lives here for proximity to the HTTP server, but could logically belong with event processing.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| SID | Subscription Identifier - UPnP header containing the unique ID for an event subscription |
| NOTIFY | HTTP method used by UPnP for event callbacks (extension to standard HTTP methods) |
| NT | Notification Type - UPnP header, value `upnp:event` for property change events |
| NTS | Notification Sub-Type - UPnP header, value `upnp:propchange` for property changes |
| Callback URL | The HTTP URL where UPnP devices send event notifications |

### B. References

- [UPnP Device Architecture 2.0](http://upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v2.0.pdf) - Section 4 (Eventing)
- [warp documentation](https://docs.rs/warp/) - HTTP server framework
- [tokio documentation](https://docs.rs/tokio/) - Async runtime

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2025-01-14 | Claude | Initial specification created |
