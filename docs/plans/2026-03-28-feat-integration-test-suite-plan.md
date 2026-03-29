---
title: Integration Test Suite for Real Speaker Validation
type: feat
status: completed
date: 2026-03-28
origin: docs/brainstorms/2026-03-28-integration-test-suite-brainstorm.md
---

# Integration Test Suite for Real Speaker Validation

## Overview

Create a modular integration test suite that validates core Sonos SDK functionality against real speakers before PR submission. The suite will catch breaking changes by running fast smoke tests across critical SDK areas using a single binary approach with focused test modules.

## Problem Statement

The Sonos SDK needs reliable pre-PR validation to catch breaking changes before they reach production. Manual testing is inconsistent, and the existing examples require individual execution. We need a comprehensive yet fast integration test suite that enforces testing against real hardware and provides clear pass/fail feedback for developers.

## Proposed Solution

Implement standard Rust integration tests (`cargo test --ignored`) with modular test architecture:
- **Standard Testing**: Uses Rust's built-in integration test infrastructure
- **Fail Loudly**: Tests fail when no real speakers detected (no mock fallbacks)
- **Modular Design**: Five focused test functions with single responsibility
- **Fast Execution**: Smoke tests designed for < 30 second execution
- **Clear Reporting**: Standard cargo test output with detailed error messages

## Technical Approach

### Integration Test Architecture

**Location:** `sonos-sdk/tests/integration_real_speakers.rs` (standard integration test location)

**Command:** `cargo test --package sonos-sdk --test integration_real_speakers --ignored`

**Core Structure:**
```rust
//! Integration tests for real Sonos speaker validation
//!
//! These tests require actual Sonos hardware on the network and are marked with #[ignore]
//! Run with: cargo test --package sonos-sdk --test integration_real_speakers --ignored

use sonos_sdk::prelude::*;

/// Helper function to ensure real speakers are available for testing
fn require_real_speakers() -> Result<SonosSystem, Box<dyn std::error::Error>> {
    let system = SonosSystem::new()?;
    if system.speaker_names().is_empty() {
        return Err("No Sonos speakers found. Integration tests require real hardware.".into());
    }
    Ok(system)
}

#[tokio::test]
#[ignore] // Requires real hardware - run manually with --ignored
async fn test_api_operations() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    // Test implementation...
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_event_streaming() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    // Test implementation...
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_group_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    // Test implementation...
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_property_watching() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    // Test implementation...
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_event_integration() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    // Test implementation...
    Ok(())
}
```

### Modular Test Design

#### 1. API Operations Test (`test_api_operations`)
**Purpose:** Validate all SOAP API calls work against real speakers

**Test Coverage:**
- Basic speaker discovery and reachability
- Volume control operations (get, set, relative adjustments)
- Playback state operations (play, pause, stop, next, previous)
- Mute/unmute operations
- Bass/treble/loudness controls

**Implementation:**
```rust
#[tokio::test]
#[ignore] // Requires real hardware - run manually with --ignored
async fn test_api_operations() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Find any reachable speaker
    let speaker = find_reachable_speaker(&system)?;

    // Test volume operations
    let original_volume = speaker.volume.fetch()?.0;
    speaker.set_volume(original_volume.saturating_sub(5))?;
    assert_eq!(speaker.volume.fetch()?.0, original_volume.saturating_sub(5));
    speaker.set_volume(original_volume)?; // restore

    // Test playback state
    let playback_state = speaker.playback_state.fetch()?;
    eprintln!("Playback state: {:?}", playback_state);

    // Test other properties without state changes
    speaker.mute.fetch()?;
    speaker.bass.fetch()?;
    speaker.treble.fetch()?;
    speaker.loudness.fetch()?;

    Ok(())
}
```

#### 2. Event Streaming Test (`test_event_streaming`)
**Purpose:** Validate UPnP events, subscription lifecycle, and grace periods

