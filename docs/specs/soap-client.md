# soap-client Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

Sonos devices communicate using the UPnP (Universal Plug and Play) protocol, which relies on SOAP (Simple Object Access Protocol) over HTTP for control operations and a separate HTTP-based subscription mechanism for receiving state change events. Without a dedicated SOAP transport layer:

1. **Code duplication**: Every crate needing device communication would implement its own HTTP/SOAP handling
2. **Inconsistent behavior**: Different implementations would have varying timeout handling, connection management, and error handling
3. **Resource waste**: Multiple HTTP clients would be created, each with their own connection pools, consuming unnecessary memory
4. **Protocol complexity**: UPnP subscription operations (SUBSCRIBE/UNSUBSCRIBE/RENEW) use non-standard HTTP methods that require specialized handling

The soap-client crate provides a unified, resource-efficient transport layer that handles all low-level SOAP and UPnP HTTP communication.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Resource efficiency via singleton pattern | Multiple Sonos clients should share HTTP connections to minimize memory usage (~95% reduction in multi-client scenarios) |
| P0 | Blocking HTTP client | Simple, synchronous API that integrates easily with the stateless sonos-api design |
| P0 | Complete UPnP support | Must support both SOAP POST for control operations and HTTP SUBSCRIBE/UNSUBSCRIBE for event subscriptions |
| P1 | Minimal API surface | Private crate should expose only what's necessary for internal use |
| P1 | Clear error boundaries | Errors should be categorized by source (network, parsing, SOAP faults) for proper upstream handling |
| P2 | Customizable timeouts | Advanced users may need different timeout configurations for specific network environments |

### 1.3 Non-Goals

- **Async HTTP**: The crate uses blocking I/O via `ureq` because the primary consumer (sonos-api) is designed as a stateless, blocking API. Async event processing is handled at higher layers (sonos-stream, sonos-state).
- **Connection pooling configuration**: The singleton pattern with default timeouts covers 99% of use cases. Connection pool tuning is not exposed.
- **Generic SOAP support**: This crate is specifically designed for UPnP/Sonos communication, not general-purpose SOAP services.
- **Response caching**: Caching is a higher-level concern handled by sonos-state.
- **Public API**: This crate is marked `publish = false` and is intended only for workspace-internal use.

### 1.4 Success Criteria

- [x] Single HTTP agent shared across all SonosClient instances
- [x] Support for SOAP POST operations with proper envelope construction
- [x] Support for UPnP SUBSCRIBE/UNSUBSCRIBE/RENEW methods
- [x] Proper SOAP fault extraction with error codes and descriptions, including from HTTP 500 responses
- [x] Configurable timeouts for both connection and read operations
- [x] Thread-safe singleton access via `LazyLock`

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────┐
│                         Public API                               │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  SoapClient::get() -> &'static SoapClient (singleton)     │   │
│  │  SoapClient::with_agent() -> SoapClient (custom)          │   │
│  └──────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                      Core Methods                                │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────────┐  │
│  │   call()     │  │ subscribe()  │  │ renew_subscription()  │  │
│  │ (SOAP POST)  │  │ (HTTP SUB)   │  │   (HTTP SUBSCRIBE)    │  │
│  └──────┬───────┘  └──────┬───────┘  └───────────┬───────────┘  │
│         │                 │                      │               │
│  ┌──────┴─────────────────┴──────────────────────┴───────────┐  │
│  │                 unsubscribe() (HTTP UNSUBSCRIBE)          │  │
│  └────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│                   Internal Components                            │
│  ┌────────────────────────────┐  ┌───────────────────────────┐  │
│  │  SOAP Envelope Builder     │  │  scan_envelope()          │  │
│  │  (inline in call())        │  │  (fault + shape checking) │  │
│  └────────────────────────────┘  └───────────────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│                External Dependencies                             │
│  ┌───────────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │  ureq 2.x (HTTP)  │  │ quick-xml (XML)  │  │  thiserror   │  │
│  └───────────────────┘  └──────────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

**Design Rationale**: The architecture is intentionally flat with no module hierarchy. Since the crate has a single responsibility (SOAP/HTTP transport), a complex module structure would add unnecessary indirection. The singleton pattern using `LazyLock` ensures thread-safe, zero-cost access to the shared client.

### 2.2 Module Structure

