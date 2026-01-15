# sonos-discovery Specification

---

## 1. Purpose & Motivation

### 1.1 Problem Statement

Sonos devices on a local network are not directly addressable without first discovering their IP addresses and capabilities. Before any speaker control operations can be performed, applications must:

1. Detect which Sonos devices exist on the network
2. Obtain their IP addresses for subsequent communication
3. Identify device metadata (room name, model, unique ID) for user presentation
4. Filter out non-Sonos UPnP devices that respond to discovery requests

Without this crate, every application would need to implement SSDP multicast communication, handle UDP socket timeouts, parse UPnP XML descriptions, and manage the complex filtering logic to identify genuine Sonos devices among potentially hundreds of UPnP-enabled devices on a network.

### 1.2 Design Goals

| Priority | Goal | Rationale |
|----------|------|-----------|
| P0 | Reliable device discovery | Core functionality - must find all Sonos devices on the local network |
| P0 | Zero-configuration operation | Users should not need to know IP addresses or configure network settings |
| P1 | Resource efficiency | UDP sockets and HTTP connections must be properly cleaned up, even on early termination |
| P1 | Accurate device filtering | Must distinguish Sonos devices from other UPnP devices (routers, smart TVs, etc.) |
| P2 | Flexible consumption patterns | Support both collect-all and streaming iterator patterns for different use cases |
| P2 | Configurable timeouts | Allow tuning discovery duration based on network conditions |

### 1.3 Non-Goals

- **Continuous monitoring**: This crate performs one-shot discovery. Persistent device tracking is handled by `sonos-state`.
- **Device communication**: Discovery only identifies devices. Control operations are handled by `sonos-api`.
- **Async runtime integration**: Uses blocking I/O intentionally for simplicity and to avoid forcing async on consumers.
- **IPv6 support**: Sonos devices currently use IPv4 for SSDP discovery.
- **Device caching**: No persistence of discovered devices between calls. Each discovery is fresh.

### 1.4 Success Criteria

- [x] Discovers all Sonos devices on a local network within the timeout period
- [x] Returns zero results (not errors) when no devices are found
- [x] Properly cleans up UDP sockets and HTTP connections on iterator drop
- [x] Filters out non-Sonos UPnP devices (routers, smart TVs, media servers)
- [x] Deduplicates devices that respond multiple times to SSDP requests
- [x] Provides accurate device metadata (name, room, model, ID)

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Public API (lib.rs)                             │
│  get() / get_with_timeout() / get_iter() / get_iter_with_timeout()      │
├─────────────────────────────────────────────────────────────────────────┤
│                       DiscoveryIterator (discovery.rs)                   │
│  - Coordinates discovery flow                                            │
│  - Deduplicates by location URL                                          │
│  - Filters non-Sonos devices                                             │
│  - Converts to public Device type                                        │
├──────────────────────────┬──────────────────────────────────────────────┤
│    SsdpClient (ssdp.rs)  │         DeviceDescription (device.rs)        │
│  - UDP multicast M-SEARCH│  - XML parsing with quick-xml                │
│  - Response iteration    │  - Sonos validation logic                    │
│  - Timeout handling      │  - Device type conversion                    │
├──────────────────────────┴──────────────────────────────────────────────┤
│                        HTTP Client (reqwest)                             │
│  - Fetches device description XML                                        │
│  - Timeout-aware blocking requests                                       │
└─────────────────────────────────────────────────────────────────────────┘
          │                               │
          ▼                               ▼
   ┌──────────────┐              ┌──────────────────┐
   │ UDP Socket   │              │ Sonos Device     │
   │ 239.255.255. │              │ HTTP/1400        │
   │     250:1900 │              │ /xml/device_     │
   │  (Multicast) │              │ description.xml  │
   └──────────────┘              └──────────────────┘
