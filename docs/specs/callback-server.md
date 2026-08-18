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
| P0 | Bounded resource use under hostile input | The endpoint is unauthenticated and LAN-reachable; input validation is the only defence, so every buffer must have a ceiling |
| P1 | Automatic port selection | Simplifies deployment by finding available ports in a range |
| P1 | Graceful lifecycle management | Clean startup and shutdown without resource leaks |
| P2 | Firewall detection | Proactively identify when devices cannot reach the callback server |

### 1.3 Non-Goals

- **Device-specific parsing**: The crate delivers raw XML payloads; parsing Sonos-specific event formats is handled by consuming crates
- **Subscription creation**: This crate only receives events; creating UPnP subscriptions is handled by `sonos-api`
- **Event persistence**: Events are delivered via channels with no durability guarantees
- **Authentication**: UPnP eventing has no authentication mechanism, so the endpoint cannot authenticate callers. This is a deliberate non-goal, not an oversight — but it makes input validation and resource bounding the only available defence (see §10)
- **HTTPS support**: UPnP callbacks use plain HTTP by specification

### 1.4 Success Criteria

- [x] HTTP server binds to available port within specified range
- [x] UPnP NOTIFY requests with valid headers are accepted and routed
- [x] Invalid requests (missing SID, wrong NT/NTS) are rejected with appropriate HTTP status codes
- [x] Multiple concurrent subscriptions are handled without interference
- [x] Server shuts down gracefully without dropping in-flight requests
- [x] Firewall status is detected per-device via event delivery monitoring
- [x] Oversized NOTIFY bodies are rejected before being buffered in memory
- [x] The pending-event buffer cannot grow without bound from unrecognised SIDs
- [x] Non-ASCII event payloads never panic the request handler

---

## 2. Architecture

### 2.1 High-Level Design

```
                                    ┌─────────────────────────────────────────┐
                                    │           CallbackServer                 │
                                    │  (HTTP server binding & lifecycle)       │
                                    ├─────────────────────────────────────────┤
    ┌──────────────┐                │                                         │
    │ Sonos Device │ ──NOTIFY──────▶│     axum HTTP Server (port 3400-3500)   │
    │ (Speaker)    │   HTTP POST    │     body capped at 64 KiB (→413/411)    │
    └──────────────┘                └─────────────────┬───────────────────────┘
                                                      │
                                                      ▼
                                    ┌─────────────────────────────────────────┐
                                    │           EventRouter                    │
                                    │  (Subscription registry & routing)       │
                                    ├─────────────────────────────────────────┤
                                    │  HashSet<subscription_id>               │
                                    │  pending buffer (≤256, TTL 5s)          │
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

1. **HTTP Transport** (`CallbackServer`): Handles network binding, TLS would go here if needed, and HTTP protocol details. Uses `axum` for async HTTP handling.

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
    state: Arc<RwLock<RouterState>>,              // Subscriptions + pending buffer
    event_sender: mpsc::UnboundedSender<NotificationPayload>, // Output channel
}

struct RouterState {
    subscriptions: HashSet<String>,               // Active subscription IDs
    pending: Vec<(String, String, Instant)>,      // Capped at MAX_PENDING_EVENTS
}
```

Both fields live behind a single lock so registration and routing cannot interleave
into a TOCTOU gap between "is this SID registered?" and "buffer it".

**Purpose**: Routes incoming events to the unified event stream based on subscription registration.

**Invariants**:
- Events for registered subscription IDs are forwarded immediately
- Events for unregistered subscription IDs are buffered and replayed when `register()` is called
- Buffered events expire after 5 seconds (`BUFFER_TTL`), swept on both `route_event()` and `register()`
- The pending buffer never exceeds `MAX_PENDING_EVENTS` (256); on overflow the oldest entry is dropped
- `unregister()` drains buffered events to prevent stale replays
- Thread-safe for concurrent registration/routing

**Ownership**: Owned by `CallbackServer` via `Arc`, accessible to consumers for registration management.

#### `NotificationPayload`

```rust
pub struct NotificationPayload {
    pub subscription_id: String,  // UPnP SID header value
    pub event_xml: String,        // Raw XML event body
    pub received_at: Instant,     // Monotonic arrival instant
}
```

**Purpose**: Generic container for UPnP event data. Deliberately simple to avoid device-specific assumptions.

