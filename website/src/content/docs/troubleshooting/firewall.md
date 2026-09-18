---
title: Firewall & Events
description: Troubleshoot UPnP event delivery when firewalls block callbacks.
---

## Symptoms

- `watch()` works but events never arrive
- Updates land, but seconds late instead of immediately
- Events work on one machine but not another
- Events work on home Wi-Fi but not office/corporate network

## How events work

When you call `watch()`, the SDK:

1. Starts a local HTTP callback server, binding the first free port in the range **3400–3500**
2. Sends a SUBSCRIBE request to the Sonos device with that callback URL
3. The device sends HTTP NOTIFY requests back to your machine when state changes

If a firewall blocks incoming connections to your machine, step 3 fails silently.

## Automatic fallback

The SDK detects this and falls back to polling. You don't need to change your code — `watch()` and `sonos.iter()` work the same way regardless of whether values arrive via push or poll.

**How detection works:** detection is per device and needs no probe traffic. After the first subscription to a device, the SDK waits up to **15 seconds** for a real UPnP event from it. If one arrives, that device is marked accessible. If none does, the device is marked blocked and switched to polling. The verdict is cached per device, so later subscriptions to the same speaker skip the wait.

**Polling interval:** 5 seconds to start. Polling is adaptive — a property that rarely changes backs off toward a 30-second ceiling, and a busy one stays near the floor.

## Common firewall scenarios

### macOS firewall

macOS may prompt "Allow incoming connections?" when the callback server starts. Click **Allow**. If you previously clicked Deny:

1. Open System Settings → Network → Firewall → Options
2. Find your application in the list
3. Set it to "Allow incoming connections"

### Corporate/managed networks

Many corporate networks block all inbound connections. The SDK handles this automatically via polling. No action needed.

### Docker / containers

If your code runs in a container, the callback URL must be reachable from the Sonos device. Ensure:
- The container is on the same network as the Sonos devices (use `--network host`, since SSDP multicast does not cross a bridge)
- Ports 3400–3500 are reachable from the device

### Linux iptables / nftables

Open the callback port range for inbound TCP. Note this is the range the SDK
listens on — port 1400 is the port on the *speaker*, and nothing needs to accept
inbound traffic there:

```bash
# iptables
sudo iptables -A INPUT -p tcp --dport 3400:3500 -j ACCEPT

# ufw
sudo ufw allow 3400:3500/tcp
```

## Diagnosing issues

`WatchHandle::mode()` reports which transport is actually behind a given watch —
`Events` for real-time UPnP, `Polling` when the subscription failed, `CacheOnly`
when no event manager is configured. `has_realtime_events()` is the same answer
narrowed to a bool:

```rust
let volume = speaker.volume.watch()?;
println!("transport: {}", volume.mode());
```

For more detail, turn on tracing:

```rust
// Add tracing-subscriber to your dependencies
tracing_subscriber::fmt::init();

// Run with RUST_LOG=sonos_stream=debug
// Look for polling-mode messages naming the reason, e.g. "firewall blocked"
```

## Performance comparison

| Mode | Latency | CPU usage | Network |
|------|---------|-----------|---------|
| Push (UPnP events) | As the device sends them | Minimal (event-driven) | Low |
| Poll (fallback) | 5s, backing off to 30s when quiet | Slight (periodic requests) | Moderate |

For most applications (dashboards, CLIs, automation), polling mode is workable, though a UI that mirrors physical button presses will feel the lag.