```

**Design Rationale**: The architecture separates concerns into three layers:
1. **Public API**: Simple function calls that hide complexity
2. **Iterator logic**: Manages the multi-step discovery workflow
3. **Protocol handlers**: SSDP for discovery, HTTP for metadata retrieval

This separation allows the SSDP and XML parsing logic to be tested independently while providing a clean, ergonomic API to consumers.

### 2.2 Module Structure

```
src/
├── lib.rs              # Public API surface and Device/DeviceEvent types
├── discovery.rs        # DiscoveryIterator implementation
├── ssdp.rs            # SSDP protocol implementation (internal)
├── device.rs          # UPnP XML parsing and Sonos validation (pub for testing)
└── error.rs           # Error types
```

| Module | Responsibility | Visibility |
|--------|---------------|------------|
| `lib` | Public API functions, `Device`, `DeviceEvent` types | `pub` |
| `discovery` | `DiscoveryIterator` coordinating the discovery workflow | `pub` (type only) |
| `ssdp` | SSDP client and response parsing | `pub(crate)` |
| `device` | UPnP XML parsing and Sonos device validation | `pub` (for test access) |
| `error` | `DiscoveryError` enum and `Result` alias | `pub` |

### 2.3 Key Types

#### `Device`

```rust
/// Information about a discovered Sonos device
pub struct Device {
    pub id: String,           // UDN: "uuid:RINCON_7828CA0E1E1801400"
    pub name: String,         // Friendly name from UPnP
    pub room_name: String,    // Sonos room assignment
    pub ip_address: String,   // Device IP for communication
    pub port: u16,            // Always 1400 for Sonos
    pub model_name: String,   // e.g., "Sonos One", "Sonos Play:1"
}
```

**Purpose**: Represents all information needed to identify and connect to a Sonos speaker.

**Invariants**:
- `id` always starts with "uuid:" prefix
- `id` contains "RINCON" (Sonos device identifier)
- `port` is always 1400
- All string fields are non-empty

**Ownership**: Created by `DeviceDescription::to_device()`, owned by caller after discovery.

#### `DeviceEvent`

```rust
pub enum DeviceEvent {
    Found(Device),
}
```

**Purpose**: Event-based API that allows for future extension (e.g., `Lost`, `Updated` events).

**Design Rationale**: Using an enum rather than returning `Device` directly future-proofs the API. New event types can be added without breaking existing code that matches on `Found`.

#### `DiscoveryIterator`

```rust
pub struct DiscoveryIterator {
    ssdp_client: Option<SsdpClient>,    // Consumed on first iteration
    ssdp_buffer: Vec<SsdpResponse>,     // Cached SSDP responses
    buffer_index: usize,                 // Current position in buffer
    seen_locations: HashSet<String>,     // For deduplication
    http_client: reqwest::blocking::Client,
    finished: bool,
}
```

**Purpose**: Implements `Iterator<Item = DeviceEvent>` for streaming device discovery.

**Invariants**:
- `ssdp_client` is `Some` only before first `next()` call
- `seen_locations` prevents duplicate devices
- `finished` is `true` after SSDP search completes

**Ownership**: Created by `get_iter*` functions, owned by caller. Implements `Drop` for resource cleanup.

---

## 3. Code Flow

### 3.1 Primary Flow: Device Discovery

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   get_iter() │────▶│  SSDP Search │────▶│  HTTP Fetch  │────▶│ Parse & Emit │
│   lib.rs:141 │     │  ssdp.rs:40  │     │discovery.rs: │     │device.rs:47  │
│              │     │              │     │     101      │     │              │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
       │                    │                    │                    │
       │                    ▼                    ▼                    ▼
       │             ┌────────────┐       ┌────────────┐       ┌────────────┐
       │             │ UDP Socket │       │ HTTP GET   │       │ XML Parse  │
       │             │ Multicast  │       │ /xml/...   │       │ quick-xml  │
       │             └────────────┘       └────────────┘       └────────────┘
       │
       ▼
  DiscoveryIterator::new()
  discovery.rs:47
```

**Step-by-step**:

1. **Entry** (`src/lib.rs:141`): `get_iter()` calls `get_iter_with_timeout()` with 3-second default timeout.

2. **Iterator Creation** (`src/discovery.rs:47-62`): `DiscoveryIterator::new()` creates:
   - `SsdpClient` bound to ephemeral UDP port with configured timeout
   - `reqwest::blocking::Client` for HTTP requests
   - Empty `HashSet` for deduplication