**Invariants**:
- `subscription_id` is never empty (validated by router before creation)
- `event_xml` contains the raw HTTP body (may be malformed XML; validation is consumer responsibility)
- `received_at` is the monotonic instant the notification arrived, taken before the router's own
  lock. Consumers order state writes by it (see `sonos-state` spec §4.1a), so it is an `Instant`
  and not a `SystemTime`: a wall-clock ordering would invert under NTP correction. A buffered
  event replayed on late SID registration carries its **original** arrival instant, not the
  replay instant

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
│ Sonos Device │────▶│  axum HTTP   │────▶│ EventRouter  │────▶│   Consumer   │
│ NOTIFY POST  │     │  Handler     │     │   .route()   │     │   Channel    │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
       │                    │                    │                    │
       ▼                    ▼                    ▼                    ▼
   HTTP Request      NotifyRequest         route_event()        rx.recv()
                     + handle_notify
```

Every gate lives in one `FromRequest` extractor, `NotifyRequest`. Expressing them as an
extractor rather than as checks inside the handler means they run before the handler body
and each failure carries its own status via `IntoResponse` — no separate error-to-status
mapping table to keep in step. The extractor also makes the handler's own signature the
proof that validation happened: `handle_notify` cannot be reached with an unvalidated
request.

**Step-by-step**:

1. **Method Validation**: `req.method().as_str() != NOTIFY_METHOD` rejects non-NOTIFY with
   404. This gate deliberately runs **before** anything looks at the body, so ordinary
   bodiless requests (e.g. a browser GET) still get 404 rather than 411 Length Required.
   NOTIFY is not a standard verb, so it is absent from `http::Method`'s constants and
   axum's `MethodFilter` cannot express it; comparing `as_str()` against a `&'static str`
   constant is the whole gate and constructs no `Method` per request.

2. **Body Size Limit**, enforced in **two independent places**:
   - `declared_content_length()` refuses on the declared `Content-Length` before a single
     body byte is read (413), and turns a request with no declared length into 411.
   - `DefaultBodyLimit::max(MAX_NOTIFY_BODY_BYTES)` then caps the bytes actually read, so
     the ceiling holds even if the header check is later changed or bypassed.

   See §10.4 for sizing rationale.

3. **HTTP Reception**: the extractor reads:
   - HTTP method (must be NOTIFY)
   - Path (any path accepted, for logging only)
   - Headers: `SID`, `NT`, `NTS`, via `upnp_header()` — a present-but-not-UTF-8 value is a
     400, not the 500 it used to fall through to
   - Body bytes (size-capped)

4. **Header Validation** (`validate_upnp_headers`): SID must be present, since without it
   an event cannot be routed. NT and NTS are optional (some devices omit them) but each is
   checked **independently** when present:

   ```rust
   if nt.is_some_and(|nt| nt != "upnp:event") { return false; }
   if nts.is_some_and(|nts| nts != "upnp:propchange") { return false; }
   ```

   **This fixed a real validation gap.** The previous form was
   `if let (Some(nt), Some(nts)) = (nt, nts)`, which validated the pair only when *both*
   were present — so a request carrying `NT: garbage` and no `NTS` at all skipped
   validation entirely and was accepted. Independent checks preserve the "optional"
   intent while closing that hole.

   Header validation runs **before** the body is decoded to a `String`, so junk requests never pay for the full allocation. This ordering matters: decoding first would let an invalid request cost a full body copy on top of the buffered bytes.

5. **Trace Preview**: If trace logging is enabled, the body is previewed via `preview(&event_xml, TRACE_PREVIEW_BYTES)`, which snaps the truncation point back to a UTF-8 char boundary. A naive `&s[..200]` panics when byte 200 falls inside a multi-byte codepoint — reachable with any non-Latin track title.

6. **Event Routing** (`src/router.rs`): The router checks if the subscription ID is registered:
   - If registered: creates `NotificationPayload` and sends to channel immediately
   - If not registered: buffers event for replay when `register()` is called, sweeping stale entries and enforcing `MAX_PENDING_EVENTS`

7. **Channel Delivery**: The payload is sent via `event_sender.send()`. Errors are ignored (receiver may have dropped).

8. **HTTP Response**: Always returns 200 OK for valid, size-conformant NOTIFY requests. Events are either routed immediately or buffered for replay — returning 404 could cause speakers to cancel subscriptions.

