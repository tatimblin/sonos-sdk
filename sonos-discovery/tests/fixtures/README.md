# Test Fixtures

This directory contains XML device descriptions captured from real Sonos devices for use in testing.

## Fixture Files

### Real Device Fixtures

These fixtures were captured from actual Sonos devices on a network using the `capture_fixtures` test:

- **sonos_one_device.xml** - Sonos One speaker (Model S18)
  - Room: Bedroom
  - Features: Modern Sonos speaker with voice control support
  - UDN: uuid:RINCON_7828CA0E1E1801400

- **sonos_play1_device.xml** - Sonos Play:1 speaker (Model S1)
  - Room: Dining Room  
  - Features: Compact speaker, older generation
  - UDN: uuid:RINCON_B8E9373C4F0601400

- **sonos_playbar_device.xml** - Sonos Playbar soundbar (Model S9)
  - Room: TV Room
  - Features: Home theater soundbar with HTControl service
  - UDN: uuid:RINCON_5CAAFDAE58BD01400

- **sonos_amp_device.xml** - Sonos Amp (Model S16)
  - Room: Living Room
  - Features: Amplifier with AudioIn service for external sources
  - UDN: uuid:RINCON_804AF2AA2FA201400

- **sonos_roam_device.xml** - Sonos Roam 2 portable speaker (Model S54)
  - Room: Roam / Office
  - Features: Portable battery-powered speaker
  - UDN: uuid:RINCON_C43875CA135801400

### Test-Only Fixtures

- **non_sonos_router_device.xml** - Non-Sonos UPnP device
  - Purpose: Test filtering of non-Sonos devices
  - Device Type: InternetGatewayDevice (router)
  - Should be rejected by `is_sonos_device()` validation

- **minimal_sonos_device.xml** - Minimal valid Sonos device
  - Purpose: Test parsing with only required fields
  - Contains bare minimum XML structure for a valid Sonos device
  - Tests default value handling for optional fields

## Usage in Tests

These fixtures can be used to:

1. **Mock HTTP responses** - Return fixture content instead of making real network requests
2. **Test XML parsing** - Validate device description parsing logic
3. **Test device validation** - Ensure Sonos vs non-Sonos device detection works
4. **Test edge cases** - Use minimal fixture to test handling of missing optional fields

### Parameterized Integration Tests

The `fixture_based_integration.rs` test file provides comprehensive parameterized tests using these fixtures:

```rust
// Test helper for loading fixtures
use helpers::{DeviceFixture, FixtureSet};

// Load individual fixtures
let fixture = DeviceFixture::load("sonos_one_device.xml", "192.168.1.100");

// Use predefined fixture sets
let fixtures = FixtureSet::all_sonos_devices();  // All 5 Sonos devices
let fixtures = FixtureSet::mixed_devices();      // Mix of Sonos and non-Sonos
```

Run the fixture-based tests:
```bash
cargo test --test fixture_based_integration
```

These tests cover 33 test cases including device parsing, identification, filtering, HTTP mocking, and error handling.

## Capturing New Fixtures

To capture additional device data from your network:

```bash
cargo test --test capture_fixtures -- --nocapture --ignored
```

This will:
- Discover all Sonos devices on your network
- Display device information
- Fetch and print device XML descriptions
- Suggest fixture filenames

Save the XML output to new files in this directory as needed.

## Fixture Characteristics

All Sonos device fixtures share these characteristics:
- Manufacturer: "Sonos, Inc."
- Device Type: Contains "ZonePlayer" or "MediaRenderer"
- UDN: Contains "RINCON" prefix
- Port: 1400 (standard Sonos port)
- XML namespace: urn:schemas-upnp-org:device-1-0

## Privacy Note

The fixtures have been sanitized to use example IP addresses and MAC addresses where appropriate. Serial numbers and device IDs from real devices are preserved as they're needed for realistic testing.