3. **SSDP M-SEARCH** (`src/ssdp.rs:40-56`): On first `next()` call:
   - Sends M-SEARCH multicast to `239.255.255.250:1900`
   - Target: `urn:schemas-upnp-org:device:ZonePlayer:1`
   - Collects all responses into buffer until timeout

4. **Response Processing** (`src/discovery.rs:138-187`): For each SSDP response:
   - Skip if location already seen (deduplication)
   - Skip if not likely Sonos (early filtering by URN/USN/SERVER)
   - Fetch device description via HTTP
   - Parse XML with `DeviceDescription::from_xml()`
   - Validate with `is_sonos_device()`
   - Extract IP from location URL
   - Yield `DeviceEvent::Found(device)`

5. **Termination**: Iterator returns `None` when all buffered responses processed.

### 3.2 Secondary Flow: Early Termination

When a consumer breaks out of the iterator early:

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  for event   │────▶│    break;    │────▶│  Drop impl   │
│  in get_iter │     │              │     │ discovery.rs │
│              │     │              │     │    191-200   │
└──────────────┘     └──────────────┘     └──────────────┘
                                                 │
                                                 ▼
                                          ┌────────────┐
                                          │ UDP Socket │
                                          │   Closed   │
                                          └────────────┘
```

**Step-by-step**:

1. Consumer calls `break` or drops iterator
2. `Drop::drop()` (`src/discovery.rs:191-200`) is invoked
3. `ssdp_client.take()` ensures UDP socket is closed
4. HTTP client automatically cleaned up by Rust's drop semantics

### 3.3 Error Flow

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│Network Error │────▶│DiscoveryError│────▶│ Continue/    │
│(UDP/HTTP)    │     │  Wrapping    │     │ Skip Device  │
└──────────────┘     └──────────────┘     └──────────────┘

┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ Parse Error  │────▶│DiscoveryError│────▶│ Skip Device  │
│  (XML)       │     │::ParseError  │     │              │
└──────────────┘     └──────────────┘     └──────────────┘
```

**Error handling philosophy**: Discovery is best-effort. Individual device failures should not abort the entire discovery process. Errors are logged internally but silently skipped, allowing the iterator to continue discovering other devices.

---

## 4. Features

### 4.1 Feature: SSDP Discovery

#### What

Discovers UPnP devices on the local network by sending M-SEARCH multicast requests and collecting responses.

#### Why

SSDP (Simple Service Discovery Protocol) is the standard mechanism for UPnP device discovery. Sonos devices advertise themselves as ZonePlayer devices and respond to M-SEARCH requests with their location URLs.

#### How

```rust
// M-SEARCH request format (ssdp.rs:41-50)
let request = format!(
    "M-SEARCH * HTTP/1.1\r\n\
     HOST: 239.255.255.250:1900\r\n\
     MAN: \"ssdp:discover\"\r\n\
     MX: 2\r\n\
     ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
     USER-AGENT: sonos-rs/1.0 UPnP/1.0\r\n\r\n"
);
```

The client binds to an ephemeral port (`0.0.0.0:0`), sends to the SSDP multicast address, and reads responses until timeout.

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Blocking UDP socket | Async with tokio | Simpler API, no runtime dependency for consumers |
| Buffer all responses first | Stream as received | Prevents partial iteration issues with early termination |
| Fixed MX=2 delay | Configurable MX | 2 seconds is standard; timeout controls overall duration |

### 4.2 Feature: Multi-Stage Device Filtering

#### What

Three-stage filtering to identify genuine Sonos devices:
1. SSDP response filtering (URN, USN, SERVER header)
2. HTTP fetch success (reachable devices only)
3. XML validation (manufacturer, device type)

#### Why

SSDP discovery can return many non-Sonos devices (routers, smart TVs, NAS devices). Multi-stage filtering minimizes HTTP requests to non-Sonos devices while ensuring accurate identification.

#### How

