# sonos-parser

A modular XML parsing library for Sonos UPnP service responses and events.

## Overview

The `sonos-parser` crate provides XML parsing capabilities specifically designed for Sonos UPnP service responses and events. It extracts and centralizes parsing logic to make it reusable across the entire Sonos SDK workspace.

## Features

- **Modular Design**: Service-specific parsers organized by UPnP service type
- **Common Utilities**: Shared XML processing utilities and data structures
- **DIDL-Lite Support**: Built-in support for DIDL-Lite metadata parsing
- **Error Handling**: Comprehensive error types for different parsing scenarios
- **Namespace Handling**: Automatic XML namespace stripping for simplified parsing

## Usage

```rust
use sonos_parser::AVTransportParser;

// Parse AVTransport UPnP event
let parser = AVTransportParser::from_xml(xml_content)?;
let state = parser.transport_state();
let metadata = parser.track_metadata();
```

## Architecture

- `common/`: Shared utilities and data structures
- `services/`: Service-specific parsers (AVTransport, etc.)
- `error.rs`: Centralized error handling

## Dependencies

- `serde`: Serialization framework
- `quick-xml`: XML parsing engine
- `thiserror`: Error handling