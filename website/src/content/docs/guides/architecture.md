---
title: Architecture
description: How the SDK layers work together to control Sonos devices.
---

## Overview

The Sonos SDK is built as a layered Rust workspace of eight crates. Each layer has a single responsibility, and you only interact with the top layer (`sonos-sdk`).

```
┌────────────────────────────────────────────────────┐
│ sonos-sdk             (your entry point)           │
│ SonosSystem → Speaker → Property Handles           │
├─────────────────────────┬──────────────────────────┤
│ sonos-sdk-discovery     │ sonos-api                │
│ (SSDP multicast)        │ (UPnP operations)        │
├─────────────────────────┴──────────────────────────┤
│ sonos-sdk-state          (property store)          │
│ sonos-sdk-event-manager  (subscriptions)           │
│ sonos-sdk-stream         (event/poll switch)       │
├─────────────────────────┬──────────────────────────┤
│ sonos-sdk-callback-     │ sonos-sdk-soap-client    │
│ server (HTTP listener)  │ (SOAP transport)         │
└─────────────────────────┴──────────────────────────┘
             │
             ▼
     Sonos Devices (port 1400)
```

## The three layers you care about

### 1. Discovery

`SonosSystem::new()` is cache-first. It reads the device cache at
`~/.cache/sonos/cache.json` (override the directory with `SONOS_CACHE_DIR`). A
cache younger than 24 hours is used as-is, which makes the constructor
essentially free on repeat runs. Otherwise the SDK sends an SSDP multicast with
a 3-second timeout and writes the result back to the cache; if that finds
nothing and a stale cache exists, the stale devices are used rather than
failing. With no cache and no SSDP response, `new()` returns
`SdkError::DiscoveryFailed`.

```rust
let sonos = SonosSystem::new()?; // cached devices, or SSDP
let speakers = sonos.speakers();  // Vec<Speaker>
```

### 2. Direct control (fetch and setters)

Every property has a `fetch()` method that makes a live SOAP call to the device. No event infrastructure is involved — it's a simple HTTP request to port 1400. Writes are methods on `Speaker` and `Group`, not on the property handle.

```rust
let volume = speaker.volume.fetch()?;    // GET request to device
speaker.set_volume(40)?;                  // SET request to device
```

### 3. Reactive events (watch)

The first time you call `watch()` on any property, the SDK lazily initializes:
- A callback HTTP server bound to a free port in the range 3400–3500, to receive UPnP NOTIFY events
- A subscription to the relevant service on the device
- Firewall detection, with fallback to polling

```rust
// Acquire the handle first — iter() only emits events for properties
// that are already being watched.
let volume = speaker.volume.watch()?;

for _event in sonos.iter() {
    println!("Volume: {:?}", volume.value());
}
```

## Lazy initialization

The SDK is designed so you pay only for what you use:

| Action | What happens |
|--------|-------------|
| `SonosSystem::new()` | Cache read, or SSDP discovery (3s timeout) |
| `.fetch()` or a setter | Single HTTP request per call |
| `.watch()` (first call) | Starts callback server + subscribes |
| `.watch()` (subsequent) | Reuses existing subscription (ref-counted) |

If you never call `watch()`, the event system never starts. If you only read properties with `fetch()`, no background threads are spawned.

## Automatic fallback

When a firewall blocks UPnP event callbacks (common on corporate networks), the SDK detects this and switches to polling mode. Your code doesn't change — `watch()` and `sonos.iter()` work identically regardless of the underlying transport.

## How reactivity is wired

Two pieces do the work.

**A watched-key set.** `sonos-sdk-state` tracks which `(speaker_id, property_key)` pairs are under watch, as reference counts rather than flags — several independent watchers can hold the same pair, and it stops being watched only when the last one lets go. Store writes for a pair nobody watches update the cache and emit nothing.

**An event fanout.** Each call to `sonos.iter()` registers an independent subscriber holding its own unbounded `std::sync::mpsc` queue. Every `ChangeEvent` is pushed to all live subscribers under one lock, which buys three guarantees:

- Two event loops each see the whole stream rather than splitting it between them
- Order is preserved per subscriber, in the order the emitter produced it
- Nothing is dropped, and a slow consumer never stalls a fast one

Unbounded queues are what make that last one hold, and they carry the matching cost: a subscriber that never drains grows its own queue without bound.

Plain `std::sync::mpsc` senders, rather than a Tokio channel, are what keep this sync-first. Nothing here assumes a runtime, `recv()` blocks safely from inside one, and `ChangeIterator::recv_timeout` exists because the channel supports it.

Events carry the new value as a typed `PropertyChange` rather than a signal to go re-read the store. That matters when draining a backlog: a `Playing → Transitioning → Playing` sequence is three queued events but only one final store value, so re-reading the store would hide the middle state and the fact that anything moved at all.

## Crate responsibilities

| Crate | Role | You use it? |
|-------|------|-------------|
| `sonos-sdk` | Public API, property handles, navigation | Yes |
| `sonos-sdk-discovery` | SSDP network scanning | Via SonosSystem |
| `sonos-api` | UPnP operation types and execution | Via property handles |
| `sonos-sdk-state` | Property store, watch bookkeeping, change events | Via .watch() |
| `sonos-sdk-event-manager` | Ref-counted subscription lifecycle | Internal |
| `sonos-sdk-stream` | Event/polling transport switching | Internal |
| `sonos-sdk-callback-server` | HTTP server for UPnP callbacks | Internal |
| `sonos-sdk-soap-client` | SOAP envelope building and HTTP | Internal |

## Further reading

- [Properties guide](/sonos-sdk/guides/properties/) — deep dive into get/fetch/watch
- [API reference on docs.rs](https://docs.rs/sonos-sdk) — full type documentation
