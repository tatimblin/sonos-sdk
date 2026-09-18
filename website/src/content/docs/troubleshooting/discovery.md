---
title: Discovery Issues
description: Troubleshoot when speakers aren't found on the network.
---

## Symptoms

- `SonosSystem::new()` returns `Err(SdkError::DiscoveryFailed)`
- Only some speakers are found
- Speakers that no longer exist keep showing up
- Discovery works intermittently

## How discovery works

`SonosSystem::new()` is cache-first:

1. It reads the device cache at `~/.cache/sonos/cache.json` (the directory is the
   platform cache dir, or `SONOS_CACHE_DIR` when that is set to an absolute path)
2. A cache younger than 24 hours is used as-is — no network traffic at all
3. Otherwise it sends an `M-SEARCH` multicast to `239.255.255.250:1900` and
   waits 3 seconds for replies, then writes the result back to the cache
4. If SSDP finds nothing but a stale cache exists, the stale devices are used
5. If there is no cache and SSDP finds nothing, `new()` returns
   `SdkError::DiscoveryFailed`

Steps 3–5 require multicast UDP to work on your network.

## Common issues

### A stale cache is answering instead of the network

Because a fresh cache short-circuits SSDP entirely, a speaker you renamed,
moved, or removed in the last 24 hours can still appear — and a newly added one
will not. Delete the cache file to force a full SSDP pass:

```bash
rm -f "${SONOS_CACHE_DIR:-$HOME/Library/Caches/sonos}/cache.json"   # macOS
rm -f "${SONOS_CACHE_DIR:-$HOME/.cache/sonos}/cache.json"           # Linux
```

Point `SONOS_CACHE_DIR` at a scratch directory to isolate a test run from your
everyday cache.

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

### Slow devices missing the window

The SSDP window is a fixed 3 seconds. A device on poor Wi-Fi can miss it. The
first successful run caches every device it did find, so a second
`SonosSystem::new()` right after a failed one has a fresh chance at the
stragglers — clear the cache first so it actually re-scans.

### Only some speakers found

If you consistently find some but not all speakers:

1. **Different subnets:** Some speakers may be on a different VLAN
2. **SonosNet vs. WiFi:** Speakers on SonosNet (Sonos's own mesh) bridge to Wi-Fi speakers, but discovery still requires being on the same subnet as at least one device
3. **Cache:** A partial result from an earlier run is cached for 24 hours — clear it and retry

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
