# sonos-api

A stateless, type-safe Rust library for constructing requests to Sonos speakers and handling their responses.

## Overview

The `sonos-api` crate provides a high-level, stateless layer for interacting with Sonos devices through their UPnP/SOAP interface. It focuses on:

- **Request Construction**: Type-safe builders for SOAP payloads
- **Response Parsing**: Structured parsing of XML responses into Rust types
- **Operation Modeling**: Each UPnP action is modeled as a distinct operation with its own request/response types
- **Service Organization**: Operations are grouped by UPnP service (AVTransport, RenderingControl, etc.)

This crate is **stateless** - it doesn't manage connections, maintain device state, or handle networking. It purely focuses on the request/response transformation layer.

For a whole-system, reactive API with discovery, grouping and live property updates, use
[`sonos-sdk`](https://crates.io/crates/sonos-sdk) instead.

## Architecture

Every operation implements the `UPnPOperation` trait:

```rust,ignore
pub trait UPnPOperation {
    type Request: Serialize + Validate;
    type Response: for<'de> Deserialize<'de>;

    const SERVICE: Service;
    const ACTION: &'static str;

    fn build_payload(request: &Self::Request) -> Result<String, ValidationError>;
    fn parse_response(xml: &str) -> Result<Self::Response, ApiError>;
}
```

Each operation provides:
- Type-safe request and response structures
- A validated SOAP payload built from the request data
- XML response parsing into structured data

Operations are not called directly. A request is wrapped in an `OperationBuilder`, which applies
a `ValidationLevel` and an optional timeout and produces a `ComposableOperation`; that is what
`SonosClient::execute_enhanced` sends to a device.

`parse_response` takes the raw SOAP response body as `&str`. `soap-client` returns text
rather than a parsed DOM: it handles transport and SOAP-fault detection, so an `Ok` body is
guaranteed to be a fault-free `<{action}Response>` envelope, while response *shape* is
service-specific and belongs here.

## Supported Services

`Service` has five variants, each with its own module under `sonos_api::services`:

| `Service` variant | Module | Scope | Covers |
|-------------------|--------|-------|--------|
| `AVTransport` | `av_transport` | Per-coordinator | Playback, seeking, queue, sleep timer, alarms |
| `RenderingControl` | `rendering_control` | Per-speaker | Volume, mute, bass, treble, loudness |
| `GroupRenderingControl` | `group_rendering_control` | Per-coordinator | Group volume, group mute, volume snapshots |
| `ZoneGroupTopology` | `zone_group_topology` | Per-network | Zone group state |
| `GroupManagement` | `group_management` | Per-coordinator | Group membership, track buffering reports |

Each module also exposes `subscribe()` and `subscribe_with_timeout()` helpers for UPnP eventing,
plus event types and parsers under its `events` submodule.

## Usage

### Executing an operation

Each operation ships a convenience constructor named after it in snake_case, returning an
`OperationBuilder`:

```rust
use sonos_api::services::av_transport;
use sonos_api::SonosClient;

let client = SonosClient::new();

let operation = av_transport::get_transport_info_operation().build()?;
let response = client.execute_enhanced("192.168.1.100", operation)?;
println!("Current state: {}", response.current_transport_state);
```

### Working with different operations

```rust
use sonos_api::services::{av_transport, rendering_control};
use sonos_api::SonosClient;

let client = SonosClient::new();
let device_ip = "192.168.1.100";

// Play music
let play = av_transport::play_operation("1".to_string()).build()?;
client.execute_enhanced(device_ip, play)?;

// Get current volume
let get_volume = rendering_control::get_volume_operation("Master".to_string()).build()?;
let volume = client.execute_enhanced(device_ip, get_volume)?;
println!("Current volume: {}", volume.current_volume);

// Set volume to 50%
let set_volume = rendering_control::set_volume_operation("Master".to_string(), 50).build()?;
client.execute_enhanced(device_ip, set_volume)?;

// Pause playback
let pause = av_transport::pause_operation().build()?;
client.execute_enhanced(device_ip, pause)?;
```

### Building a request directly

The convenience constructors default `instance_id` to `0`. To set it — or any other field —
explicitly, construct the request struct and hand it to `OperationBuilder`. The generated
request type is the operation name with a `Request` suffix:

```rust
use sonos_api::services::rendering_control::{SetVolumeOperation, SetVolumeOperationRequest};
use sonos_api::{OperationBuilder, SonosClient};

let client = SonosClient::new();

let request = SetVolumeOperationRequest {
    instance_id: 0,
    channel: "Master".to_string(),
    desired_volume: 50,
};

let operation = OperationBuilder::<SetVolumeOperation>::new(request).build()?;
client.execute_enhanced("192.168.1.100", operation)?;
```

### Validation and timeouts

`build()` runs the request's `Validate` implementation at the configured level and fails before
any network traffic. `ValidationLevel::None` skips it:

```rust
use sonos_api::operation::ValidationLevel;
use sonos_api::services::rendering_control;
use std::time::Duration;

// Rejected by SetVolume's range check: volume must be 0-100.
assert!(rendering_control::set_volume_operation("Master".to_string(), 150)
    .build()
    .is_err());

let operation = rendering_control::set_volume_operation("Master".to_string(), 50)
    .with_validation(ValidationLevel::Basic)
    .with_timeout(Duration::from_secs(5))
    .build()?;
```

### Event Subscriptions

UPnP event subscriptions are managed separately from control operations. They target a service's
`/Event` endpoint, while operations target `/Control`. `subscribe` returns a
`ManagedSubscription` that tracks expiry and cleans up:

```rust
use sonos_api::{Service, SonosClient};

let client = SonosClient::new();
let device_ip = "192.168.1.100";

// Subscribe to AVTransport events (default timeout: 1800 seconds)
let subscription = client.subscribe(
    device_ip,
    Service::AVTransport,
    "http://192.168.1.50:8080/callback",
)?;
println!("Subscribed with SID: {}", subscription.subscription_id());

// Renew before it expires
if subscription.needs_renewal() {
    subscription.renew()?;
}

// Clean up when done
subscription.unsubscribe()?;
```

A custom timeout is available through `subscribe_with_timeout`, and each service module offers
the same pair scoped to itself:

```rust
use sonos_api::services::av_transport;
use sonos_api::SonosClient;

let client = SonosClient::new();

let subscription = av_transport::subscribe_with_timeout(
    &client,
    "192.168.1.100",
    "http://192.168.1.50:8080/callback",
    3600,
)?;
```

### Parsing an event body

Delivered NOTIFY bodies are parsed by the service's event parser:

```rust
use sonos_api::events::EventParser;
use sonos_api::services::av_transport::AVTransportEventParser;

let xml = r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
    <e:property>
        <LastChange>&lt;Event&gt;&lt;InstanceID val="0"&gt;
            &lt;TransportState val="PLAYING"/&gt;
        &lt;/InstanceID&gt;&lt;/Event&gt;</LastChange>
    </e:property>
</e:propertyset>"#;

let event = AVTransportEventParser.parse_upnp_event(xml)?;
assert_eq!(event.transport_state().as_deref(), Some("PLAYING"));
```

### Error Handling

The crate provides structured error handling through the `ApiError` type:

```rust
use sonos_api::services::av_transport;
use sonos_api::{ApiError, SonosClient};

let client = SonosClient::new();
let operation = av_transport::get_transport_info_operation().build()?;

match client.execute_enhanced("192.168.1.100", operation) {
    Ok(response) => println!("Success: {}", response.current_transport_state),
    Err(ApiError::NetworkError(msg)) => eprintln!("Network error: {msg}"),
    Err(ApiError::ParseError(msg)) => eprintln!("Parse error: {msg}"),
    Err(ApiError::SoapFault(code)) => eprintln!("Device returned error code: {code}"),
    Err(ApiError::InvalidParameter(msg)) => eprintln!("Invalid parameter: {msg}"),
    Err(ApiError::SubscriptionError(msg)) => eprintln!("Subscription error: {msg}"),
    Err(ApiError::DeviceError(msg)) => eprintln!("Device error: {msg}"),
}
```

## Integration with Other Crates

This crate sits in the middle of the Sonos SDK workspace:

- **soap-client**: Handles the actual SOAP communication and networking
- **sonos-discovery**: Discovers devices on the network
- **sonos-stream**: Manages event subscriptions and real-time updates

The typical flow is:
1. Use `sonos-discovery` to find devices
2. Use `sonos-api` to construct requests and parse responses
3. `soap-client` sends the requests over the network
4. Use `sonos-stream` for real-time event handling

## Design Principles

- **Stateless**: No connection management or device state tracking
- **Type Safety**: Strong typing for all requests and responses
- **Separation of Concerns**: Pure request/response transformation
- **Extensible**: Easy to add new operations following the same patterns
- **Error Transparency**: Clear error types for different failure modes

## Adding New Operations

Operations are declared with macros that generate the request struct, the response struct, the
`UPnPOperation` implementation and the snake_case constructor. `define_upnp_operation!` covers
actions with no out-arguments; `define_operation_with_response!` covers actions that return data:

```rust,ignore
define_upnp_operation! {
    operation: MyActionOperation,
    action: "MyAction",
    service: AVTransport,
    request: {
        parameter: String,
    },
    response: (),
    payload: |req| format!(
        "<InstanceID>{}</InstanceID><Parameter>{}</Parameter>",
        req.instance_id, req.parameter
    ),
    parse: |_xml| Ok(()),
}

define_operation_with_response! {
    operation: GetMyInfoOperation,
    action: "GetMyInfo",
    service: AVTransport,
    request: {},
    response: GetMyInfoResponse {
        info_field: String,
    },
    xml_mapping: {
        info_field: "InfoField",
    },
}

impl Validate for MyActionOperationRequest {
    fn validate_basic(&self) -> Result<(), ValidationError> {
        if self.parameter.is_empty() {
            return Err(ValidationError::invalid_value("parameter", &self.parameter));
        }
        Ok(())
    }
}
```

See [`src/services/README.md`](src/services/README.md) for the full walkthrough, including
`request_xml_mapping:` for multi-word UPnP argument names and how to wire up event parsing.

## Testing

The crate includes tests for all operations, covering:
- Payload construction with various input parameters
- Response parsing with valid XML
- Error handling for malformed or missing XML elements
- Edge cases and validation scenarios

Run tests with:
```bash
cargo test -p sonos-api
```

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.