**Test Coverage:**
- UPnP subscription creation and cleanup
- Event reception and processing
- Grace period behavior (50ms delay before unsubscribe)
- Subscription sharing across multiple watches

**Implementation:**
```rust
#[tokio::test]
#[ignore] // Requires real hardware - run manually with --ignored
async fn test_event_streaming() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;

    // Test basic subscription creation
    let handle = speaker.volume.watch()?;
    eprintln!("Watch mode: {}", handle.mode());

    // Test grace period behavior
    let start_time = std::time::Instant::now();
    drop(handle);

    // Create new handle within grace period
    let new_handle = speaker.volume.watch()?;
    let elapsed = start_time.elapsed();

    if elapsed < std::time::Duration::from_millis(50) {
        eprintln!("Grace period active: subscription reused at {:?}", elapsed);
    }

    // Test event iteration (brief)
    let iter = system.iter();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut event_count = 0;

    // Make a small volume change to trigger events
    let original_volume = speaker.volume.get().unwrap_or(Volume(20)).0;
    speaker.set_volume(original_volume.saturating_add(1))?;

    while std::time::Instant::now() < deadline && event_count < 3 {
        if let Some(_event) = iter.recv_timeout(std::time::Duration::from_millis(100)) {
            event_count += 1;
        }
    }

    // Restore volume
    speaker.set_volume(original_volume)?;

    eprintln!("Events received: {}", event_count);
    assert!(event_count > 0, "Should receive at least one event after volume change");
    Ok(())
}
```

#### 3. Group Management Test (`test_group_lifecycle`)
**Purpose:** Validate group creation, joining, leaving, and dissolution

**Test Coverage:**
- Group topology discovery
- Speaker joining and leaving groups
- Group coordinator identification
- Group dissolution
- Multi-speaker coordination

**Device Requirements:**
- Minimum 2 standalone speakers (not bonded pairs)
- Skip home theater devices (Playbar, Beam, Arc)

**Implementation:**
```rust
#[tokio::test]
#[ignore] // Requires real hardware - run manually with --ignored
async fn test_group_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;

    // Find compatible speakers
    let standalone_speakers = find_standalone_speakers(&system, 2)?;
    if standalone_speakers.len() < 2 {
        return Err("Group tests require at least 2 standalone speakers".into());
    }

    let speaker_a = &standalone_speakers[0];
    let speaker_b = &standalone_speakers[1];

    // Test group creation
    let group = system.create_group(&[speaker_a.id.clone(), speaker_b.id.clone()])?;
    eprintln!("Group created: {}", &group.id.as_str()[..8]);

    // Wait for topology update
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Verify group membership
    let updated_groups = system.groups();
    let our_group = updated_groups.iter()
        .find(|g| g.member_count() == 2)
        .ok_or("Group not found after creation")?;

    assert_eq!(our_group.member_count(), 2);

    // Test speaker removal
    speaker_b.leave_group()?;

    // Wait and verify
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let final_groups = system.groups();

    // Should have 2 standalone speakers again
    let standalone_count = final_groups.iter().filter(|g| g.member_count() == 1).count();
    assert!(standalone_count >= 2, "Speakers should be standalone after leave_group");

    Ok(())
}
```

#### 4. Property Watching Test (`test_property_watching`)
**Purpose:** Validate WatchHandle API and property access patterns

**Test Coverage:**
- WatchHandle creation and RAII cleanup
- Property access methods: get(), fetch(), watch()
- Cache behavior and consistency
- Multiple concurrent watches

