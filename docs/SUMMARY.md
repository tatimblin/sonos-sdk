# Sonos SDK Architecture Summary

This document provides an overview of each Rust crate in the Sonos SDK workspace and describes how they work together as a complete system. For detailed specifications, see the individual spec documents linked below.

## Table of Contents

- [Crate Specifications](#crate-specifications)
- [System Design](#system-design)
  - [Architecture Diagram](#architecture-diagram)
  - [Data Flow](#data-flow)
  - [Key Design Patterns](#key-design-patterns)
- [Crate Size Reference](#crate-size-reference)

---

## Crate Specifications

### Public APIs

| Crate | Purpose | Specification |
|-------|---------|---------------|
| **sonos-sdk** | DOM-like API for Sonos device control (recommended entry point) | [View Spec](specs/sonos-sdk.md) |
| **sonos-api** | Type-safe API layer for UPnP operations | [View Spec](specs/sonos-api.md) |
| **sonos-discovery** | Network device discovery using SSDP | [View Spec](specs/sonos-discovery.md) |

### Internal Crates

| Crate | Purpose | Specification |
|-------|---------|---------------|
| **sonos-state** | Reactive state management layer | [View Spec](specs/sonos-state.md) |
| **sonos-stream** | Low-level event streaming with transparent fallback | [View Spec](specs/sonos-stream.md) |
| **sonos-event-manager** | Reference-counted subscription management | [View Spec](specs/sonos-event-manager.md) |
| **callback-server** | Generic HTTP server for UPnP event callbacks | [View Spec](specs/callback-server.md) |
| **soap-client** | Low-level SOAP transport layer | [View Spec](specs/soap-client.md) |

---

## System Design

### Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                           End Users                                  │
└────────────────────────────────┬────────────────────────────────────┘
                                 │
                                 │ User-Facing API
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      sonos-sdk (DOM-like API)                        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │  SonosSystem → Speaker → Property Handles                     │  │
│  │                                                               │  │
│  │  speaker.volume.get()    → Cached value                       │  │
│  │  speaker.volume.fetch()  → API call + update cache            │  │
│  │  speaker.volume.watch()  → Reactive UPnP stream               │  │
│  └───────────────────────────────────────────────────────────────┘  │
└──────────┬─────────────────────┬───────────────────────┬────────────┘
           │                     │                       │
           │                     │                       │
           ▼                     ▼                       ▼
┌──────────────────┐  ┌──────────────────┐  ┌──────────────────────────┐
│ sonos-discovery  │  │   sonos-api      │  │     sonos-state          │
│ (Device Discovery│  │   (Operations)   │  │  (Reactive State)        │
│                  │  │                  │  │                          │
│ • SSDP multicast │  │ • SonosOperation │  │ • StateManager           │
│ • Device enum    │  │ • Type-safe APIs │  │ • PropertyWatcher        │
│ • Deduplication  │  │ • Service groups │  │ • Event Decoders         │
└──────────────────┘  └────────┬─────────┘  └────────────┬─────────────┘
                               │                         │
                               │         Internal APIs   │
                               │                         ▼
                               │       ┌─────────────────────────────────┐
                               │       │    sonos-event-manager          │
                               │       │    (Reference Counting)         │
                               │       │                                 │
                               │       │  • Subscription lifecycle       │
                               │       │  • Ref count management         │
                               │       │  • Automatic cleanup            │
                               │       └────────────┬────────────────────┘
                               │                    │
                               │                    ▼
                               │       ┌─────────────────────────────────┐
                               │       │      sonos-stream               │
                               │       │      (Event Streaming)          │
                               │       │                                 │
                               │       │  • UPnP/Polling switching       │
                               │       │  • Firewall detection           │
                               │       │  • Event enrichment             │
                               │       └───────────┬─────────────────────┘
                               │                   │
                               │    ┌──────────────┴──────────────┐
                               │    │                             │
                               │    ▼                             ▼
                               │  ┌───────────────────┐  ┌────────────────┐
                               │  │  callback-server  │  │                │
                               │  │  (HTTP Server)    │  │                │
                               │  │                   │  │                │
                               │  │ • NOTIFY handling │  │                │
                               │  │ • Event routing   │  │                │
                               │  └─────────┬─────────┘  │                │
                               │            │            │                │
                               └────────────┼────────────┘                │
                                            │                             │
                                            ▼                             │
                            ┌─────────────────────────────┐               │
                            │       soap-client           │◄──────────────┘
                            │       (SOAP Transport)      │
                            │                             │
                            │  • Singleton pattern        │
                            │  • Shared HTTP agent        │
                            │  • Connection pooling       │
                            │  • SOAP envelope building   │
                            └──────────────┬──────────────┘
                                           │
                                           ▼
                            ┌─────────────────────────────┐
                            │      Sonos Devices          │
                            │      (Port 1400)            │
                            └─────────────────────────────┘
```

### Data Flow

#### 1. Device Discovery Flow (SonosSystem::new)
```
SonosSystem::new()
    │
    ▼ sonos_discovery::get()
sonos-discovery
    │ SSDP M-SEARCH multicast
    ▼
Network (224.0.0.250:1900)
    │ SSDP responses
    ▼
sonos-discovery
    │ HTTP GET device description XML
    ▼
Sonos Devices (port 1400)
    │ Device metadata
    ▼
Vec<Device>
    │
    ▼
sonos-sdk creates Speaker handles with property accessors
    │
    ▼
SonosSystem ready for use
```

#### 2. Property Get Flow (Cached)
```
speaker.volume.get()
    │
    ▼
VolumeHandle
    │
    ▼
sonos-state (StateManager)
    │ Read from StateStore
    ▼
Option<Volume> returned instantly
```

#### 3. Property Fetch Flow (Fresh API Call)
```
speaker.volume.fetch().await
    │
    ▼
VolumeHandle
    │ Build GetVolume operation
    ▼
sonos-api (SonosClient)
    │ Execute operation
    ▼
soap-client
    │ SOAP request
    ▼
Sonos Device
    │ SOAP response
    ▼
sonos-api
    │ Parse response
    ▼
VolumeHandle
    │ Update sonos-state cache
    ▼
sonos-state (StateManager)
    │ Updates StateStore + notifies watchers
    ▼
Volume returned to caller
```

#### 4. Property Watch Flow (Reactive)
```
speaker.volume.watch().await
    │
    ▼
VolumeHandle
    │
    ▼
sonos-state (StateManager.watch_property)
    │ Checks if RenderingControl subscription exists
    ▼
sonos-event-manager
    │ Reference count: 0→1, creates subscription
    ▼
sonos-stream (EventBroker)
    │ Registers speaker/service pair
    ▼
callback-server                           sonos-api
    │ Binds callback URL                      │ Subscribe request
    ▼                                         ▼
soap-client ─────────────────────────────► Sonos Device
                                               │ NOTIFY events
                                               ▼
callback-server ◄──────────────────────────────┘
    │ Route to subscription handler
    ▼
sonos-stream
    │ Enrich event with metadata
    ▼
sonos-event-manager
    │ Distribute to consumers
    ▼
sonos-state (Decoders)
    │ Parse to property updates
    ▼
StateStore
    │ Update watch channels
    ▼
PropertyWatcher<Volume> receives update
```

#### 5. Firewall Fallback Flow
```
sonos-stream detects no callbacks received
    │
    ▼
FirewallDetectionCoordinator marks firewall blocked
    │
    ▼
EventBroker switches to polling mode
    │
    ▼
sonos-api (GetVolume, GetTransportInfo, etc.)
    │ Periodic polling requests
    ▼
soap-client → Sonos Device
    │ Response data
    ▼
sonos-stream
    │ Convert to EnrichedEvent (source: Polling)
    ▼
Same downstream flow as UPnP events
```

### Key Design Patterns

#### 1. DOM-like Property API (sonos-sdk)
Properties are accessed directly on speaker objects with three consistent methods:

```rust
// Cached read (instant)
let volume = speaker.volume.get();

// Fresh API call + cache update
let volume = speaker.volume.fetch().await?;

// Reactive stream
let mut watcher = speaker.volume.watch().await?;
```

#### 2. Singleton Pattern (soap-client)
All SOAP clients share a single `LazyLock<SoapClient>` instance with pooled HTTP connections, reducing memory usage by ~95% in multi-client scenarios.

```rust
static SHARED_SOAP_CLIENT: LazyLock<SoapClient> = LazyLock::new(|| SoapClient::new());

impl SoapClient {
    pub fn get() -> &'static Self { &SHARED_SOAP_CLIENT }
}
```

#### 3. Trait-Based Operations (sonos-api)
The `SonosOperation` trait provides a consistent interface for all UPnP operations:

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

#### 4. Reference-Counted Observable (sonos-event-manager)
Similar to RxJS `refCount()`, subscriptions are automatically managed based on consumer count:

```
T1: First watcher    → ref count 0→1 → Creates UPnP subscription
T2: Second watcher   → ref count 1→2 → Reuses existing subscription
T3: First dropped    → ref count 2→1 → Subscription remains
T4: Second dropped   → ref count 1→0 → Unsubscribes from device
```

#### 5. Transparent Fallback (sonos-stream)
Events are delivered consistently regardless of source:

```rust
pub enum EventSource {
    UPnP,      // Real-time callbacks
    Polling,   // Fallback when firewall blocks
}

pub struct EnrichedEvent {
    pub source: EventSource,
    pub data: EventData,
    // Same interface regardless of source
}
```

#### 6. Watch Channels (sonos-state)
Properties use `tokio::sync::watch` for efficient reactive updates:

```rust
// Single producer, multiple consumers
// Latest value always available
// Efficient change notification
let mut watcher = speaker.volume.watch().await?;
while watcher.changed().await.is_ok() {
    let current = watcher.current();
}
```

---

## Crate Size Reference

| Crate | Size | Classification | Primary Responsibility |
|-------|------|----------------|------------------------|
| sonos-sdk | - | **Public** | DOM-like API (main entry point) |
| sonos-api | 228 KB | Public | Type-safe UPnP operations |
| sonos-discovery | 40 KB | Public | SSDP device discovery |
| sonos-stream | 204 KB | Internal | Event streaming with fallback |
| sonos-state | 148 KB | Internal | Reactive state management |
| callback-server | 56 KB | Internal | HTTP event server |
| sonos-event-manager | 20 KB | Internal | Subscription reference counting |
| soap-client | 20 KB | Internal | SOAP transport (singleton) |