```
src/
├── lib.rs              # Public API, SoapClient struct, singleton
└── error.rs            # SoapError enum
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `lib.rs` | SoapClient implementation, SOAP envelope construction, UPnP subscription methods | `pub` |
| `error.rs` | Error type definitions | `pub` (SoapError only) |

### 2.3 Key Types

#### `SoapClient`

```rust
#[derive(Debug, Clone)]
pub struct SoapClient {
    agent: Arc<ureq::Agent>,  // Shared HTTP connection pool
}
```

**Purpose**: Provides a unified interface for all HTTP communication with Sonos devices.

**Invariants**:
- The `agent` field is always valid (created at construction)
- Cloning is cheap (Arc clone)
- The singleton instance is immutable after initialization

**Ownership**:
- The global singleton (`SHARED_SOAP_CLIENT`) owns the primary instance
- Consumers receive either a `&'static` reference (via `get()`) or a cloned instance (via `clone()` or `default()`)
- `sonos-api::SonosClient` stores a cloned `SoapClient`
- `sonos-api::ManagedSubscription` stores a cloned `SoapClient` for renewal/unsubscribe operations

#### `SubscriptionResponse`

```rust
#[derive(Debug, Clone)]
pub struct SubscriptionResponse {
    pub sid: String,           // Subscription ID from device
    pub timeout_seconds: u32,  // Actual timeout granted
}
```

**Purpose**: Captures the essential information returned by UPnP SUBSCRIBE operations.

**Invariants**:
- `sid` is a non-empty string in UUID format (e.g., `uuid:RINCON_...`)
- `timeout_seconds` is the actual timeout granted, which may differ from the requested timeout

#### `SoapError`

```rust
#[derive(Debug, Error)]
pub enum SoapError {
    Network(String),   // Transport failures, or error statuses with no SOAP fault
    Parse(String),     // XML parsing failures
    Fault {            // SOAP fault returned by the device
        code: u16,             // UPnP error code from <errorCode>
        description: Option<String>, // Device reason from <errorDescription>
    },
}
```

**Purpose**: Categorizes all possible failure modes for upstream error handling.

**Why `Fault` is a struct variant**: The `<errorDescription>` in a UPnP fault body is the only field that states *why* the device refused a request (e.g. code 402 "Invalid Args"). Codes alone are standardized but coarse; carrying the description makes device rejections diagnosable without packet capture. `SoapError::fault(code)` constructs a description-less fault for callers that only have a code.

**Critical distinction**: `Fault` means *the device understood the request and refused it*. `Network` means *the request never produced a device-level answer*. Conflating the two makes "the speaker rejected my arguments" indistinguishable from "the speaker is unplugged" — see §4.3.

---

## 3. Code Flow

### 3.1 Primary Flow: SOAP Call (Control Operations)

```
┌──────────────┐     ┌──────────────────┐     ┌──────────────┐
│  call()      │────▶│  Build SOAP      │────▶│  HTTP POST   │
│  entry       │     │  Envelope        │     │  (ureq)      │
└──────────────┘     └──────────────────┘     └──────┬───────┘
                                                      │
                                                      ▼
┌──────────────┐     ┌──────────────────┐     ┌──────────────┐
│  Return      │◀────│  check_response()│◀────│  Read body   │
│  String      │     │  (scan_envelope) │     │  as text     │
└──────────────┘     └──────────────────┘     └──────────────┘
```

**Step-by-step**:

1. **Entry** (`src/lib.rs`, `call()`): receives the device IP, endpoint, service URI, action name, and payload.

2. **Envelope Construction**: SOAP envelope is constructed inline using `format!()`. This avoids the overhead of a separate envelope builder module.

3. **HTTP Request**:
   - URL constructed as `http://{ip}:1400/{endpoint}`
   - SOAPACTION header formatted as `"{service_uri}#{action}"`
   - Request sent via `ureq` with Content-Type `text/xml; charset="utf-8"`
   - Errors routed through `map_ureq_error()`, which reads the fault body out of
     an error status rather than discarding it (see §3.3)

4. **Body Read**: the response body is read into a `String`. No DOM is built.

5. **Verification** (`check_response()` → `scan_envelope()`): a single streaming `quick-xml`
   pass confirms the body is a fault-free `<{action}Response>` envelope, or returns the
   `SoapError` describing what it actually was.

6. **Return**: the raw body `String`, handed to `sonos-api`'s `parse_response(&str)`.

#### Why `call()` returns `String`, not a parsed DOM

Response *shape* is service-specific: only `sonos-api` knows that a `GetVolume` reply
carries `<CurrentVolume>`. This crate's job is transport plus one question — did the device
refuse? Returning text draws that line cleanly:

