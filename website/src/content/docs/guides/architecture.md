---
title: Architecture
description: How the SDK layers work together to control Sonos devices.
---

## Overview

The Sonos SDK is built as a layered Rust workspace. Each layer has a single responsibility, and you only interact with the top layer (`sonos-sdk`).

```
┌─────────────────────────────────────────────┐
│  sonos-sdk         (your entry point)       │
│  SonosSystem → Speaker → Property Handles   │
├─────────────────────────────────────────────┤
│  sonos-discovery   │  sonos-api             │
│  (SSDP multicast)  │  (UPnP operations)     │
├─────────────────────────────────────────────┤
│  sonos-state       (reactive state)         │
│  sonos-event-manager (subscription refcount)│
│  sonos-stream      (event/polling switch)   │
├─────────────────────────────────────────────┤
│  callback-server   │  soap-client           │
│  (HTTP listener)   │  (SOAP transport)      │
└─────────────────────────────────────────────┘
            │
            ▼
    Sonos Devices (port 1400)
```

## The three layers you care about

### 1. Discovery

When you call `SonosSystem::new()`, the SDK sends an SSDP multicast to find all Sonos devices on your network. This is fast (~2 seconds) and happens once.

```rust
let sonos = SonosSystem::new()?; // discovers all speakers
let speakers = sonos.speakers();  // vec of Speaker handles
```

### 2. Direct control (fetch)

Every property has a `fetch()` method that makes a live SOAP call to the device. No event infrastructure is involved — it's a simple HTTP request to port 1400.

```rust
let volume = speaker.volume.fetch()?;    // GET request to device
speaker.volume.set(40)?;                  // SET request to device
```

### 3. Reactive events (watch)

The first time you call `watch()` on any property, the SDK lazily initializes:
- A callback HTTP server to receive UPnP NOTIFY events
- A subscription to the relevant service on the device
- Automatic firewall detection with fallback to polling

```rust
for event in sonos.iter() {
    let volume = speaker.volume.watch()?;  // initializes event system on first call
    println!("Volume: {:?}", volume.value());
}
```

## Lazy initialization

The SDK is designed so you pay only for what you use:

| Action | What happens |
|--------|-------------|
| `SonosSystem::new()` | SSDP discovery only (~2s) |
| `.fetch()` or `.set()` | Single HTTP request per call |
| `.watch()` (first call) | Starts callback server + subscribes |
| `.watch()` (subsequent) | Reuses existing subscription (ref-counted) |

If you never call `watch()`, the event system never starts. If you only read properties with `fetch()`, no background threads are spawned.

## Automatic fallback

When a firewall blocks UPnP event callbacks (common on corporate networks), the SDK detects this proactively and switches to polling mode. Your code doesn't change — `watch()` and `sonos.iter()` work identically regardless of the underlying transport.

## Crate responsibilities

| Crate | Role | You use it? |
|-------|------|-------------|
| `sonos-sdk` | Public API, property handles, navigation | Yes |
| `sonos-discovery` | SSDP network scanning | Via SonosSystem |
| `sonos-api` | UPnP operation types and execution | Via property handles |
| `sonos-state` | Reactive state with watch channels | Via .watch() |
| `sonos-event-manager` | Ref-counted subscription lifecycle | Internal |
| `sonos-stream` | Event/polling transport switching | Internal |
| `callback-server` | HTTP server for UPnP callbacks | Internal |
| `soap-client` | SOAP envelope building and HTTP | Internal |

## Further reading

- [Properties guide](/sonos-sdk/guides/properties/) — deep dive into get/fetch/watch
- [API reference on docs.rs](https://docs.rs/sonos-sdk) — full type documentation