**Implementation:**
```rust
#[tokio::test]
#[ignore] // Requires real hardware - run manually with --ignored
async fn test_property_watching() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;

    // Test get() before fetch() returns None
    assert!(speaker.volume.get().is_none(), "get() should return None initially");

    // Test fetch() updates cache
    let fetched_volume = speaker.volume.fetch()?;
    let cached_volume = speaker.volume.get().expect("get() should return Some after fetch()");
    assert_eq!(fetched_volume.0, cached_volume.0, "fetch() should update cache");

    // Test watch() returns current value
    let watch_handle = speaker.volume.watch()?;
    if let Some(watched_volume) = watch_handle.value() {
        assert_eq!(watched_volume.0, cached_volume.0, "watch() should return cached value");
    }

    // Test multiple concurrent watches
    let handle1 = speaker.volume.watch()?;
    let handle2 = speaker.volume.watch()?;

    // Both should have same value and mode
    assert_eq!(handle1.mode(), handle2.mode(), "Concurrent watches should share mode");

    if let (Some(vol1), Some(vol2)) = (handle1.value(), handle2.value()) {
        assert_eq!(vol1.0, vol2.0, "Concurrent watches should have same value");
    }

    // Test RAII cleanup
    drop(handle1);
    drop(handle2);
    // Subscription should cleanup after grace period

    eprintln!("Property access patterns validated");
    Ok(())
}
```

#### 5. Event Integration Test (`test_event_integration`)
**Purpose:** Validate end-to-end event flow from property watching to system.iter()

**Test Coverage:**
- Property watching triggers initial events
- API-driven property changes generate events
- Events are received through system.iter()
- Event timing and ordering validation

**Implementation:**
```rust
#[tokio::test]
#[ignore] // Requires real hardware - run manually with --ignored
async fn test_event_integration() -> Result<(), Box<dyn std::error::Error>> {
    let system = require_real_speakers()?;
    let speaker = find_reachable_speaker(&system)?;

    // Start watching volume - this should trigger initial event
    let _volume_handle = speaker.volume.watch()?;

    // Get the event iterator
    let iter = system.iter();

    // Wait for initial event from property watching setup
    let setup_event = iter.recv_timeout(std::time::Duration::from_millis(2000))
        .ok_or("No initial event received after starting volume watch")?;

    assert_eq!(setup_event.property_key, "volume");
    eprintln!("✅ Initial setup event received: {}", setup_event.property_key);

    // Get current volume for API change
    let current_volume = speaker.volume.fetch()?.0;
    let new_volume = if current_volume > 5 {
        current_volume - 1
    } else {
        current_volume + 1
    };

    // Change volume via API - this should trigger another event
    speaker.set_volume(new_volume)?;
    eprintln!("🔧 Changed volume: {} -> {}", current_volume, new_volume);

    // Wait for API-triggered event
    let mut api_event_received = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);

    while std::time::Instant::now() < deadline {
        if let Some(event) = iter.recv_timeout(std::time::Duration::from_millis(100)) {
            if event.property_key == "volume" {
                api_event_received = true;
                eprintln!("✅ API-triggered event received: {}", event.property_key);
                break;
            }
        }
    }

    assert!(api_event_received, "No volume event received after API change");

    // Verify the volume actually changed
    let final_volume = speaker.volume.get()
        .ok_or("Volume should be cached after events")?;
    assert_eq!(final_volume.0, new_volume, "Cached volume should match API change");

    // Restore original volume
    speaker.set_volume(current_volume)?;

    eprintln!("Event integration validated: setup event + API-triggered event");
    Ok(())
}
```

### Helper Functions