- **One XML library, one place.** `sonos-api` already parsed with `quick-xml` + serde.
  Returning `xmltree::Element` forced a second XML crate into the workspace purely as an
  interchange format, and every operation had to bridge between the two models.
- **The guarantee is still enforced here.** Fault detection did *not* move up a layer. An
  `Ok(String)` is contractually an envelope containing `<{action}Response>` and no
  `<Fault>`, so callers cannot forget to check.
- **No wasted parse.** The old path parsed the whole document into a tree, read two or
  three elements out of it, and dropped it. Now there is one streaming scan here and one
  streaming read in `sonos-api`, with no intermediate tree.

The cost is that the guarantee is documented rather than encoded in the type — hence the
explicit contract on `call()`'s doc comment.

### 3.2 Secondary Flow: UPnP Subscription

```
┌──────────────┐     ┌──────────────────┐     ┌──────────────┐
│  subscribe() │────▶│  Build Headers   │────▶│ HTTP SUBSCRIBE│
│  entry       │     │  (CALLBACK,NT)   │     │   (ureq)     │
└──────────────┘     └──────────────────┘     └──────┬───────┘
                                                      │
                                                      ▼
┌──────────────────────┐     ┌─────────────────────────────┐
│  SubscriptionResponse │◀────│  Parse SID & TIMEOUT headers│
└──────────────────────┘     └─────────────────────────────┘
```

**Step-by-step**:

1. **Entry** (`src/lib.rs:133-140`): The `subscribe()` method receives device IP, port, endpoint, callback URL, and timeout.

2. **Header Construction** (`src/lib.rs:144-150`):
   - HOST: `{ip}:{port}`
   - CALLBACK: `<{callback_url}>` (angle brackets required by UPnP spec)
   - NT: `upnp:event`
   - TIMEOUT: `Second-{timeout_seconds}`

3. **HTTP SUBSCRIBE** (`src/lib.rs:144-151`): Uses `ureq`'s generic `request()` method for non-standard HTTP verb.

4. **Response Parsing** (`src/lib.rs:160-177`): Extracts SID and TIMEOUT from response headers.

### 3.3 Error Flow

**The `ureq` status trap**: `ureq` 2.x does not return `Ok` for HTTP statuses >= 400. It returns `Err(Error::Status(code, Response))`, with the response body *still unread inside the error*. UPnP faults arrive as **HTTP 500 with a SOAP fault body**, so they land in this error branch — not in the success path.

This means the error branch must discriminate on `ureq::Error`, not map it wholesale:

```
                          ┌──────────────────────┐
   ureq::Error ──────────▶│  map_ureq_error()    │
                          └──────────┬───────────┘
                                     │
              ┌──────────────────────┴───────────────────────┐
              │                                              │
     Error::Status(code, resp)                     Error::Transport(t)
              │                                              │
              ▼                                              ▼
     read resp.into_string()                        SoapError::Network
              │                                     (unreachable host,
              ▼                                      timeout, DNS, …)
     scan envelope, run
     check_response()
              │
      ┌───────┴────────┐
      │                │
 <s:Fault> found   no fault
      │                │
      ▼                ▼
 SoapError::Fault  SoapError::Network
 { code,            ("HTTP {status}")
   description }
```

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ quick-xml error │────▶│  map to         │────▶│  SoapError::    │
│  (syntax fail)  │     │  Malformed      │     │  Parse(msg)     │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

**Error handling philosophy**: Errors are categorized by their source rather than their meaning. This allows upstream crates (like sonos-api) to map errors to their own domain-specific error types while preserving the original cause.

**Why the body must be read in the error branch**: Mapping `ureq::Error` directly to `Network` — without reading the response — makes the fault-parsing code unreachable in production. Every device rejection then reports as a generic network error, and `SoapError::Fault` can only ever be produced by unit tests calling `check_response()` directly. Any future refactor of this branch must preserve the body read.

**Subscription errors**: SUBSCRIBE/UNSUBSCRIBE endpoints return no SOAP envelope, so there is no fault to parse. `map_subscription_error()` keeps them as `Network` but preserves the HTTP status in the message, since the status is the actionable detail (e.g. `412 Precondition Failed` for an expired SID).

**Success-status checks**: Post-request status checks use `is_success()` (200..300) rather than `== 200`. Because `ureq` has already converted >= 400 into an error, a `== 200` check can only ever reject a 2xx/3xx *success* such as a spec-legal `201`; it can never catch a failure.

---

## 4. Features

