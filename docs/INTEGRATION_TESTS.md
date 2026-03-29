# Sonos SDK Integration Tests

This document describes how to run and understand the integration test suite that validates core SDK functionality against real Sonos hardware.

## Quick Start

Run all integration tests before submitting PRs:

```bash
# Run all integration tests
cargo test --package sonos-sdk --test integration_real_speakers -- --ignored --nocapture

# Run a specific test
cargo test --package sonos-sdk --test integration_real_speakers -- --ignored test_event_integration

# Run with output filtering (quieter)
cargo test --package sonos-sdk --test integration_real_speakers -- --ignored
```

## Requirements

### Hardware Requirements
- **Minimum:** 1 reachable Sonos speaker on local network
- **For group tests:** 2+ standalone speakers (not bonded pairs)
- **Network:** Speakers must be discoverable via SSDP

### Software Requirements
- Rust toolchain with cargo
- Network connectivity between test machine and speakers
- No firewall blocking UPnP ports (typically 1400, 3400-3500)

## Test Descriptions

### ✅ API Operations (`test_api_operations`)
**Validates:** Basic SOAP API calls work correctly
**Tests:** Volume control, playback state, mute, bass, treble, loudness properties
**Duration:** ~1 second

### ✅ Property Watching (`test_property_watching`)
**Validates:** WatchHandle API and property access patterns
**Tests:** Cache behavior, concurrent watches, RAII cleanup
**Duration:** ~1 second

### ✅ Event Streaming (`test_event_streaming`)
**Validates:** UPnP subscription lifecycle and grace period behavior
**Tests:** 50ms grace period, subscription reuse, event processing
**Duration:** ~2 seconds

### ✅ Group Management (`test_group_lifecycle`)
**Validates:** Multi-speaker group operations
**Tests:** Group creation, speaker joining/leaving, topology updates
**Duration:** ~1 second (or immediate skip if insufficient speakers)
**Note:** Requires 2+ standalone speakers; gracefully skips otherwise

### ⭐ Event Integration (`test_event_integration`)
**Validates:** End-to-end event flow from property watching to system.iter()
**Tests:** Property watching → API changes → events via system.iter()
**Duration:** ~1 second

This test specifically validates that:
1. Starting to watch a property enables event streaming
2. API changes (volume adjustments) generate events
3. Events are received through `system.iter()` with correct metadata
4. Multiple events work (change + restore)

## Expected Output

### Successful Run
```
running 5 tests
✅ API operations test completed successfully
test test_api_operations ... ok
✅ Property access patterns validated
test test_property_watching ... ok
✅ Event streaming test completed successfully
test test_event_streaming ... ok
⚠️  Skipping group lifecycle test: Found 0 standalone speakers, need 2
test test_group_lifecycle ... ok
✅ Event integration validated: property watching -> API changes -> events via system.iter()
test test_event_integration ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
```

### Common Issues

#### No Speakers Found
```
Error: "No Sonos speakers found. Integration tests require real hardware."
```
**Fix:** Ensure Sonos speakers are on the network and discoverable

#### No Reachable Speakers
```
Error: "No reachable speakers found"
```
**Fix:** Check network connectivity, ensure speakers aren't in sleep mode

#### Insufficient Speakers for Groups
```
⚠️  Skipping group lifecycle test: Found 0 standalone speakers, need 2
```
**Note:** This is expected if speakers are already grouped or are home theater setups

## Integration with Development Workflow

### Pre-PR Checklist
1. ✅ Code changes complete
2. ✅ Unit tests passing: `cargo test --workspace`
3. ✅ Integration tests passing: `cargo test --package sonos-sdk --test integration_real_speakers -- --ignored`
4. ✅ Linting clean: `cargo clippy`

### When to Run
- **Always:** Before submitting PRs
- **Recommended:** After significant SDK changes
- **Optional:** During development for real hardware validation

### Performance Expectations
- **Total runtime:** < 30 seconds
- **Individual tests:** < 5 seconds each
- **Grace period validation:** Microsecond-precision timing

## Troubleshooting

### Test Failures
1. **Check speaker connectivity** - Can you control speakers via Sonos app?
2. **Verify network setup** - Are speakers on same network as test machine?
3. **Check for interference** - Other applications using speakers simultaneously?

### Firewall Issues
If tests hang or timeout:
- Ensure ports 1400, 3400-3500 are open
- Check UPnP is enabled on router
- Verify no VPN interfering with local network discovery

### Permission Issues
On some networks, UPnP discovery may require elevated permissions:
```bash
sudo cargo test --package sonos-sdk --test integration_real_speakers -- --ignored
```

## Architecture Notes

The integration tests validate the complete SDK stack:

```
Integration Tests
       ↓
    sonos-sdk (Public API)
       ↓
    sonos-state (Reactive State)
       ↓
    sonos-event-manager (Subscriptions)
       ↓
    sonos-stream (Event Processing)
       ↓
    callback-server (HTTP Events) + sonos-api (SOAP)
       ↓
    Real Sonos Hardware
```

Each test validates different layers:
- **API Operations:** sonos-sdk → sonos-api → Hardware
- **Property Watching:** Full reactive stack
- **Event Streaming:** UPnP event processing with grace periods
- **Group Management:** Multi-speaker coordination
- **Event Integration:** Complete end-to-end event flow

This ensures breaking changes are caught at any layer before reaching production.