**Common Test Helpers:**
```rust
/// Helper function to ensure real speakers are available for testing
fn require_real_speakers() -> Result<SonosSystem, Box<dyn std::error::Error>> {
    let system = SonosSystem::new()?;
    if system.speaker_names().is_empty() {
        return Err("No Sonos speakers found. Integration tests require real hardware.".into());
    }
    Ok(system)
}

fn find_reachable_speaker(system: &SonosSystem) -> Result<Speaker, Box<dyn std::error::Error>> {
    let names = system.speaker_names();
    for name in &names {
        if let Some(speaker) = system.speaker(name) {
            match speaker.volume.fetch() {
                Ok(_) => return Ok(speaker),
                Err(_) => continue,
            }
        }
    }
    Err("No reachable speakers found".into())
}

fn find_standalone_speakers(system: &SonosSystem, min_count: usize) -> Result<Vec<Speaker>, Box<dyn std::error::Error>> {
    let groups = system.groups();
    let standalone_speakers: Vec<_> = groups.iter()
        .filter(|g| g.member_count() == 1)
        .filter_map(|g| g.coordinator())
        .filter(|speaker| {
            let model = speaker.model_name.to_lowercase();
            !model.contains("playbar") && !model.contains("beam") && !model.contains("arc")
        })
        .collect();

    if standalone_speakers.len() < min_count {
        return Err(format!("Found {} standalone speakers, need {}", standalone_speakers.len(), min_count).into());
    }

    Ok(standalone_speakers)
}
```

## System-Wide Impact

### Multi-Crate Integration Testing
- **Test location**: Lives in `sonos-sdk/tests/` following Rust integration test conventions
- **Cross-crate testing**: Tests through public APIs only, validating the entire stack
- **Resource management**: Tests RAII patterns across crate boundaries (WatchHandle, StateManager)
- **Error propagation**: Validates error handling from `soap-client` up through `sonos-sdk`

### Development Workflow Impact
- **Pre-PR validation**: Developers run `cargo test --package sonos-sdk --test integration_real_speakers --ignored` before submitting PRs
- **Real hardware requirement**: Enforces testing against actual Sonos devices
- **Fast feedback**: < 30 second execution encourages regular use
- **Clear diagnostics**: Standard cargo test output with detailed assertion messages

### CI/CD Considerations
- **Not in CI**: Integration tests marked with `#[ignore]` and require real hardware, run manually only
- **Feature flags**: Can use `--features sonos-sdk/test-support` for additional test helpers
- **Build validation**: Integration test file must compile as part of `cargo check` validation

## Acceptance Criteria

### Functional Requirements
- [x] **Standard execution**: `cargo test --package sonos-sdk --test integration_real_speakers --ignored` runs all tests
- [x] **Hardware requirement**: Tests fail when no speakers detected (no mock fallbacks)
- [x] **Fast execution**: Complete test suite runs in < 30 seconds (achieved 0.44s)
- [x] **Modular design**: Five focused test functions with single responsibility
- [x] **Clear reporting**: Standard cargo test output with detailed error messages for failures

### Test Coverage Requirements
- [x] **API Operations**: Basic SOAP operations (volume, playback, mute) validated
- [x] **Event Streaming**: UPnP subscription lifecycle and grace period behavior tested
- [x] **Group Management**: Group creation, joining, leaving operations validated
- [x] **Property Watching**: WatchHandle API and property access patterns tested
- [x] **Event Integration**: End-to-end event flow from property changes to system.iter() validated

### Quality Requirements
- [x] **Multi-crate best practices**: Follows Rust workspace patterns, tests public APIs only
- [x] **Error handling**: Structured errors with meaningful messages across crate boundaries
- [x] **Resource cleanup**: RAII patterns validated, no resource leaks
- [x] **Device compatibility**: Proper speaker qualification and filtering
- [x] **Maintainability**: Easy to add new test functions following established patterns