### 4.1 Feature: Singleton Pattern

#### What

A global shared `SoapClient` instance accessible via `SoapClient::get()`.

#### Why

Sonos applications typically interact with multiple devices simultaneously. Without connection sharing, each device interaction would create a separate HTTP client with its own connection pool, leading to:
- Excessive memory usage
- Connection pool fragmentation
- Potential socket exhaustion

#### How

```rust
static SHARED_SOAP_CLIENT: LazyLock<SoapClient> = LazyLock::new(|| {
    SoapClient {
        agent: Arc::new(
            ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(10))
                .build(),
        ),
    }
});

impl SoapClient {
    pub fn get() -> &'static Self {
        &SHARED_SOAP_CLIENT
    }
}
```

**Usage example**:
```rust
// All clients share the same HTTP connection pool
let client1 = SoapClient::get();
let client2 = SoapClient::get();
assert!(std::ptr::eq(client1, client2)); // Same instance
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Global singleton | Thread-local clients | Global sharing maximizes connection reuse across threads |
| `LazyLock` | `OnceCell` or `lazy_static!` | `LazyLock` is stdlib (no dependency) and const-initializable |
| Static lifetime return | Return Arc clone | Reference avoids allocation, caller can clone if needed |

### 4.2 Feature: UPnP Event Subscriptions

#### What

Methods for managing UPnP event subscriptions: `subscribe()`, `renew_subscription()`, and `unsubscribe()`.

#### Why

UPnP event subscriptions are fundamentally different from SOAP control operations:
- Use HTTP SUBSCRIBE/UNSUBSCRIBE methods instead of POST
- Require special headers (CALLBACK, NT, SID, TIMEOUT)
- Return subscription metadata in response headers, not body

Bundling these in soap-client keeps all HTTP communication in one place.

#### How

```rust
pub fn subscribe(
    &self,
    ip: &str,
    port: u16,
    event_endpoint: &str,
    callback_url: &str,
    timeout_seconds: u32,
) -> Result<SubscriptionResponse, SoapError>
```

The implementation uses `ureq`'s `request()` method which accepts arbitrary HTTP verbs:

```rust
self.agent
    .request("SUBSCRIBE", &url)
    .set("CALLBACK", &format!("<{}>", callback_url))
    .set("NT", "upnp:event")
    .set("TIMEOUT", &format!("Second-{}", timeout_seconds))
    .call()
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Inline header formatting | Separate struct/builder | Simpler code, headers are straightforward |
| Parse timeout from response | Trust requested timeout | Devices may grant shorter timeouts |
| Default to requested timeout on parse failure | Return error | Graceful degradation for non-compliant devices |

### 4.3 Feature: SOAP Fault Handling

#### What

Automatic detection and extraction of SOAP faults with UPnP error codes.

#### Why

UPnP devices report action failures as SOAP faults embedded in an HTTP 500 response body rather than via the HTTP status alone. The code and reason are buried in nested XML:
```xml
<s:Fault>
  <detail>
    <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
      <errorCode>402</errorCode>
      <errorDescription>Invalid Args</errorDescription>
    </UPnPError>
  </detail>
</s:Fault>
```

**Element name**: The UPnP Device Architecture spec spells this `UPnPError` (capital `P`, capital `E`). Matching is exact, so the spelling is load-bearing — a lookup for `UpnPError` silently misses on every spec-compliant device and falls through to the default code, collapsing all faults to 500. `UPNP_ERROR_ELEMENT` is the primary name; `UPNP_ERROR_ELEMENT_LEGACY` (`UpnPError`) is accepted on read only, as defensive tolerance for devices or firmware that emit it.

#### How

`scan_envelope()` makes one streaming `quick-xml` pass over the body, maintaining a stack
of element **local** names (so `s:Body` matches `Body`) and answering three questions at
once: is there a `<Body>`, is there a `<Fault>` under it, and is there a
`<{action}Response>`. Fault fields are accumulated into a `FaultFields` per spelling, then
normalized: trim, parse the code with `UNKNOWN_FAULT_CODE` (500) as fallback, and treat a
blank description as absent.

```rust
// Matching is on the exact child path, not "anywhere in the document":
at_path(&path, &["Body"])                                              // Body
at_path(&path, &["Body", "Fault"])                                     // Fault
at_path(&path, &["Body", "Fault", "detail", element, "errorCode"])     // code
```

Three properties of this design are deliberate, and two of them fixed real gaps:

