# Integration Test Suite for Real Speaker Validation

**Date:** 2026-03-28
**Context:** Building on the `watch_grace_period_demo.rs` example to create a comprehensive integration test suite for pre-PR validation

## What We're Building

A cargo test-based integration test suite that validates core Sonos SDK functionality against real speakers before PR submission. The suite will catch breaking changes by running fast smoke tests across critical SDK areas.

## Why This Approach

**Problem:** Need to catch breaking changes in core SDK functionality before PRs reach production
**Solution:** Manual pre-PR validation using real Sonos hardware to ensure functionality works as expected

**Key Requirements:**
- Fail loudly when no real speakers available (no mock fallbacks)
- Fast smoke tests (not comprehensive functional testing)
- Single, simple command to run all tests
- Integration with existing cargo test infrastructure

## Key Decisions

### Test Organization: Single Integration Binary
- Create `sonos-sdk/src/bin/integration-tests.rs` as a standalone binary
- Run with simple command: `cargo run --bin integration-tests`
- Discover speakers at startup and fail loudly if none found
- Sequential test execution with clear progress reporting
- Leverage existing patterns from demo examples and codebase

### Focused Test Modules (Single Responsibility)
1. **API Operations** (`test_api_operations`) - All SOAP API calls work (volume, playback, discovery)
2. **Event Streaming** (`test_event_streaming`) - UPnP events, subscription lifecycle, grace periods
3. **Group Management** (`test_group_lifecycle`) - Group creation, joining, leaving, dissolution
4. **Property Watching** (`test_property_watching`) - WatchHandle API, get/fetch/watch patterns

Each test module has a single focus area with no overlap, making failures easy to diagnose and new tests easy to add.

### Development Workflow: Manual Pre-PR Validation
- Developer runs tests manually before submitting PRs
- Single command execution for all integration tests
- Clear pass/fail reporting with detailed error information
- No CI integration (requires real hardware)

### Speaker Availability Handling: Fail Loudly
- Exit with error when no Sonos speakers detected
- Force developers to test against real hardware
- No graceful fallback to mocks (different from existing codebase pattern)
- Clear error messages explaining speaker requirement

### Validation Level: Fast Smoke Tests
- Quick validation of core functionality (not deep testing)
- Focus on "does it work" rather than "does it work perfectly"
- Fast feedback loop for pre-PR workflow
- Skip comprehensive edge case testing and performance benchmarks

## Technical Foundation

### Modular Test Design Benefits
- **Single Responsibility**: Each test module focuses on one SDK area
- **Easy Diagnosis**: Test failures are isolated to specific functionality
- **Simple Extension**: Adding new tests requires only a function + one line in main()
- **No Overlap**: Clear boundaries prevent duplicate testing and maintenance burden
- **Parallel Development**: Multiple developers can work on different test modules independently

### Adding New Focused Tests
```rust
// To add a new test area, create a focused function:
async fn test_new_feature(system: &SonosSystem) -> Result<(), Box<dyn std::error::Error>> {
    // Single-focus testing logic here
    Ok(())
}

// Then add one line to main():
run_test("New Feature", || test_new_feature(&system)).await?;
```

### Existing Patterns to Leverage
- **Device Discovery**: Use `SonosSystem::new()` and `speaker_names()` patterns from examples
- **Device Qualification**: Follow `group_lifecycle_test.rs` patterns for filtering compatible speakers
- **Error Handling**: Adapt timeout and polling patterns from existing examples
- **Progress Reporting**: Use clear console output patterns from existing demo examples

### Modular Test Architecture
```rust
// sonos-sdk/src/bin/integration-tests.rs
use sonos_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎵 Sonos SDK Integration Tests");

    // Shared setup: discover speakers or fail loudly
    let system = SonosSystem::new()?;
    if system.speaker_names().is_empty() {
        eprintln!("❌ No Sonos speakers found. Integration tests require real hardware.");
        std::process::exit(1);
    }

    // Run focused test modules - each has single responsibility
    run_test("API Operations", || test_api_operations(&system)).await?;
    run_test("Event Streaming", || test_event_streaming(&system)).await?;
    run_test("Group Management", || test_group_lifecycle(&system)).await?;
    run_test("Property Watching", || test_property_watching(&system)).await?;

    println!("✅ All integration tests passed!");
    Ok(())
}

// Helper for consistent test reporting
async fn run_test<F, Fut>(name: &str, test_fn: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    print!("  Testing {name}... ");
    match test_fn().await {
        Ok(()) => println!("✅ PASS"),
        Err(e) => {
            println!("❌ FAIL");
            eprintln!("    Error: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}
```

### Code Reuse Strategy
- **Grace Period Demo**: Adapt successful patterns from `watch_grace_period_demo.rs`
- **Group Lifecycle Example**: Reuse device qualification and operation patterns from `group_lifecycle_test.rs`
- **Discovery Examples**: Follow timeout and error handling patterns from existing examples

## Implementation Strategy

### Phase 1: Framework Foundation
- Create `sonos-sdk/src/bin/integration-tests.rs` with modular test runner
- Implement shared speaker discovery and test execution framework
- Add `run_test()` helper for consistent reporting

### Phase 2: Core Test Modules (Focused Implementation)
- `test_api_operations()` - Validate all SOAP operations work
- `test_event_streaming()` - Validate UPnP events and subscriptions
- `test_group_lifecycle()` - Validate group management operations
- `test_property_watching()` - Validate WatchHandle and property access

### Phase 3: Easy Extensibility
- Document pattern for adding new focused test modules
- Each new test follows: single concern, clear pass/fail, reuses shared setup
- Adding tests requires only: new function + one line in main()

## Success Criteria

### Functional Requirements
- ✅ Simple command: `cargo run --bin integration-tests`
- ✅ Tests fail loudly when no speakers available
- ✅ Fast execution (< 30 seconds for full suite)
- ✅ Clear pass/fail reporting with detailed errors
- ✅ Covers core SDK functionality for breaking change detection

### Quality Requirements
- ✅ Follows existing demo and example patterns
- ✅ Reuses established SDK usage patterns
- ✅ Simple binary approach - easy to maintain and extend
- ✅ Clear developer workflow integration

## Open Questions

*No open questions at this time - requirements are well-defined.*

## Next Steps

1. **Planning Phase**: Create detailed implementation plan with specific test cases and technical approach
2. **Foundation Implementation**: Set up test infrastructure and speaker discovery/qualification
3. **Incremental Test Development**: Build out test modules following the established patterns
4. **Integration and Polish**: Ensure smooth workflow integration and clear reporting