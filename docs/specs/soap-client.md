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
- [x] Proper SOAP fault extraction with error codes
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
│  │  SOAP Envelope Builder     │  │  extract_response()       │  │
│  │  (inline in call())        │  │  (SOAP fault handling)    │  │
│  └────────────────────────────┘  └───────────────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│                External Dependencies                             │
│  ┌───────────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │  ureq (HTTP)      │  │  xmltree (XML)   │  │  thiserror   │  │
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
    Network(String),   // HTTP/connection failures
    Parse(String),     // XML parsing failures
    Fault(u16),        // SOAP fault with UPnP error code
}
```

**Purpose**: Categorizes all possible failure modes for upstream error handling.

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
│  Return      │◀────│  extract_        │◀────│  Parse XML   │
│  Element     │     │  response()      │     │  (xmltree)   │
└──────────────┘     └──────────────────┘     └──────────────┘
```

**Step-by-step**:

1. **Entry** (`src/lib.rs:80-87`): The `call()` method receives the device IP, endpoint, service URI, action name, and payload.

2. **Envelope Construction** (`src/lib.rs:89-100`): SOAP envelope is constructed inline using `format!()`. This avoids the overhead of a separate envelope builder module.

3. **HTTP Request** (`src/lib.rs:102-110`):
   - URL constructed as `http://{ip}:1400/{endpoint}`
   - SOAPACTION header formatted as `"{service_uri}#{action}"`
   - Request sent via `ureq` with Content-Type `text/xml; charset="utf-8"`

4. **Response Parsing** (`src/lib.rs:112-116`): Raw XML text is parsed into `xmltree::Element`.

5. **Response Extraction** (`src/lib.rs:119`): The `extract_response()` method handles SOAP faults and extracts the action response element.

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

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  ureq error     │────▶│  map to         │────▶│  SoapError::    │
│  (any)          │     │  Network        │     │  Network(msg)   │
└─────────────────┘     └─────────────────┘     └─────────────────┘

┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  xmltree error  │────▶│  map to         │────▶│  SoapError::    │
│  (parse fail)   │     │  Parse          │     │  Parse(msg)     │
└─────────────────┘     └─────────────────┘     └─────────────────┘

┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  SOAP Fault     │────▶│  extract error  │────▶│  SoapError::    │
│  element found  │     │  code from XML  │     │  Fault(code)    │
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

**Error handling philosophy**: Errors are categorized by their source rather than their meaning. This allows upstream crates (like sonos-api) to map errors to their own domain-specific error types while preserving the original cause.

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

UPnP devices return errors as SOAP faults embedded in the response XML rather than HTTP error codes. The error code is buried in nested XML:
```xml
<s:Fault>
  <detail>
    <UpnPError>
      <errorCode>401</errorCode>
    </UpnPError>
  </detail>
</s:Fault>
```

#### How

```rust
fn extract_response(&self, xml: &Element, action: &str) -> Result<Element, SoapError> {
    let body = xml.get_child("Body")?;

    if let Some(fault) = body.get_child("Fault") {
        let error_code = fault
            .get_child("detail")
            .and_then(|d| d.get_child("UpnPError"))
            .and_then(|e| e.get_child("errorCode"))
            .and_then(|c| c.get_text())
            .and_then(|t| t.parse::<u16>().ok())
            .unwrap_or(500);
        return Err(SoapError::Fault(error_code));
    }

    // Extract normal response...
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Return numeric error code only | Parse full fault message | Codes are standardized; messages vary by device |
| Default to 500 on parse failure | Return Parse error | Provides usable error even for malformed faults |

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
| `ureq` | Blocking HTTP client | Lightweight, no async runtime required, supports custom HTTP methods needed for UPnP |
| `xmltree` | XML parsing | Simple DOM-style parsing sufficient for SOAP responses |
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
- HTTP errors mapped to `SoapError::Network`
- SOAP faults extracted and mapped to `SoapError::Fault`
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
    /// Contains UPnP error code (e.g., 401 = Invalid Action)
    #[error("SOAP fault: error code {0}")]
    Fault(u16),
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
| `Network` | Sometimes | Retry after delay; may indicate transient network issue |
| `Parse` | No | Indicates protocol mismatch or device bug |
| `Fault(400-499)` | Sometimes | Client error; check request parameters |
| `Fault(500-599)` | Sometimes | Server error; may be transient |
| `Fault(700-799)` | No | UPnP action-specific errors |

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
- [x] SOAP fault extraction with error code (`test_extract_response_with_soap_fault`)
- [x] Missing Body element handling (`test_extract_response_missing_body`)
- [x] Missing action response handling (`test_extract_response_missing_action_response`)
- [x] Default error code on malformed fault (`test_soap_fault_with_default_error_code`)

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

1. **SOAP Envelope Construction** (`src/lib.rs:89-100`)
   - **Complexity**: O(n) where n = payload size
   - **Bottleneck**: String formatting
   - **Optimization**: Inline format! avoids allocation overhead of builder pattern

2. **Response Parsing** (`src/lib.rs:115-116`)
   - **Complexity**: O(n) where n = response size
   - **Bottleneck**: XML parsing
   - **Optimization**: Single-pass DOM parsing with xmltree

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
| Device responses | XML structure validation | `src/lib.rs:271-292` (extract_response) |
| Timeout header | Safe parsing with fallback | `src/lib.rs:167-177` |
| Error codes | Numeric parsing with default | `src/lib.rs:277-283` |

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
- [xmltree Documentation](https://docs.rs/xmltree)

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2024-01-14 | Claude | Initial specification created |