```rust
// Stage 1: Early filtering (discovery.rs:79-98)
fn is_likely_sonos(response: &SsdpResponse) -> bool {
    response.urn.contains("ZonePlayer") ||
    response.usn.contains("RINCON") ||
    response.server.as_ref().map_or(false, |s| s.contains("sonos"))
}

// Stage 2: HTTP fetch success (implicit)
// Failed fetches are silently skipped

// Stage 3: XML validation (device.rs:76-80)
fn is_sonos_device(&self) -> bool {
    self.manufacturer.to_lowercase().contains("sonos") ||
    self.device_type.contains("ZonePlayer") ||
    self.device_type.contains("MediaRenderer")
}
```

#### Trade-offs

| Decision | Alternative Considered | Why We Chose This |
|----------|----------------------|-------------------|
| Three-stage filtering | Single XML validation | Reduces HTTP requests to non-Sonos devices |
| Case-insensitive matching | Exact string matching | Handles variations in device responses |
| Skip on failure | Propagate errors | Discovery should be resilient to individual failures |

### 4.3 Feature: Automatic Deduplication

#### What

Ensures each physical device is reported exactly once, even when devices respond multiple times to SSDP requests.

#### Why

Sonos devices often send multiple SSDP responses (for different services, or due to network conditions). Consumers expect one event per device, not multiple.

#### How

```rust
// Deduplication by location URL (discovery.rs:154-158)
if self.seen_locations.contains(&ssdp_response.location) {
    continue;
}
self.seen_locations.insert(ssdp_response.location.clone());
```

Location URL is used as the deduplication key because:
- It's unique per device
- It's available before HTTP fetch (saves network requests)
- It's more reliable than USN which can vary

### 4.4 Feature: Resource Cleanup on Early Termination

#### What

Implements `Drop` trait to ensure UDP sockets and HTTP connections are properly released when an iterator is dropped before completion.

#### Why

Consumers may find the device they need and break out of the iterator early. Without explicit cleanup, UDP sockets could remain open, eventually exhausting system resources.

#### How

```rust
// discovery.rs:191-200
impl Drop for DiscoveryIterator {
    fn drop(&mut self) {
        if let Some(client) = self.ssdp_client.take() {
            drop(client);
        }
    }
}
```

The `Option::take()` pattern ensures the socket is closed exactly once, even if `drop` is called multiple times.

---

## 5. Data Model

### 5.1 Core Data Structures

#### `SsdpResponse`

```rust
// ssdp.rs:11-17
pub(crate) struct SsdpResponse {
    pub location: String,      // URL to device description XML
    pub urn: String,           // ST header: device type URN
    pub usn: String,           // USN header: unique service name
    pub server: Option<String>, // SERVER header: device software info
}
```

**Lifecycle**:
1. **Creation**: Parsed from raw UDP response text by `parse_ssdp_response()`
2. **Mutation**: Immutable after creation
3. **Destruction**: Dropped after processing in iterator

**Memory considerations**: ~200 bytes per response. Buffered responses are held until iteration completes.

#### `DeviceDescription`

```rust
// device.rs:17-35
#[derive(Debug, Deserialize)]
pub struct DeviceDescription {
    pub device_type: String,
    pub friendly_name: String,
    pub manufacturer: String,
    pub manufacturer_url: Option<String>,
    pub model_description: Option<String>,
    pub model_name: String,
    pub model_number: Option<String>,
    pub model_url: Option<String>,
    pub serial_number: Option<String>,
    pub udn: String,
    pub room_name: Option<String>,
    pub display_name: Option<String>,
}
```

**Lifecycle**:
1. **Creation**: Deserialized from XML by `quick_xml::de::from_str()`
2. **Validation**: `is_sonos_device()` checks manufacturer and device type
3. **Conversion**: `to_device()` creates public `Device` type
4. **Destruction**: Temporary, dropped after conversion

**Memory considerations**: Variable size due to strings, typically 500-1000 bytes. Not retained after conversion.

### 5.2 Serialization

| Format | Use Case | Library | Notes |
|--------|----------|---------|-------|
| XML (UPnP) | Device description parsing | `quick-xml` + `serde` | Read-only, Sonos generates |
| HTTP/1.1 text | SSDP response parsing | Manual parsing | Simple header extraction |

---

## 6. Integration Points

### 6.1 Dependencies (Upstream)