- **Path-exact `<Fault>` matching.** `Fault` counts only as a direct child of `Body`, which
  is what the previous `xmltree` `get_child` chain enforced structurally. A looser
  "anywhere in the document" scan would let a `<Fault>` appearing inside legitimate
  response content — a queue item titled "Fault", a `<Detail><Fault>` in a device's own
  payload — turn a **successful** call into an error. There is a regression test for this
  (`test_extract_response_ignores_non_toplevel_fault`).

- **Truncated envelopes are rejected.** `quick-xml` reports `Eof` for a document whose tags
  are still open rather than erroring, so a body cut off mid-transfer would otherwise scan
  as a perfectly good `<{action}Response>` and be handed upstream as a success.
  `xmltree::Element::parse` rejected those for us; the scanner now asserts the element
  stack is empty at EOF and reports `<{tag}> was never closed`. A half-received body is not
  an answer.

- **Text is appended, not overwritten.** An element split across several text/CDATA nodes
  reads the same as `xmltree`'s `get_text`, which concatenated them.

Per-spelling accumulation (rather than one shared buffer) means the spec spelling wins
wholesale if a device somehow emits both, matching the old `get_child(..).or_else(..)`
which picked one element and read both fields from it.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Carry code *and* description | Numeric code only | The description is the only field stating *why* the device refused; codes alone are too coarse to debug |
| Accept both element spellings | Spec spelling only | Zero cost, and tolerates non-compliant firmware without weakening spec support |
| Default to 500 on parse failure | Return Parse error | Provides usable error even for malformed faults |
| `description: Option<String>` | Empty string default | Absent and empty are distinguishable; blank text is normalized to `None` |
| Streaming scan with a path stack | Build a DOM and walk it | No second XML crate, no whole-document tree for two fields, and path-exactness is explicit rather than an accident of the walk |
| Explicit unclosed-tag check | Trust the parser to error | `quick-xml` returns `Eof`, not an error, for unclosed tags — trusting it would accept truncated responses |

**Note on propagation**: `sonos-api` maps `SoapError::Fault` to `ApiError::SoapFault(u16)`, which carries only the code. The description is intentionally dropped at that boundary because widening the public `ApiError` variant would be a breaking change to a semver-checked crate. The description remains available to workspace-internal consumers of `SoapError`.

---

## 5. Data Model

### 5.1 Core Data Structures

#### `SoapClient`

```rust
pub struct SoapClient {
    /// Shared HTTP agent with connection pool
    /// Uses Arc for cheap cloning and thread-safe sharing
    agent: Arc<ureq::Agent>,
}
```

**Lifecycle**:
1. **Creation**: Either via `LazyLock` singleton initialization (preferred) or `with_agent()` for custom configurations
2. **Mutation**: Immutable after creation - all state is in the HTTP agent which manages its own connection pool
3. **Destruction**: Singleton lives for program duration; custom instances dropped when last Arc reference is dropped

**Memory considerations**:
- `SoapClient` is 8 bytes (single Arc pointer)
- Cloning is O(1) - just an atomic increment
- The underlying `ureq::Agent` manages its own connection pool (~1-2KB overhead)

#### `SubscriptionResponse`

```rust
pub struct SubscriptionResponse {
    /// UUID format subscription ID (e.g., "uuid:RINCON_...")
    pub sid: String,
    /// Actual timeout in seconds (may differ from requested)
    pub timeout_seconds: u32,
}
```

**Lifecycle**:
1. **Creation**: Returned from `subscribe()` method
2. **Mutation**: Immutable after creation (all fields pub for read access)
3. **Destruction**: Standard drop, no special cleanup

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `ureq` | Blocking HTTP client | Lightweight, no async runtime required, supports custom HTTP methods needed for UPnP. **Pinned to 2.x**: 3.x removes `Agent::request`, `Error::Status` and `Error::Transport`, all of which this crate uses — `Error::Status` in particular is how the fault body is recovered (§3.3) |
| `quick-xml` | Envelope scanning | The same XML crate `sonos-api` already used. One library across the workspace means the scanner here and the deserializer there cannot disagree about namespaces or entity decoding. Streaming, so no DOM is built for a two-field lookup |
| `thiserror` | Error derivation | Consistent error handling pattern across workspace |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| `sonos-api` | `SonosClient` wraps `SoapClient` for all device communication | Primary consumer; changes here require sonos-api updates |
| `sonos-stream` | References `SoapClient` in error types | Minimal coupling via error types only |
| `callback-server` | Dependency declared but not actively used | May be removed or used in future for subscription management |

### 6.3 External Systems

