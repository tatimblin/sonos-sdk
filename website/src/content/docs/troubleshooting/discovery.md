---
title: Discovery Issues
description: Troubleshoot when speakers aren't found on the network.
---

## Symptoms

- `SonosSystem::new()` returns an empty speaker list
- Only some speakers are found
- Discovery works intermittently

## How discovery works

The SDK uses SSDP (Simple Service Discovery Protocol) to find Sonos devices:

1. Sends an `M-SEARCH` multicast packet to `239.255.255.250:1900`
2. Sonos devices respond with their location and capabilities
3. The SDK deduplicates and returns discovered devices

This requires multicast UDP to work on your network.

## Common issues

### Speakers on a different subnet

Sonos devices and your machine must be on the same subnet. SSDP multicast doesn't cross subnet boundaries unless your router is configured for it.

**Check:** Run `sonos speakers` (CLI) or print the discovery results. If you see zero devices, verify your machine's IP is on the same subnet as the speakers (e.g., both on `192.168.1.x`).

### Wi-Fi isolation / AP isolation

Many routers have "AP isolation" or "client isolation" enabled, which prevents devices on Wi-Fi from seeing each other. This is common on:
- Guest networks
- Hotel/corporate Wi-Fi
- Mesh networks with isolation enabled

**Fix:** Disable AP isolation in your router settings, or connect via Ethernet.

### Multicast disabled on the network

Some managed switches or enterprise networks block multicast traffic.

**Check:** Try discovering devices from a different machine on the same network. If it works there but not here, the issue is specific to your machine or network port.

### macOS Wi-Fi power saving

macOS may throttle multicast reception when on battery or when the display is off.

**Fix:** For development, keep the machine plugged in and awake. In production, consider running on a wired connection.

### Discovery timeout

The default timeout gives devices ~2 seconds to respond. On slow networks or with many devices, this may not be enough.

```rust
use std::time::Duration;
use sonos_sdk::prelude::*;

// Increase discovery timeout to 5 seconds
let sonos = SonosSystem::with_timeout(Duration::from_secs(5))?;
```

### Only some speakers found

If you consistently find some but not all speakers:

1. **Different subnets:** Some speakers may be on a different VLAN
2. **SonosNet vs. WiFi:** Speakers on SonosNet (Sonos's own mesh) bridge to Wi-Fi speakers, but discovery still requires being on the same subnet as at least one device
3. **Timeouts:** Devices on poor Wi-Fi may respond slowly — increase the timeout

## Verifying network connectivity

Test that you can reach a Sonos device directly:

```bash
# Replace with your speaker's IP (find it in the Sonos app under Settings > About)
curl -s http://192.168.1.100:1400/xml/device_description.xml | head -5
```

If this returns XML, your machine can reach the device. If it times out, there's a network routing issue between your machine and the speaker.

## Still stuck?

- Verify your Sonos system is set up and working via the official Sonos app
- Try rebooting the nearest Sonos speaker (unplug for 10 seconds)
- Check that your router firmware is up to date (older firmware sometimes has multicast bugs)
- File an issue at [github.com/tatimblin/sonos-sdk/issues](https://github.com/tatimblin/sonos-sdk/issues)