| Crate | Purpose | Why This Dependency |
|-------|---------|---------------------|
| `reqwest` (blocking) | HTTP client for device descriptions | Well-maintained, supports timeouts, handles TLS |
| `quick-xml` | XML deserialization | Fast, serde-compatible, handles UPnP namespaces |
| `serde` | Struct serialization derive | Standard Rust serialization framework |

### 6.2 Dependents (Downstream)

| Crate | How It Uses Us | API Stability Notes |
|-------|---------------|---------------------|
| `sonos-api` | Provides `Device` IP for operation execution | `Device.ip_address` and `Device.port` are stable |
| `sonos-state` | Initial device population for `StateManager` | `Device.id` format is stable (UDN) |
| `sonos-stream` | Device info for subscription management | Full `Device` struct consumed |
| `sonos-event-manager` | Device identification | `Device.id` used as key |
| `sonos-sdk` | Re-exports discovery API | All public API is stable |

### 6.3 External Systems

```
┌─────────────────┐         ┌─────────────────┐
│ sonos-discovery │◀───────▶│  Sonos Device   │
│                 │  SSDP   │  (UDP 1900)     │
│                 │  HTTP   │  (TCP 1400)     │
└─────────────────┘         └─────────────────┘
```

**Protocol**:
- SSDP: UDP multicast to 239.255.255.250:1900
- HTTP: GET request to device port 1400

**Authentication**: None required for discovery

**Error handling**: Network timeouts and unreachable devices are silently skipped

**Retry strategy**: No automatic retries. Consumers should call `get()` again if needed.

---

## 7. Error Handling

### 7.1 Error Types

```rust
// error.rs:9-19
#[derive(Debug)]
pub enum DiscoveryError {
    /// Network-related errors (socket creation, HTTP requests)
    NetworkError(String),
    /// Parsing errors (XML, SSDP response)
    ParseError(String),
    /// Operation timed out waiting for responses
    Timeout,
    /// Invalid device data or non-Sonos device detected
    InvalidDevice(String),
}
```

### 7.2 Error Philosophy

| Principle | Implementation | Rationale |
|-----------|---------------|-----------|
| Best-effort discovery | Errors skip individual devices | One bad device shouldn't abort discovery |
| Graceful degradation | `get_iter_with_timeout` returns empty iterator on init failure | No panics in public API |
| Actionable messages | Error strings include context | Helps debugging network issues |

### 7.3 Error Recovery

| Error | Recoverable | Recovery Strategy |
|-------|-------------|-------------------|
| `NetworkError` (socket bind) | No | Return empty iterator, log error |
| `NetworkError` (HTTP) | Yes | Skip device, continue discovery |
| `ParseError` (SSDP) | Yes | Skip response, continue iteration |
| `ParseError` (XML) | Yes | Skip device, continue discovery |
| `Timeout` | N/A | Normal completion (not an error in practice) |
| `InvalidDevice` | Yes | Skip device (filtered out) |

---

## 8. Testing Strategy

### 8.1 Testing Philosophy

```
                    ┌───────────────────┐
                    │  Integration/E2E  │  Real network tests
                    └─────────┬─────────┘
              ┌───────────────┴───────────────┐
              │     Fixture-Based Tests       │  XML parsing with mocks
              └───────────────┬───────────────┘
    ┌─────────────────────────┴─────────────────────────┐
    │                   Unit Tests                       │  Pure functions
    └────────────────────────────────────────────────────┘
```

### 8.2 Unit Tests

**Location**: `src/ssdp.rs`, `src/device.rs` (inline `#[cfg(test)]`)

**What to test**:
- [x] SSDP response parsing (valid, invalid, case-insensitive headers)
- [x] Header value extraction
- [x] XML parsing for various device types
- [x] Sonos device identification logic
- [x] IP extraction from URLs
- [x] Device type conversion

**Example**:
```rust
#[test]
fn test_parse_ssdp_response_valid() {
    let response = "HTTP/1.1 200 OK\r\n\
        LOCATION: http://192.168.1.100:1400/xml/device_description.xml\r\n\
        ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
        USN: uuid:RINCON_000E58A0123456::...\r\n";

    let parsed = parse_ssdp_response(response).unwrap();
    assert_eq!(parsed.location, "http://192.168.1.100:1400/xml/device_description.xml");
}
```