```
┌─────────────────┐              ┌─────────────────┐
│   soap-client   │◀────────────▶│  Sonos Device   │
│                 │    HTTP/1.1  │   (UPnP/SOAP)   │
└─────────────────┘              └─────────────────┘
```

**Protocol**: HTTP/1.1 with:
- SOAP POST for control operations
- HTTP SUBSCRIBE/UNSUBSCRIBE for event subscriptions

**Port**: 1400 (standard Sonos UPnP port, hardcoded in sonos-api layer)

**Authentication**: None - Sonos devices use network locality for security

**Error handling**:
- Transport failures (unreachable host, timeout, DNS) mapped to `SoapError::Network`
- SOAP faults extracted from the HTTP 500 response body and mapped to `SoapError::Fault`
- Error statuses with no parseable fault body mapped to `SoapError::Network` with the status preserved
- XML parse failures mapped to `SoapError::Parse`

**Retry strategy**: None at this layer - retry logic is implemented in higher layers (sonos-api, sonos-stream)

---

## 7. Error Handling

### 7.1 Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum SoapError {
    /// Network or HTTP communication failure
    /// Includes connection timeouts, DNS failures, HTTP errors
    #[error("Network/HTTP error: {0}")]
    Network(String),

    /// XML parsing or structure validation failure
    /// Includes malformed XML, missing required elements
    #[error("XML parsing error: {0}")]
    Parse(String),

    /// SOAP fault returned by the device
    /// Contains the UPnP error code (e.g., 401 = Invalid Action,
    /// 402 = Invalid Args) and the device's reason when supplied
    #[error("SOAP fault: error code {code}...")]
    Fault {
        code: u16,
        description: Option<String>,
    },
}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Categorize by source | Three variants: Network, Parse, Fault | Enables upstream error mapping without losing context |
| Preserve error messages | Store original error message as String | Debugging requires original error details |
| Use standard codes | UPnP error codes preserved numerically | Enables programmatic error handling |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `Network` | Sometimes | Retry after delay; device unreachable or transient network issue |
| `Parse` | No | Indicates protocol mismatch or device bug |
| `Fault` code 400-499 | No | Client error; the request itself is wrong (bad action or args) - retrying is futile |
| `Fault` code 500-599 | Sometimes | Device-side error; may be transient |
| `Fault` code 700-799 | Sometimes | UPnP action-specific (e.g. 701 transition not available) - depends on device state, may succeed later |

**Retry implication**: Because faults were previously reported as `Network`, upstream retry logic could not distinguish a permanently-invalid request from a transient outage. Correct fault classification lets higher layers avoid retrying 4xx faults.

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    ┌───────────────────┐
                    │  Integration      │  (requires real device)
                    └─────────┬─────────┘
              ┌───────────────┴───────────────┐
              │       Response Parsing        │  100% coverage
              └───────────────┬───────────────┘
    ┌─────────────────────────┴─────────────────────────┐
    │                   Unit Tests                       │  100% coverage
    └────────────────────────────────────────────────────┘
```

### 8.2 Unit Tests

**Location**: `src/lib.rs` inline `#[cfg(test)]` module

**What is tested**:
- [x] Singleton pattern returns same instance (`test_singleton_pattern_consistency`)
- [x] Client creation doesn't panic (`test_soap_client_creation`)
- [x] Valid SOAP response parsing (`test_extract_response_with_valid_response`)
- [x] SOAP fault extraction with code and description, spec spelling (`test_extract_response_with_soap_fault`)
- [x] Legacy `UpnPError` spelling still parsed (`test_extract_response_with_legacy_upnperror_spelling`)
- [x] HTTP 500 + fault body yields `Fault`, not `Network` (`test_status_500_with_fault_body_yields_fault`)
- [x] Error status without a fault body stays `Network` (`test_status_error_without_fault_body_yields_network`)
- [x] Subscription errors preserve HTTP status (`test_subscription_error_preserves_http_status`)
- [x] Non-200 success codes accepted (`test_is_success_accepts_non_200_success_codes`)
- [x] Missing Body element handling (`test_extract_response_missing_body`)
- [x] Missing action response handling (`test_extract_response_missing_action_response`)
- [x] Response for a *different* action rejected (`test_extract_response_rejects_other_action_response`)
- [x] Self-closing response element accepted (`test_extract_response_accepts_self_closing_response`)
- [x] A `<Fault>` nested in legitimate response content does **not** become an error (`test_extract_response_ignores_non_toplevel_fault`)
- [x] Truncated envelope rejected rather than read as a success (`test_extract_response_rejects_truncated_envelope`)
- [x] Non-XML body is a `Parse` error, not a missing Body (`test_extract_response_with_non_xml_body`)
- [x] Default error code on malformed fault (`test_soap_fault_with_default_error_code`)
- [x] Blank `<errorDescription>` normalized to `None` (`test_soap_fault_blank_description_is_none`)
- [x] Non-numeric `<errorCode>` falls back rather than failing (`test_soap_fault_unparseable_code_falls_back`)
- [x] Escaped entities in a fault description are decoded (`test_soap_fault_description_is_unescaped`)
- [x] Granted TIMEOUT parsed, with fallback for `infinite`/malformed/absent (`test_granted_timeout_parsing_and_fallback`)