### 3.2 Secondary Flow: Server Initialization

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   new()      │────▶│ find_port()  │────▶│ detect_ip()  │────▶│ start_server │
│   Entry      │     │ 3400-3500    │     │ enumerate    │     │ axum::serve  │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
       │                    │                    │                    │
       ▼                    ▼                    ▼                    ▼
  new()            find_available_port  detect_local_ip     start_server
                                        (usable_interfaces
                                         + preferred_local_ip)
```

**Step-by-step**:

1. **Port Discovery** (`find_available_port`/`is_port_available`): Iterates through port range, attempts TCP bind to find available port.

2. **IP Detection** (`detect_local_ip`): Enumerates IPv4 interfaces via `if-addrs` and picks one with `preferred_local_ip`. See §4.5 for why this is not a route probe and why the interface netmask matters.

3. **URL Construction** (`new`): Combines IP and port into `http://ip:port` format.

4. **Server Spawn** (`start_server`): Spawns a tokio task running `axum::serve(listener, app).with_graceful_shutdown(..)`. The router is a single `.fallback(handle_notify)` — `fallback` is what makes the path arbitrary, since the SID header rather than the URL is the routing key.

5. **Ready Signal** (`ready_tx`/`ready_rx`): Server signals readiness via channel before `new()` returns, ensuring the server is actually listening. The signal is sent immediately after `TcpListener::bind` returns, because the socket is accepting from that point — so a caller may subscribe knowing events have somewhere to land. If `bind` fails, `ready_tx` is dropped instead, and `new()` reports "Server failed to start" rather than handing back a server nothing can reach.

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

Every rejection is a `NotifyRejection` variant with its own `IntoResponse`, so the status
lives next to the reason rather than in a central mapping function:

```
[Not a NOTIFY request] ──▶ [NotifyRejection::NotNotify]          ──▶ [404 Not Found]

[Invalid/missing SID, bad NT or NTS,
 non-UTF-8 header value]
                       ──▶ [NotifyRejection::InvalidUpnpHeaders] ──▶ [400 Bad Request]

[Content-Length > 64 KiB]
                       ──▶ [NotifyRejection::BodyTooLarge]       ──▶ [413 Payload Too Large]

[No parseable Content-Length]
                       ──▶ [NotifyRejection::LengthRequired]     ──▶ [411 Length Required]

[Read hit the limit, or connection
 failed mid-body]      ──▶ [NotifyRejection::UnreadableBody]     ──▶ [413 or 400, axum's own mapping]

[Unknown subscription] ──▶ [router.route_event buffers event] ──▶ [200 OK]
                                          │
                                          ▼
                                   Buffered for replay on register()

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

The `CallbackServer` accepts any path for NOTIFY requests: the router is a single
`Router::new().fallback(handle_notify)`, and `fallback` is what makes the path arbitrary.
The subscription ID from the SID header is the routing key, not the URL path. This allows the same callback URL to be registered for all subscriptions.

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

Sequential scan from start to end of range, attempting TCP bind on each (`is_port_available`). First successful bind wins. The bound listener is immediately dropped (just testing availability), then axum binds to the same port.

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

Validation in `validate_upnp_headers`:

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

### 4.5 Feature: Per-Interface Callback Address Selection

#### What

The advertised callback address is chosen by enumerating the host's IPv4
interfaces and, where a target speaker is known, selecting the interface whose
subnet **actually contains** that speaker — using the interface's real netmask.

| Function | Role |
|----------|------|
| `usable_interfaces()` | Enumerate IPv4 interfaces, excluding loopback, link-local, unspecified |
| `select_interface(&[(addr, netmask)], target)` | Pure: pick the interface containing `target`, most-specific prefix first |
| `preferred_local_ip(&[(addr, netmask)])` | Target-less fallback used to build `base_url`; deprioritises CGNAT |
| `CallbackServer::local_ip_for_speaker(ip)` | Public wrapper over `select_interface` for reachability diagnostics |

#### Why

`detect_local_ip` previously bound a UDP socket, "connected" it to `8.8.8.8:80`
(sending nothing) and read back the local address. That reports whichever
interface wins the **default route** — which is not the interface that reaches a
LAN speaker. With a VPN up the default route is the tunnel, so the callback URL
advertised a tunnel address no speaker could reach. Speakers accepted the
SUBSCRIBE and then had nowhere to deliver to, so **every event was silently lost
and the firewall detector attributed it to a firewall**, which is a plausible but
wrong diagnosis that sends the device to polling forever.

The same probe existed a second time in `sonos-stream::broker`, which rebuilt
`http://{ip}:{port}` by hand rather than reading `base_url()`. Two independent
copies of one derivation is what allowed them to disagree; the broker now consumes
`base_url()` verbatim (see `docs/specs/sonos-stream.md` §3.1).

