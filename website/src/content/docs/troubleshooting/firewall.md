---
title: Firewall & Events
description: Troubleshoot UPnP event delivery when firewalls block callbacks.
---

## Symptoms

- `watch()` works but events never arrive
- Events work on one machine but not another
- Events work on home Wi-Fi but not office/corporate network

## How events work

When you call `watch()`, the SDK:

1. Starts a local HTTP server on your machine
2. Sends a SUBSCRIBE request to the Sonos device with your callback URL
3. The device sends HTTP NOTIFY requests back to your machine when state changes

If a firewall blocks incoming connections to your machine, step 3 fails silently.

## Automatic fallback

The SDK detects firewall issues proactively and falls back to polling mode. You don't need to change your code — `watch()` and `sonos.iter()` work the same way regardless of whether events arrive via push or poll.

**How detection works:** After subscribing, the SDK sends a test event to itself. If it doesn't arrive within a short window, it switches to polling for that device.

**Polling interval:** ~1 second by default. This is slightly less responsive than real-time events (~50ms) but functionally equivalent for most applications.

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
- The container is on the same network as the Sonos devices (use `--network host` or bridge networking)
- The callback port is exposed

### Linux iptables / nftables

Ensure the callback port is open for inbound TCP:

```bash
# iptables
sudo iptables -A INPUT -p tcp --dport 1400 -j ACCEPT

# Or temporarily disable firewall for testing
sudo ufw allow 1400/tcp
```

## Diagnosing issues

If you want to verify whether events are arriving via push or poll, enable tracing:

```rust
// Add tracing-subscriber to your dependencies
tracing_subscriber::fmt::init();

// Run with RUST_LOG=sonos_stream=debug
// Look for "Firewall detected, switching to polling" in output
```

## Performance comparison

| Mode | Latency | CPU usage | Network |
|------|---------|-----------|---------|
| Push (UPnP events) | ~50ms | Minimal (event-driven) | Low |
| Poll (fallback) | ~1s | Slight (periodic requests) | Moderate |

For most applications (dashboards, CLIs, automation), polling mode is indistinguishable from push mode.
