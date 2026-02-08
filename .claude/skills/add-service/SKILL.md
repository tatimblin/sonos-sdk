---
name: add-service
description: Add a complete new Sonos service across all SDK layers (api → stream → state → sdk). This orchestrator skill coordinates the 4 layer-specific skills for full-stack service implementation.
---

# Add Service Orchestrator

## Overview

This skill coordinates the implementation of a new Sonos UPnP service across all 4 SDK layers:

1. **sonos-api** - UPnP operations (SOAP requests/responses)
2. **sonos-stream** - Event streaming and polling
3. **sonos-state** - Reactive state management
4. **sonos-sdk** - DOM-like public API

## Prerequisites

Before starting, gather:
- Service documentation URL (e.g., `https://sonos.svrooij.io/services/queue`)
- List of operations to implement
- List of properties to expose
- Access to a real Sonos speaker for testing

## Quick Start

```bash
# Check implementation status across all layers
python .claude/skills/add-service/scripts/service_status.py NewService

# After implementation, run full integration test
python .claude/skills/add-service/scripts/integration_test.py NewService 192.168.1.100
```

## Complete Workflow

### Step 1: API Layer (sonos-api)

Use `/implement-service` skill to implement UPnP operations.

**Files Modified:**
- `sonos-api/src/services/{service}/mod.rs` - Service module
- `sonos-api/src/services/{service}/operations.rs` - Operation structs
- `sonos-api/src/services/mod.rs` - Service registration
- `sonos-api/src/lib.rs` - Re-exports

**Key Tasks:**
1. Extract operations from service documentation
2. Generate operation structs with macros
3. Test against real speakers
4. Add validation and error handling

**Verify:**
```bash
cargo test -p sonos-api
cargo run --example cli_example -- <speaker_ip> NewService OperationName
```

### Step 2: Stream Layer (sonos-stream)

Use `/implement-service-stream` skill to add event streaming support.

**Files Modified:**
- `sonos-stream/src/events/types.rs` - Event struct + EventData variant
- `sonos-stream/src/events/processor.rs` - Event processor case
- `sonos-stream/src/polling/strategies.rs` - ServicePoller impl

**Key Tasks:**
1. Define event struct with all state fields
2. Add EventData enum variant
3. Implement convert_api_event_data() case
4. Implement ServicePoller for polling fallback

**Verify:**
```bash
cargo test -p sonos-stream
python .claude/skills/implement-service-stream/scripts/analyze_stream_events.py --validate
python .claude/skills/implement-service-stream/scripts/test_polling.py <speaker_ip> NewService
```

### Step 3: State Layer (sonos-state)

Use `/implement-service-state` skill to add reactive state management.

**Files Modified:**
- `sonos-state/src/property.rs` - Property structs
- `sonos-state/src/decoder.rs` - PropertyChange variants + decoder
- `sonos-state/src/lib.rs` - Re-exports

**Key Tasks:**
1. Define property structs with Property + SonosProperty traits
2. Add PropertyChange enum variants
3. Implement decoder function
4. Update decode_event() switch

**Verify:**
```bash
cargo test -p sonos-state
python .claude/skills/implement-service-state/scripts/analyze_properties.py --coverage
```

### Step 4: SDK Layer (sonos-sdk)

Use `/implement-service-sdk` skill to expose properties through DOM-like API.

**Files Modified:**
- `sonos-sdk/src/property/handles.rs` - Fetchable impl + type alias
- `sonos-sdk/src/speaker.rs` - Speaker struct fields
- `sonos-sdk/src/lib.rs` - Re-exports

**Key Tasks:**
1. Implement Fetchable trait (if property has Get operation)
2. Create type aliases for handles
3. Add fields to Speaker struct
4. Initialize handles in Speaker::new()

**Verify:**
```bash
cargo test -p sonos-sdk
python .claude/skills/implement-service-sdk/scripts/analyze_handles.py --coverage
```

### Step 5: Integration Testing

After all layers are complete:

```bash
# Full workspace test
cargo test

# Integration test with real speaker
python .claude/skills/add-service/scripts/integration_test.py NewService <speaker_ip>

# Run SDK example
cargo run -p sonos-sdk --example basic_usage
```

## Layer Responsibilities Reference

| Layer | Crate | Purpose | Key Files |
|-------|-------|---------|-----------|
| API | sonos-api | UPnP SOAP operations | `services/{service}/operations.rs` |
| Stream | sonos-stream | Event streaming/polling | `events/types.rs`, `polling/strategies.rs` |
| State | sonos-state | Reactive state store | `property.rs`, `decoder.rs` |
| SDK | sonos-sdk | DOM-like public API | `property/handles.rs`, `speaker.rs` |

## Data Flow

```
                    UPnP Events
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ sonos-stream: EventData enum, ServicePoller                 │
└─────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ sonos-state: decode_event() → PropertyChange → StateStore   │
└─────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ sonos-sdk: PropertyHandle.get() / watch() / fetch()         │
└─────────────────────────────────────────────────────────────┘
                        │
                        ▼
                   User Code
```

## Common Issues

### Events Not Showing Up
1. Check EventData variant exists in `sonos-stream/src/events/types.rs`
2. Check `convert_api_event_data()` handles the service
3. Check decoder exists in `sonos-state/src/decoder.rs`

### Properties Not Updating
1. Check PropertyChange variant exists
2. Check `key()` and `service()` match arms
3. Check decoder function parses all fields

### fetch() Not Available
The property may be event-only (no dedicated Get operation). Document this and use get() with watch().

### Watch Mode Always CacheOnly
Event manager not configured. Call `system.configure_events()` first.

## Rollback Procedure

If implementation fails, revert in reverse order:
1. SDK layer changes
2. State layer changes
3. Stream layer changes
4. API layer changes

Use `git diff` to identify changes in each crate:
```bash
git diff sonos-sdk/
git diff sonos-state/
git diff sonos-stream/
git diff sonos-api/
```

## References

- [Implement Service (API)](../implement-service/SKILL.md)
- [Implement Service Stream](../implement-service-stream/SKILL.md)
- [Implement Service State](../implement-service-state/SKILL.md)
- [Implement Service SDK](../implement-service-sdk/SKILL.md)
- [Workflow Checklist](references/workflow-checklist.md)