### 8.3 Component Tests

**Location**: `tests/fixture_based_integration.rs`

**What to test**:
- [x] Device parsing with real captured XML fixtures
- [x] Sonos device identification across all model types
- [x] Non-Sonos device filtering
- [x] HTTP mock server integration
- [x] Error handling for invalid XML

### 8.4 Integration Tests

**Location**: `tests/discovery_integration.rs`, `tests/resource_cleanup.rs`

**Prerequisites**:
- [x] May require Sonos devices on network (tests pass with 0 devices)
- [x] No environment variables needed

**What to test**:
- [x] Full discovery flow with iterator API
- [x] Deduplication logic
- [x] Early iterator termination
- [x] Convenience function behavior
- [x] Multiple sequential discoveries
- [x] Resource cleanup on drop

### 8.5 Test Fixtures & Mocks

| Dependency | Mock Strategy | Location |
|------------|--------------|----------|
| Sonos device XML | Pre-captured fixtures | `tests/fixtures/*.xml` |
| HTTP responses | mockito server | `tests/fixture_based_integration.rs` |
| SSDP responses | Synthetic generation | `tests/helpers/mod.rs` |

**Available fixtures**:
- `sonos_one_device.xml` - Sonos One (S18)
- `sonos_play1_device.xml` - Sonos Play:1 (S1)
- `sonos_playbar_device.xml` - Sonos Playbar (S9)
- `sonos_amp_device.xml` - Sonos Amp (S16)
- `sonos_roam_device.xml` - Sonos Roam 2 (S54)
- `minimal_sonos_device.xml` - Minimal valid device
- `non_sonos_router_device.xml` - Non-Sonos UPnP device

---

## 9. Performance

### 9.1 Performance Goals

| Metric | Target | Rationale |
|--------|--------|-----------|
| Discovery latency | < 5 seconds typical | Network dependent, configurable via timeout |
| Memory per device | < 2KB | Minimal metadata storage |
| Socket usage | 1 UDP + N HTTP connections | N = number of potential Sonos devices |

### 9.2 Critical Paths

1. **SSDP Response Collection** (`src/ssdp.rs:79-111`)
   - **Complexity**: O(n) where n = number of responses
   - **Bottleneck**: Network timeout (blocks entire collection)
   - **Optimization**: Responses buffered to minimize timeout impact

2. **HTTP Fetch Loop** (`src/discovery.rs:138-187`)
   - **Complexity**: O(m) where m = unique locations
   - **Bottleneck**: Sequential HTTP requests
   - **Optimization**: Early filtering reduces HTTP requests to likely Sonos devices

### 9.3 Resource Management

| Resource | Acquisition | Release | Pooling |
|----------|-------------|---------|---------|
| UDP socket | `DiscoveryIterator::new()` | `Drop::drop()` | No - one per discovery |
| HTTP connections | On-demand per request | After response | Yes - reqwest internal pool |
| SSDP response buffer | First `next()` call | When iterator dropped | No - transient |

---

## 10. Security Considerations

### 10.1 Threat Model

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| Malicious SSDP responses | Low | Low | XML parsing validates structure, filtering validates content |
| Spoofed device descriptions | Low | Medium | Consumers should verify device IDs match expectations |
| DoS via many responses | Low | Low | Timeout limits discovery duration |
| Man-in-the-middle (HTTP) | Medium | Low | Discovery is informational only, no secrets transmitted |

### 10.2 Sensitive Data

| Data Type | Sensitivity | Protection |
|-----------|-------------|------------|
| Device IPs | Low | Local network only |
| Device names/rooms | Low | User-configured, not secrets |
| MAC addresses | Low | In XML but not exposed in public API |

### 10.3 Input Validation

| Input Source | Validation | Location |
|--------------|------------|----------|
| SSDP responses | Required headers present | `src/ssdp.rs:114-146` |
| Device XML | Schema validation via serde | `src/device.rs:47-52` |
| Location URLs | HTTP scheme check (implicit via reqwest) | `src/discovery.rs:101-112` |