The test names keep the historical `test_extract_response_*` prefix even though the function
is now `check_response`; they are pinned by the spec rather than renamed so the mapping from
each documented behaviour to its regression test survives the refactor.

**Testing `ureq` error mapping without a device**: `ureq::Response::new(status, status_text, body)` is public, so `ureq::Error::Status(500, resp)` can be constructed directly and driven through `map_ureq_error()`. This covers the fault-detection path with no network or speaker involved.

**Example**:
```rust
#[test]
fn test_singleton_pattern_consistency() {
    let client1 = SoapClient::get();
    let client2 = SoapClient::get();

    // Both should point to the same static instance
    assert!(std::ptr::eq(client1, client2));

    // Clones should share the same underlying agent
    let cloned1 = client1.clone();
    let cloned2 = client2.clone();
    assert!(Arc::ptr_eq(&cloned1.agent, &cloned2.agent));
}
```

### 8.3 Integration Tests

**Location**: Tested via `sonos-api` CLI example

**Prerequisites**:
- [x] Sonos device on local network
- [x] Network connectivity to device on port 1400

**What to test**:
- [x] End-to-end SOAP call with real device
- [x] Subscription creation and cancellation

### 8.4 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| HTTP responses | Inline XML strings | `src/lib.rs` test module |
| ureq Agent | Not mocked (unit tests focus on parsing) | N/A |

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Memory per client | ~8 bytes | Only stores Arc pointer; all HTTP resources shared |
| Clone cost | O(1) atomic | Arc increment only |
| Connection reuse | 100% for same host | ureq Agent pools connections by host |
| Timeout (connect) | 5 seconds | Fast failure for unreachable devices |
| Timeout (read) | 10 seconds | Accommodate slow devices/networks |

### 9.2 Critical Paths

1. **SOAP Envelope Construction** (`src/lib.rs`, in `call()`)
   - **Complexity**: O(n) where n = payload size
   - **Bottleneck**: String formatting
   - **Optimization**: Inline format! avoids allocation overhead of builder pattern

2. **Envelope Verification** (`src/lib.rs`, `scan_envelope()`)
   - **Complexity**: O(n) where n = response size
   - **Bottleneck**: Network I/O dominates; UPnP bodies are small
   - **Design**: One streaming `quick-xml` pass, no DOM. Only the fault leaves allocate
     (a `String` per path element on the stack, and the fault text if present). The
     previous design built a full `xmltree` tree, read two or three elements out, and
     dropped it — and then `sonos-api` did its own parse on top

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| HTTP connections | On first request to host | After idle timeout (ureq default) | Yes - per-host pooling |
| Memory buffers | Per request | After response parsed | No - allocated fresh |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Network eavesdropping | Medium | Low | Sonos only operates on local network; no sensitive data |
| Malicious device responses | Low | Medium | XML parsing limits prevent DoS; no code execution |
| Request tampering | Low | Low | Local network only; devices reject malformed requests |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Device IPs | Low | Not logged; transient in memory |
| Subscription IDs | Low | UUIDs, no inherent meaning |
| Callback URLs | Medium | Reveals local network topology; not persisted |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| Device responses | Envelope shape: XML well-formedness, no unclosed tags, `Body` present, `<{action}Response>` present | `src/lib.rs` (`scan_envelope`, `check_response`) |
| Timeout header | Safe parsing with fallback | `src/lib.rs` (`granted_timeout`) |
| Error codes | Numeric parsing with `UNKNOWN_FAULT_CODE` default | `src/lib.rs` (`FaultFields::into_scan`) |
| Error status bodies | Scanned as a SOAP envelope, falling back to `Network` | `src/lib.rs` (`map_ureq_error`) |
| Truncated bodies | Element stack must be empty at EOF, else `Parse` | `src/lib.rs` (`scan_envelope`) |