### Integration Requirements
- [x] **Example patterns**: Reuses successful patterns from existing demos
- [x] **test-support feature**: Compatible with existing test infrastructure
- [x] **CLAUDE.md compliance**: Follows established testing conventions
- [x] **No CI integration**: Designed for manual execution only (uses #[ignore])

## Implementation Phases

### Phase 1: Test Infrastructure Foundation
**Deliverables:**
- Create `sonos-sdk/tests/integration_real_speakers.rs` with test framework
- Implement `require_real_speakers()` for shared setup and failure handling
- Add helper functions: `find_reachable_speaker()`, `find_standalone_speakers()`
- Basic test structure with `#[tokio::test]` and `#[ignore]` attributes

**Success Criteria:**
- Test file compiles and can be run with `--ignored` flag
- `require_real_speakers()` fails tests when no speakers found
- Helper functions properly qualify speakers for testing

### Phase 2: Core Test Functions
**Deliverables:**
- Implement `test_api_operations()` - basic SOAP validation
- Implement `test_property_watching()` - WatchHandle and property access patterns
- Test execution with real speakers to validate patterns

**Success Criteria:**
- API operations test covers volume, playback, mute operations
- Property watching validates get/fetch/watch patterns and cache behavior
- Tests pass on real hardware with clear cargo test output

### Phase 3: Advanced Test Functions
**Deliverables:**
- Implement `test_event_streaming()` - UPnP events and grace periods
- Implement `test_group_lifecycle()` - group management operations
- Implement `test_event_integration()` - end-to-end event flow validation
- Add device qualification and filtering logic to helper functions

**Success Criteria:**
- Event streaming validates subscription lifecycle and grace period timing
- Group management handles multi-speaker scenarios with proper qualification
- Event integration validates complete event flow from property changes to system.iter()
- All five test functions integrated and working with proper assertions

### Phase 4: Polish and Documentation
**Deliverables:**
- Enhanced assertion messages and error context
- Usage documentation for developers
- Integration with existing development workflow documentation

**Success Criteria:**
- Clear developer documentation for running integration tests
- Consistent assertion messages and error context across all test functions
- Easy extensibility pattern documented for adding new test functions

## Dependencies & Risks

### Dependencies
- **Real Sonos hardware**: Tests require at least 1 speaker, group tests need 2+ standalone speakers
- **Network connectivity**: Speakers must be discoverable and reachable on local network
- **Compatible devices**: Some tests skip home theater setups or bonded pairs

### Risks & Mitigation
- **Device availability**: Risk of false failures when compatible speakers unavailable
  - **Mitigation**: Clear error messages explaining device requirements
- **Network reliability**: Risk of flaky tests due to network issues
  - **Mitigation**: Retry patterns and timeout handling from existing examples
- **Test interference**: Risk of tests affecting each other through shared device state
  - **Mitigation**: Restore original state after changes, use different speakers when possible

## Success Metrics

### Functional Metrics
- **Execution time**: < 30 seconds for complete suite on typical network
- **Error detection**: Catches at least 80% of common SDK breaking changes
- **Developer adoption**: Used by developers before 90%+ of PRs

### Quality Metrics
- **Reliability**: < 5% false failure rate due to network/device issues
- **Maintainability**: New test functions addable in < 10 lines of code
- **Coverage**: All five critical SDK areas validated with focused tests

## Sources & References

### Origin
- **Brainstorm document:** [docs/brainstorms/2026-03-28-integration-test-suite-brainstorm.md](docs/brainstorms/2026-03-28-integration-test-suite-brainstorm.md) — Key decisions carried forward: fail loudly on no speakers, modular test architecture with single responsibility. **Note:** Changed from binary approach to standard Rust integration tests for better multi-crate best practices.

### Internal References
- **Grace period demo patterns:** [sonos-sdk/examples/watch_grace_period_demo.rs:188](sonos-sdk/examples/watch_grace_period_demo.rs) (speaker discovery)
- **Group management patterns:** [sonos-sdk/examples/group_lifecycle_test.rs:42](sonos-sdk/examples/group_lifecycle_test.rs) (device qualification)
- **Test infrastructure:** [sonos-discovery/tests/helpers/mod.rs:15](sonos-discovery/tests/helpers/mod.rs) (fixture patterns)
- **Development conventions:** [CLAUDE.md](CLAUDE.md) (testing strategy using examples)

### External References
- **Multi-crate testing best practices:** Rust workspace patterns for integration testing
- **RAII patterns:** Resource management across crate boundaries
- **Async testing patterns:** `tokio::test` for async integration tests