---

## 11. Observability

### 11.1 Logging

This crate does not currently emit logs. Future versions may add `tracing` support.

| Level | What Would Be Logged | Example |
|-------|---------------------|---------|
| `debug` | SSDP responses received | "Received SSDP response from 192.168.1.100" |
| `debug` | Device validation results | "Device at 192.168.1.100 identified as Sonos One" |
| `trace` | Raw XML content | "Device description XML: ..." |
| `warn` | Skipped devices | "Skipping device: HTTP fetch failed" |

---

## 12. Configuration

### 12.1 Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `timeout` | `Duration` | 3 seconds | Maximum time to wait for SSDP responses and HTTP requests |

Configuration is provided via function parameters rather than environment variables or config files.

```rust
// Default timeout
let devices = get();

// Custom timeout
let devices = get_with_timeout(Duration::from_secs(10));
```

---

## 13. Migration & Compatibility

### 13.1 API Stability

| API | Stability | Notes |
|-----|-----------|-------|
| `get()` | Stable | Core API, unlikely to change |
| `get_with_timeout()` | Stable | Core API |
| `get_iter()` | Stable | Core API |
| `get_iter_with_timeout()` | Stable | Core API |
| `Device` struct | Stable | Fields may be added (non-breaking) |
| `DeviceEvent` enum | Stable | Variants may be added (match with `_`) |
| `DiscoveryError` | Stable | Variants may be added |
| `device::DeviceDescription` | Semi-stable | Public for testing, internal use discouraged |

### 13.2 Breaking Changes

**Policy**: Semantic versioning. Breaking changes only in major versions.

**Current deprecations**: None

---

## 14. Known Limitations

### 14.1 Current Limitations

| Limitation | Impact | Workaround | Planned Fix |
|------------|--------|------------|-------------|
| Blocking I/O only | Can't integrate with async runtimes | Use `spawn_blocking` | No change planned (design decision) |
| No IPv6 support | Won't find devices on IPv6-only networks | Use IPv4 | Low priority (Sonos uses IPv4) |
| No device change notification | Won't detect devices added/removed | Re-run discovery | Use `sonos-state` for monitoring |
| Sequential HTTP fetches | Slower with many devices | N/A | Could parallelize (low priority) |

### 14.2 Technical Debt

| Debt Item | Location | Severity | Remediation Plan |
|-----------|----------|----------|------------------|
| No tracing/logging | All files | Low | Add tracing spans for debugging |
| Hardcoded port 1400 | `src/device.rs:68` | Low | Extract to constant |

---

## 15. Future Considerations

### 15.1 Planned Enhancements

| Enhancement | Priority | Rationale | Dependencies |
|-------------|----------|-----------|--------------|
| Add tracing spans | P2 | Better debugging for network issues | `tracing` crate |
| Parallel HTTP fetches | P2 | Faster discovery with many devices | None |

### 15.2 Open Questions

- [ ] **Should we support IPv6?**: Sonos devices currently use IPv4, but this may change in the future.
- [ ] **Should we cache discovered devices?**: Could speed up repeated discoveries, but adds complexity and staleness concerns.

---

## Appendix

### A. Glossary

| Term | Definition |
|------|------------|
| SSDP | Simple Service Discovery Protocol - UDP-based multicast for UPnP device discovery |
| UPnP | Universal Plug and Play - Protocol suite for device discovery and control |
| M-SEARCH | SSDP message type for discovering devices |
| UDN | Unique Device Name - UUID identifying a specific device |
| RINCON | Sonos device identifier prefix (e.g., RINCON_7828CA0E1E1801400) |
| ZonePlayer | Sonos UPnP device type for speakers |

### B. References

- [UPnP Device Architecture 1.0](http://upnp.org/specs/arch/UPnP-arch-DeviceArchitecture-v1.0.pdf)
- [SSDP Draft Specification](https://tools.ietf.org/html/draft-cai-ssdp-v1-03)
- [Sonos UPnP Documentation](https://developer.sonos.com/) (requires account)

### C. Changelog

| Date | Author | Change |
|------|--------|--------|
| 2024-01-14 | Claude | Initial specification |
