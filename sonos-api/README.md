# sonos-api

A stateless, type-safe Rust library for constructing requests to Sonos speakers and handling their responses.

## Overview

The `sonos-api` crate provides a high-level, stateless layer for interacting with Sonos devices through their UPnP/SOAP interface. It focuses on:

- **Request Construction**: Type-safe builders for SOAP payloads
- **Response Parsing**: Structured parsing of XML responses into Rust types
- **Operation Modeling**: Each UPnP action is modeled as a distinct operation with its own request/response types
- **Service Organization**: Operations are grouped by UPnP service (AVTransport, RenderingControl, etc.)

This crate is **stateless** - it doesn't manage connections, maintain device state, or handle networking. It purely focuses on the request/response transformation layer.

## Architecture

The crate is built around the `SonosOperation` trait, which defines a common interface for all Sonos UPnP operations:

```rust
pub trait SonosOperation {
    type Request: Serialize;
    type Response: for<'de> Deserialize<'de>;
    
    const SERVICE: Service;
    const ACTION: &'static str;
    
    fn build_payload(request: &Self::Request) -> String;
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError>;
}
```

Each operation implements this trait to provide:
- Type-safe request and response structures
- SOAP payload construction from request data
- XML response parsing into structured data

## Supported Services

The crate currently supports operations for these UPnP services:

- **AVTransport**: Playback control (play, pause, stop, transport info)
- **RenderingControl**: Volume and audio settings
- **DeviceProperties**: Device information and capabilities
- **ZoneGroupTopology**: Multi-room grouping and topology
- **GroupRenderingControl**: Group-level audio control

## Usage

### Using the SonosClient (Recommended)

The easiest way to use the API is through the `SonosClient`, which handles all the SOAP communication for you:

```rust
use sonos_api::{SonosClient, operations::av_transport::{GetTransportInfoOperation, GetTransportInfoRequest}};

// Create a client
let client = SonosClient::new();

// Create a request
let request = GetTransportInfoRequest {
    instance_id: 0,
};

// Execute the operation against a device
let response = client.execute::<GetTransportInfoOperation>("192.168.1.100", &request)?;
println!("Current state: {:?}", response.current_transport_state);
```

### Working with Different Operations

```rust
use sonos_api::{SonosClient, operations::{
    av_transport::{PlayOperation, PlayRequest, PauseOperation, PauseRequest},
    rendering_control::{GetVolumeOperation, GetVolumeRequest, SetVolumeOperation, SetVolumeRequest},
}};

let client = SonosClient::new();
let device_ip = "192.168.1.100";

// Play music
let play_request = PlayRequest {
    instance_id: 0,
    speed: "1".to_string(),
};
client.execute::<PlayOperation>(device_ip, &play_request)?;

// Get current volume
let volume_request = GetVolumeRequest {
    instance_id: 0,
    channel: "Master".to_string(),
};
let volume_response = client.execute::<GetVolumeOperation>(device_ip, &volume_request)?;
println!("Current volume: {}", volume_response.current_volume);

// Set volume to 50%
let set_volume_request = SetVolumeRequest {
    instance_id: 0,
    channel: "Master".to_string(),
    desired_volume: 50,
};
client.execute::<SetVolumeOperation>(device_ip, &set_volume_request)?;

// Pause playback
let pause_request = PauseRequest {
    instance_id: 0,
};
client.execute::<PauseOperation>(device_ip, &pause_request)?;
```

### Low-Level Operation Usage (Advanced)

For advanced use cases, you can work directly with operations without the client:

```rust
use sonos_api::operations::av_transport::{GetTransportInfoOperation, GetTransportInfoRequest};
use sonos_api::SonosOperation;

// Create a request
let request = GetTransportInfoRequest {
    instance_id: 0,
};

// Build the SOAP payload
let payload = GetTransportInfoOperation::build_payload(&request);
// Returns: "<InstanceID>0</InstanceID>"

// Parse a response (after receiving XML from the device)
let xml = /* parsed XML response */;
let response = GetTransportInfoOperation::parse_response(&xml)?;
println!("Current state: {:?}", response.current_transport_state);
```

### Working with Different Operations

```rust
use sonos_api::operations::av_transport::{PlayOperation, PlayRequest};
use sonos_api::operations::rendering_control::{GetVolumeOperation, GetVolumeRequest};

// Play operation
let play_request = PlayRequest {
    instance_id: 0,
    speed: "1".to_string(),
};
let play_payload = PlayOperation::build_payload(&play_request);

// Volume operation  
let volume_request = GetVolumeRequest {
    instance_id: 0,
    channel: "Master".to_string(),
};
let volume_payload = GetVolumeOperation::build_payload(&volume_request);
```

### Error Handling

The crate provides structured error handling through the `ApiError` type:

```rust
use sonos_api::{SonosClient, ApiError, operations::av_transport::{GetTransportInfoOperation, GetTransportInfoRequest}};

let client = SonosClient::new();
let request = GetTransportInfoRequest { instance_id: 0 };

match client.execute::<GetTransportInfoOperation>("192.168.1.100", &request) {
    Ok(response) => println!("Success: {:?}", response),
    Err(ApiError::NetworkError(msg)) => eprintln!("Network error: {}", msg),
    Err(ApiError::ParseError(msg)) => eprintln!("Parse error: {}", msg),
    Err(ApiError::SoapFault(code)) => eprintln!("Device returned error code: {}", code),
    Err(e) => eprintln!("Other error: {}", e),
}
```

## Integration with Other Crates

This crate is designed to work with other crates in the Sonos SDK ecosystem:

- **soap-client**: Handles the actual SOAP communication and networking
- **sonos-discovery**: Discovers devices on the network
- **sonos-stream**: Manages event subscriptions and real-time updates

The typical flow is:
1. Use `sonos-discovery` to find devices
2. Use `sonos-api` to construct requests and parse responses
3. Use `soap-client` to send the requests over the network
4. Use `sonos-stream` for real-time event handling

## Design Principles

- **Stateless**: No connection management or device state tracking
- **Type Safety**: Strong typing for all requests and responses
- **Separation of Concerns**: Pure request/response transformation
- **Extensible**: Easy to add new operations following the same patterns
- **Error Transparency**: Clear error types for different failure modes

## Adding New Operations

To add a new operation:

1. Create a new module under the appropriate service directory
2. Define request and response types with proper serde annotations
3. Implement the `SonosOperation` trait
4. Add comprehensive tests for payload construction and response parsing

Example structure:
```rust
pub struct MyOperation;

#[derive(Serialize)]
pub struct MyRequest {
    // request fields
}

#[derive(Deserialize)]
pub struct MyResponse {
    // response fields  
}

impl SonosOperation for MyOperation {
    type Request = MyRequest;
    type Response = MyResponse;
    
    const SERVICE: Service = Service::AVTransport;
    const ACTION: &'static str = "MyAction";
    
    fn build_payload(request: &Self::Request) -> String {
        // construct XML payload
    }
    
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError> {
        // parse XML response
    }
}
```

## Testing

The crate includes comprehensive tests for all operations, covering:
- Payload construction with various input parameters
- Response parsing with valid XML
- Error handling for malformed or missing XML elements
- Edge cases and validation scenarios

Run tests with:
```bash
cargo test -p sonos-api
```