PR #80 fixed the same class of bug for *discovery* by binding SSDP per-interface
instead of `0.0.0.0`. The callback URL was left on the old pattern, which made
things strictly worse: discovery now finds speakers that events then cannot reach.

#### How: the netmask is load-bearing

Subnet containment uses each interface's actual netmask, converted to a prefix
length only for ranking specificity. **Assuming /24 is wrong.** The network this
SDK is developed against is a single **/22**:

| Interface | Address | Netmask | Range |
|-----------|---------|---------|-------|
| `en1` | `192.168.4.32` | `255.255.252.0` (`0xfffffc00`) | `192.168.4.0`-`192.168.7.255` |

Speakers sit on both `192.168.4.x` and `192.168.5.x` and are all directly
reachable from that one interface. A /24 comparison would conclude the
`192.168.5.x` speakers are off-net and either refuse to subscribe or advertise a
wrong address — breaking half the household. `test_selects_interface_across_22_subnet`
encodes exactly this case and fails under a /24 implementation.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Duplicate ~15 lines of interface filtering from `sonos-discovery/src/ssdp.rs` | Export `usable_interfaces()` from `sonos-discovery` and depend on it | `callback-server` sits *below* `sonos-discovery` in the dependency graph; importing would invert the layering for the sake of a filter predicate. Duplicating a filter is the cheaper coupling. |
| Real netmask from `if_addrs::Ifv4Addr::netmask` | Assume /24 | /24 is factually wrong on the project's own network (above) |
| CGNAT (`100.64.0.0/10`) ranked last in the target-less fallback | Treat all interfaces equally | CGNAT is where VPN tunnels live, and a tunnel address is precisely the failure the route probe produced |
| Pure `select_interface` over `(addr, netmask)` pairs | Operate on `if_addrs::Interface` directly | Makes the /22 and VPN cases provable offline with synthetic data — no real NICs, no packets |
| Most-specific prefix wins on overlap | First match | Matches longest-prefix routing semantics |

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

    /// Monotonic instant the notification arrived; used downstream to order
    /// state writes by observation time.
    pub received_at: Instant,
}
```

**Lifecycle**:
1. **Creation**: Created in `EventRouter::route_event()` when a valid event arrives, or in
   `EventRouter::register()` when a buffered event is replayed — in which case `received_at`
   is carried over from the buffer entry rather than re-taken
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
| `if-addrs` | IPv4 interface enumeration with netmasks | Callback address selection needs each interface's real netmask (§4.5); already used by `sonos-discovery` for per-interface SSDP |
| `tokio` | Async runtime | Standard async runtime in Rust ecosystem; required for async HTTP server |
| `axum` | HTTP server framework | This crate serves exactly **one** route, so a framework's composition story is nearly irrelevant; what matters is a small dependency tree, first-party `tower`/`hyper` alignment, and a `FromRequest` extractor that lets every gate run before the handler with its own status. Replaced `warp`, which was the sole reason `openssl-sys` (and its C toolchain requirement) was in the tree |
| `reqwest` | HTTP client (**dev-dependency only**) | Test client for driving the server end to end |

**Deps that used to be listed here and are gone**: `bytes` (axum re-exports `Bytes`, so no
direct dependency), `async-trait` (no async traits in this crate), `thiserror` (errors here
are strings, see §7.1), `uuid`, `url` and `soap-client` (all unused — `soap-client` in
particular was an inverted-layering hazard).

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

- `Content-Length`: Required (bodies without it are rejected with 411, since their size cannot be bounded before reading)

**Body**: UPnP propertyset XML containing changed property values, up to 64 KiB (§10.4)

**Error handling**: Invalid requests receive HTTP 400, oversized bodies 413, bodies with no declared length 411. Valid NOTIFY requests always receive 200 OK (events are buffered if the SID is not yet registered). Network errors on the device side are not our responsibility.

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

HTTP-level errors are one enum, `NotifyRejection`, used as `NotifyRequest`'s
`FromRequest::Rejection`. Each variant implements its own status via `IntoResponse`, so
there is no central `handle_rejection` function and no `anything else => 500` catch-all to
fall through:

```rust
enum NotifyRejection {
    NotNotify,
    LengthRequired,
    BodyTooLarge,
    UnreadableBody(axum::extract::rejection::BytesRejection),
    InvalidUpnpHeaders,
}
```

| Variant | Status | Meaning |
|---------|--------|---------|
| `NotNotify` | 404 | Not a NOTIFY request. Deliberately **not** 405, which would advertise the NOTIFY verb to a scanner |
| `InvalidUpnpHeaders` | 400 | Missing SID, bad NT/NTS, or a header value that is not UTF-8 |
| `BodyTooLarge` | 413 | Declared `Content-Length` over `MAX_NOTIFY_BODY_BYTES` (logged at error level with the limit) |
| `LengthRequired` | 411 | No parseable `Content-Length`, so the body cannot be bounded before reading |
| `UnreadableBody` | 413 or 400 | The read hit `DefaultBodyLimit`, or the connection failed part-way. axum's own mapping is reused rather than second-guessed |

**Why a non-UTF-8 header is 400**: it previously fell through to an unhandled rejection and
became a 500. None of SID/NT/NTS can be valid without being text, so 400 is the honest
answer and it no longer reports a client error as a server fault.

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Fail fast on startup | Port/IP detection errors abort server creation | Better to fail clearly than run in broken state |
| Graceful degradation at runtime | Channel send errors ignored | Receiver dropping is valid shutdown; no need to propagate |
| HTTP-appropriate responses | 400 for bad headers, 200 for all valid NOTIFY (buffered if unregistered) | Always 200 OK for valid events to prevent speakers from cancelling subscriptions |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| Port exhaustion | Yes | Widen port range or wait for ports to free |
| IP detection failure | Partial | May indicate no network; retry after network comes up |
| Invalid UPnP headers | Yes | Device issue; subsequent valid requests will succeed |
| Oversized body (413) | Yes | Not expected from real devices; a genuine over-limit event would indicate the limit needs revisiting (§10.4) |
| Pending buffer eviction | Partial | The dropped event's state is recovered on the next NOTIFY or poll for that SID |
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
- [x] Port availability detection
- [x] Port range scanning
- [x] Local IP detection
- [x] Subnet containment across a **/22** (`test_selects_interface_across_22_subnet`) — the load-bearing case; fails under a /24 assumption (§4.5)
- [x] Interface selection among several candidates, most-specific prefix on overlap (`test_selects_interface_on_target_subnet`)
- [x] CGNAT/VPN tunnel not chosen for a LAN target, in both `select_interface` and the target-less fallback (`test_tunnel_interface_not_chosen_for_lan_target`)
- [x] Loopback / link-local / unspecified filtered out (`test_unusable_addresses_are_filtered`)
- [x] UPnP header validation
- [x] UTF-8-safe trace preview (`test_event_xml_preview_handles_multibyte_boundary`) — asserts the previously-panicking 198-ASCII + multibyte case
- [x] Event router registration and routing
- [x] Pending buffer cap and drop-oldest eviction (`test_pending_buffer_is_bounded`, `test_pending_buffer_evicts_oldest_first`)
- [x] TTL sweep on `route_event`, not just `register()` (`test_stale_entries_swept_on_route`)
- [x] Firewall detection state transitions

**Example**:
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
- [x] SUBSCRIBE/NOTIFY race replay (`test_notify_before_register_is_replayed`)
- [x] Oversized body rejected with 413 (`test_oversized_notify_body_rejected`)

**Example**:
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
| Network interfaces | Synthetic `(addr, netmask)` pairs passed to the pure `select_interface` / `preferred_local_ip` | `src/server.rs` tests — no real NICs, no packets |

**Port allocation in tests**: tests must not hardcode a port range. The
`free_port_range()` helper (present in both `src/server.rs` and
`tests/integration_tests.rs`) binds `0.0.0.0:0`, records the OS-assigned port and
releases it.

Hardcoded ranges were only disjoint *within* one test binary. Two concurrent
`cargo test --workspace` runs both found the low end of each range free and raced
to bind it, failing with "Address already in use". Separate `CARGO_TARGET_DIR`s do
not help, because the contended resource is the host's port space, not the build
directory. Verified with four simultaneous `cargo test` invocations: zero
collisions.

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Event latency | < 10ms from HTTP receipt to channel delivery | Real-time UPnP events should feel instantaneous |
| Concurrent connections | 100+ simultaneous | Support many speakers and services |
| Memory per subscription | < 1KB | Subscription registry should be lightweight |
| Memory per in-flight request | ≤ ~128 KiB | 64 KiB body + one lossy-decode copy; hard ceiling under hostile input |

### 9.2 Critical Paths

1. **Event Routing** (`EventRouter::route_event`)
   - **Complexity**: O(1) average for HashSet lookup
   - **Bottleneck**: RwLock acquisition for subscriptions set
   - **Optimization**: Using `read()` lock for routing; write lock only for register/unregister

2. **HTTP Handler**
   - **Complexity**: O(1) for header extraction and validation
   - **Bottleneck**: Body allocation (`String::from_utf8_lossy` copies the whole body)
   - **Optimization**: Bounded rather than eliminated — `content_length_limit` caps the body at 64 KiB, so the double allocation is bounded at ~128 KiB per request. Header validation runs first so invalid requests skip the copy entirely. Zero-copy is possible but not worth the complexity at these sizes.

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| TCP port | On server creation | On shutdown | No - single port per server |
| Tokio tasks | Server task on creation, timeout task for firewall detection | Graceful shutdown signal | No - long-lived tasks |
| Channel buffers | Unbounded on creation | When receiver drops | No - grows as needed |
| NOTIFY body buffer | Per request, capped at `MAX_NOTIFY_BODY_BYTES` (64 KiB) | End of request | No |
| Pending event buffer | On buffering an unregistered SID, capped at `MAX_PENDING_EVENTS` (256) | TTL sweep, cap eviction, `register()` replay, or `unregister()` drain | No |

---

## 10. Security Considerations

### 10.1 Threat Model

The server binds `0.0.0.0` and accepts NOTIFY on any path. UPnP eventing has no
authentication mechanism, so **any host on the LAN can reach this endpoint** and
the server cannot distinguish a real speaker from a hostile sender. Input
validation and resource bounding are therefore the only defences available, and
every unbounded buffer is a memory-exhaustion primitive.

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious HTTP requests | Medium | Low | Header validation rejects non-UPnP traffic before the body is decoded |
| Memory exhaustion via large body | Medium | High | `content_length_limit(64 KiB)` → 413; missing `Content-Length` → 411 (§10.4) |
| Memory exhaustion via pending buffer | Medium | High | `MAX_PENDING_EVENTS` cap (256) with drop-oldest, plus TTL sweep on every `route_event` (§10.5) |
| Panic-based DoS via non-ASCII payload | Medium | Medium | `preview()` snaps truncation to a UTF-8 char boundary (§10.6) |
| Subscription ID spoofing | Low | Medium | Events only routed to registered subscriptions |
| Unbounded output channel growth | Low | Medium | Not bounded; acceptable because delivery is gated by the size and buffer caps above |
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
| HTTP method | Must be NOTIFY (checked before anything reads the body) | `NotifyRequest::from_request` |
| Content-Length | Must be present and ≤ `MAX_NOTIFY_BODY_BYTES` | `declared_content_length` (declared size) **and** `DefaultBodyLimit` (bytes actually read) |
| SID header | Must be present, and must be UTF-8 | `validate_upnp_headers`, `upnp_header` |
| NT header | If present, must be `upnp:event` — checked **independently** of NTS | `validate_upnp_headers` |
| NTS header | If present, must be `upnp:propchange` — checked **independently** of NT | `validate_upnp_headers` |
| Event body | Size-capped; contents not parsed (passed through) | Consumer responsibility |

**Independent NT/NTS checks**: these were previously gated behind
`if let (Some(nt), Some(nts))`, so a request supplying only one of the two was never
validated — `NT: garbage` with no `NTS` was accepted. Each header is now checked on its own
with `is_some_and`, which keeps both optional while closing that gap.

**Ordering requirement**: header validation must precede `String::from_utf8_lossy`
on the body. The lossy decode allocates a second full copy of the body, so
validating first means an invalid request costs only the (already capped) buffered
bytes rather than double.

### 10.4 Body Size Limit

`MAX_NOTIFY_BODY_BYTES = 64 * 1024` (64 KiB).

**Why a limit is required**: without one, the entire body is buffered and the handler then
allocates a second full copy when decoding it to a `String`. Peak cost is ~2x the body size
per in-flight request, so two concurrent 1 GB posts would be roughly 4 GB resident. The
endpoint is unauthenticated by design (UPnP eventing has no auth), so any host on the LAN
can post to it.

**Why the limit is enforced twice**, deliberately:

| Check | When | What it protects |
|-------|------|------------------|
| `declared_content_length()` | Before a single body byte is read | Refuses a hostile *declaration* for free, and is what turns an undeclared length into 411 |
| `DefaultBodyLimit::max(..)` | While reading | Caps the bytes actually read, so the ceiling holds even if a lying `Content-Length` gets past the first check, or the first check is later changed |

**Why chunked bodies get 411 rather than being read**: a chunked request declares no size
at all, so there is nothing to refuse *before* reading it — the only alternative to 411 is
to start reading a body of unknown length and rely solely on `DefaultBodyLimit` to stop.
411 keeps the cheap up-front refusal for the unauthenticated case. This costs nothing in
practice: real Sonos devices always send `Content-Length` and never chunk (§14.1).

**Why 64 KiB specifically**:

| Payload | Typical size |
|---------|--------------|
| Volume / Mute / TransportState propertyset | ~1-5 KB |
| `ZoneGroupTopology` event, small household | ~2-5 KB |
| `ZoneGroupTopology` event, 32-player household (Sonos max) | ~20-30 KB |

The `ZoneGroupTopology` event is the largest realistic case: its `ZoneGroupState`
property carries a double-XML-escaped topology document containing a UUID,
`Location` URL, and `ZoneName` for every player, running a few hundred bytes per
player. 64 KiB gives ~2x headroom over the 32-player worst case and ~10x over
typical events, while capping the cost of a hostile request at ~128 KiB
(bytes + string copy) instead of unbounded. Legitimate traffic cannot reach it.

**Rejection behaviour**: over-limit → 413 Payload Too Large; missing
`Content-Length` → 411 Length Required. Real Sonos devices always send
`Content-Length`, so 411 does not affect legitimate traffic.

### 10.5 Pending Buffer Cap

`MAX_PENDING_EVENTS = 256`, with drop-oldest eviction.

**Why a cap is required**: `route_event` buffers events for any SID it does not
recognise, which is what bridges the SUBSCRIBE/NOTIFY race. But the TTL sweep
originally ran only inside `register()`, which is called only on a genuine new
subscription. A sender spraying events for random SIDs therefore grew `pending`
without bound and never triggered cleanup — each entry retains a full event body.
The "0-5 entries" expectation held only for well-behaved senders.

**Fix**: `route_event` now sweeps `BUFFER_TTL`-expired entries *and* enforces the
cap on every buffering operation, so cleanup no longer depends on `register()`
being called.

**Why 256**: legitimate occupancy is 0-5 entries (the race window is
microseconds), so 256 is ~50x the real high-water mark and cannot be reached by
normal operation even when a large household brings every subscription up at
once. Worst-case retention is bounded at 256 event bodies.

**Why drop-oldest**: these are pending state snapshots. The newest entry is both
the most current state and the one most likely to still have a `register()`
coming, so the oldest is the cheapest to lose. Eviction picks the minimum
timestamp rather than index 0, because `swap_remove` elsewhere in the module means
index order does not track insertion order.

### 10.6 UTF-8 Boundary Safety

Slicing a `&str` at a byte index that is not a char boundary panics. The
trace-logging path previously did `&event_xml[..200]` directly: 198 ASCII bytes
followed by a 3-byte codepoint spanning bytes 198-200 panics with
`byte index 200 is not a char boundary`. Event XML routinely carries non-ASCII
track metadata, so this is reachable in normal use for a music SDK — a panic in
the request handler, triggerable by any sender, at trace level.

The `preview(s, max)` helper walks the truncation point back to the nearest char
boundary. It is a free function rather than inline code specifically so it can be
unit-tested.

---

## 11. Observability

### 11.1 Logging

The crate uses structured `tracing` events:

| Level | What's Logged |
|-------|--------------|
| `info` | Server startup and bound address |
| `debug` | Incoming request details (method, path, body size, headers), event buffering, pending-buffer eviction, replay on register |
| `trace` | Event XML body — truncated to `TRACE_PREVIEW_BYTES` via `preview()` when longer |
| `error` | Invalid headers, oversized bodies (with the limit), firewall detection results |

**Note**: the `trace` level logs event bodies. `preview()` bounds both the output
size and the char-boundary panic risk on that path.

### 11.2 Tracing

**Current state**: `tracing` events are emitted, but there are no spans.

The blocker is gone, though. The handler used to be a closure inside a `warp` filter chain,
with no named function to attach `#[tracing::instrument]` to. It is now a free async fn,
`handle_notify(State<Arc<EventRouter>>, NotifyRequest) -> StatusCode`, so adding
request-scoped spans is a one-attribute change rather than a refactor.

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `port_range` | `(u16, u16)` | `(3400, 3500)` | Range of ports to search for binding |