---

## 11. Observability

### 11.1 Logging

The soap-client crate currently does not include logging. All observability is handled at higher layers (sonos-api, sonos-stream).

| Level | What Would Be Logged | Status |
|-------|---------------------|--------|
| `error` | Network failures, parse errors | Not implemented |
| `debug` | Request/response details | Not implemented |
| `trace` | Full XML bodies | Not implemented |

**Rationale**: As a low-level transport crate, logging is deferred to consumers who have more context about which operations are significant.

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| Connect timeout | `Duration` | 5 seconds | Maximum time to establish TCP connection |
| Read timeout | `Duration` | 10 seconds | Maximum time to receive complete response |

These are hardcoded in the singleton but can be customized via `SoapClient::with_agent()`:

```rust
let custom_agent = ureq::AgentBuilder::new()
    .timeout_connect(Duration::from_secs(10))
    .timeout_read(Duration::from_secs(30))
    .build();
let client = SoapClient::with_agent(Arc::new(custom_agent));
```

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `SoapClient::get()` | Stable | Primary API, no changes planned |
| `SoapClient::with_agent()` | Stable | Escape hatch for custom configuration |
| `SoapClient::new()` | Deprecated | Marked deprecated since 0.2.0; use `get()` |
| `call()` | Stable | Core functionality |
| `subscribe()`, `renew_subscription()`, `unsubscribe()` | Stable | UPnP subscription API |

### 13.2 Breaking Changes

**Policy**: As a private crate, breaking changes are coordinated within the workspace. Downstream crates are updated atomically.

**Current deprecations**:
- `SoapClient::new()`: Use `SoapClient::get()` instead (marked `#[deprecated]` at `src/lib.rs:69`)

### 13.3 Version History

| Version | Changes | Migration Guide |
|---------|---------|-----------------|
| 0.1.0 | Initial release | N/A |
| 0.2.0 | Added singleton pattern, deprecated `new()` | Replace `SoapClient::new()` with `SoapClient::get().clone()` |

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Hardcoded timeouts in singleton | Cannot adjust timeouts globally | Use `with_agent()` for custom timeouts | None planned |
| No connection pool metrics | Cannot monitor pool health | None | Consider adding metrics |
| callback-server dependency unused | Unnecessary compilation | May be used in future | Review and remove if unneeded |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| Deprecated `new()` method | `src/lib.rs:69-77` | Low | Remove in next major version |
| Unused callback-server dependency | `Cargo.toml` | Low | Review usage, potentially remove |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Tracing integration | P2 | Better debugging for complex scenarios | tracing crate |
| Connection pool metrics | P2 | Operational visibility | metrics crate |

### 15.2 Open Questions

- [ ] **Should we add async support?**: The blocking design was intentional, but async would enable better integration with sonos-stream. May add a feature-gated async variant.
- [ ] **Remove callback-server dependency?**: Currently listed but not used. Verify if planned for future use.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| SOAP | Simple Object Access Protocol - XML-based messaging protocol used by UPnP |
| UPnP | Universal Plug and Play - Network protocol for device discovery and control |
| SID | Subscription ID - UUID returned by device for event subscriptions |
| SOAPACTION | HTTP header specifying the SOAP operation being invoked |

### B. References

- [UPnP Device Architecture 1.1](http://www.upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v1.1.pdf)
- [SOAP 1.1 Specification](https://www.w3.org/TR/2000/NOTE-SOAP-20000508/)
- [ureq Documentation](https://docs.rs/ureq)
- [quick-xml Documentation](https://docs.rs/quick-xml)

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2024-01-14 | Claude | Initial specification created |
| 2026-08-15 | Claude | Documented the `ureq` error-status trap (§3.3): UPnP faults arrive as HTTP 500 and must be read out of `ureq::Error::Status`. Corrected fault element spelling to `UPnPError` (§4.3), widened `SoapError::Fault` to carry `errorDescription` (§2.3), and replaced `status() != 200` checks with `is_success()`. |
| 2026-08-17 | Claude Opus 5 | `call()` now returns `Result<String, SoapError>` instead of `Result<xmltree::Element, _>`, and fault detection is a streaming `quick-xml` scan (`scan_envelope`). `xmltree` removed from the workspace. Documented why text and not a DOM (§3.1), path-exact `<Fault>` matching and truncated-envelope rejection (§4.3), and the `ureq` 2.x pin (§6.1). |