Compile-time limits (not runtime-configurable; see §10.4 and §10.5 for rationale):

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_NOTIFY_BODY_BYTES` | `64 KiB` | Maximum accepted NOTIFY body size |
| `TRACE_PREVIEW_BYTES` | `200` | Bytes of event XML included in trace logs |
| `MAX_PENDING_EVENTS` | `256` | Maximum buffered events for unregistered SIDs |
| `BUFFER_TTL` | `5 seconds` | Lifetime of a buffered pending event |

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
| `CallbackServer::base_url()` | Stable | Core functionality. The single authoritative source for the callback URL — consumers must read it, never rebuild `http://{ip}:{port}` themselves |
| `CallbackServer::local_ip_for_speaker()` | Evolving | Reachability diagnostics; the hook for per-subscription URLs (§14.1) |
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
| **One `base_url` cannot serve speakers on genuinely different subnets** | A household split across two subnets gets a callback URL only one half can reach; the other half silently falls back to polling | None; polling still delivers state, just less promptly | **Named follow-up**: per-subscription callback URL selection. `SubscriptionManager::callback_url` is a single `String`, so this needs a signature change through `sonos-stream`. `select_interface` and `CallbackServer::local_ip_for_speaker` are the pieces that follow-up would consume. Moot on the current dev network (one flat /22), but the SDK does **not** support multi-subnet households today. |
| Interface selection is address-based, not route-table-based | A speaker reachable only via a gateway (different subnet, routed) is reported unreachable | Polling fallback | Consult the routing table rather than interface subnets |
| Unbounded output channel | Memory growth under sustained load | Acceptable for UPnP event rates, and bounded upstream by the body-size and pending-buffer caps | Consider bounded with overflow policy |
| Chunked NOTIFY bodies rejected (411) | A device omitting `Content-Length` could not deliver events | None needed; real Sonos devices always send it | Revisit only if a device is found that omits it |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| No request-scoped tracing spans | `src/server.rs` (`handle_notify`) | Low | The handler is now a named async fn, so this is just adding `#[tracing::instrument]` |
| ~~Unused `soap-client` dependency~~ | ~~`Cargo.toml`~~ | — | **Resolved 2026-08-17.** Removed, along with unused `bytes`, `async-trait`, `thiserror`, `uuid` and `url` |
| String-based errors for server startup | `src/server.rs` | Low | Consider `thiserror` enum. The *HTTP* errors are already a typed enum (`NotifyRejection`, §7.1); only the three `CallbackServer::new` failures are strings |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Request-scoped tracing spans | P1 | Correlate log lines for a single NOTIFY. Now unblocked: `handle_notify` is a named async fn | None |
| Configurable callback URL | P2 | Support for Docker/NAT environments where auto-detection fails | None |
| Metrics export | P2 | Prometheus-compatible counters for events received, routing success rate, and rejected/evicted counts | `metrics` crate |
| Per-source-IP rate limiting | P3 | The size and buffer caps bound memory per request, but not request *rate* from a hostile LAN host | None |

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
- [axum documentation](https://docs.rs/axum/) - HTTP server framework
- [tokio documentation](https://docs.rs/tokio/) - Async runtime

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2025-01-14 | Claude | Initial specification created |
| 2026-08-15 | Claude | Replaced route-to-8.8.8.8 IP detection with per-interface selection using each interface's real netmask (§4.5), added `if-addrs` (§6.1), documented the single-`base_url` multi-subnet limitation as a named follow-up (§14.1), and required OS-assigned ports in tests (§8.4). |
| 2026-08-17 | Claude Opus 5 | Ported the single route from `warp` to `axum`: gates now live in a `NotifyRequest` extractor and `NotifyRejection` replaces the rejection-mapping table (§3.1, §3.4, §7.1). Documented the **fixed NT/NTS validation gap** — they were only checked when both were present (§3.1, §10.3) — the two independent body-size checks and why chunked bodies stay 411 (§10.4), and the removal of six unused dependencies (§6.1, §14.2). |
| 2026-08-15 | Claude | Hardened the unauthenticated NOTIFY endpoint: added the 64 KiB body limit (§10.4), the 256-entry pending buffer cap with per-route TTL sweep (§10.5), and UTF-8-safe trace previews (§10.6). Expanded the threat model to state that unbounded buffers are the primary risk given UPnP has no authentication. |
