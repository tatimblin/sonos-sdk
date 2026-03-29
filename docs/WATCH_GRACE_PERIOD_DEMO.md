# WatchHandle Grace Period Demo

This document explains the new RAII WatchHandle with 50ms grace period feature introduced in PR #59 and how to validate it using the enhanced demonstration example.

## What's New in This PR

### RAII WatchHandle Pattern
- **Before**: `watch()` returned `WatchStatus<P>`, required manual `unwatch()` calls
- **After**: `watch()` returns `WatchHandle<P>`, automatically unsubscribes when dropped

### 50ms Grace Period
- When a `WatchHandle` is dropped, there's a 50ms delay before the underlying UPnP subscription is terminated
- If another `WatchHandle` for the same property is created within this grace period, the existing subscription is reused
- **Key benefit**: Prevents subscription churn in TUI applications that call `watch()` inside draw methods

### Breaking Changes
- `watch()` now returns `WatchHandle<P>` instead of `WatchStatus<P>`
- `unwatch()` method removed from `PropertyHandle` and `GroupPropertyHandle`
- `WatchHandle` uses `value()` / `mode()` methods instead of public fields

## Running the Grace Period Demo

### Prerequisites
1. Ensure you have Sonos speakers on your local network
2. Make sure the speakers are discoverable (not in sleep mode)

### Running the Demo
```bash
# From the project root
cargo run -p sonos-sdk --example watch_grace_period_demo
```

### What the Demo Shows

#### Demo 1: Normal Usage Pattern
Shows the standard lifecycle of creating a watch handle, using it, and letting it drop naturally.

#### Demo 2: TUI Pattern (The Key Innovation)
Simulates a TUI application calling `watch()` inside a draw method repeatedly:
- Creates 10 rapid watch handles (simulating 60 FPS rendering)
- Each handle is dropped at the end of the frame
- **Without grace period**: Would cause 9 unsubscribe/resubscribe cycles (expensive!)
- **With grace period**: Subscription is maintained throughout, preventing churn

#### Demo 3: Grace Period Timing
Demonstrates the exact 50ms timing:
- Creates a handle, drops it (grace period starts)
- Creates a new handle within 25ms - subscription is reused
- Shows that the subscription was never interrupted

#### Demo 4: Subscription Persistence
Shows how multiple overlapping watches share the same subscription and how the grace period manages cleanup.

### Sample Output

```
🎵 Sonos SDK Grace Period Demo
===============================

This demo shows how the 50ms grace period prevents subscription churn
when WatchHandles are created and dropped rapidly (like in TUI apps).

🔍 Finding reachable speaker...
   ✅ Kitchen at 192.168.1.100 is reachable

📋 Demo 1: Normal Usage Pattern
--------------------------------
Creating a watch handle and keeping it alive...
   ✅ Watch handle created - mode: Events
   📊 Current volume: 25%
   ⏱️  Keeping handle alive for 2 seconds...
   🗑️  Dropping handle (grace period starts)...
   ✅ Handle dropped - subscription will cleanup after 50ms grace period

🖥️  Demo 2: TUI Pattern (Rapid Watch Creation/Dropping)
--------------------------------------------------------
Simulating a TUI app calling watch() inside draw() method...
This would cause subscription churn WITHOUT the grace period.

   🖼️  Frame  1: Volume: 25% | Handle mode: Events
      ⏱️  Frame rendered in 2.1ms
   🖼️  Frame  2: Volume: 25% | Handle mode: Events
      ⏱️  Frame rendered in 651µs
   ...
   🖼️  Frame 10: Volume: 25% | Handle mode: Events
      ⏱️  Frame rendered in 423µs

   ✅ 10 frames rendered in 184.7ms
   🎯 Grace period prevented 9 unnecessary unsubscribe/resubscribe cycles!
```

## Validating the PR

### Automated Testing
```bash
# Run tests with test-support feature
cargo test -p sonos-sdk --features test-support

# Build all examples
cargo build -p sonos-sdk --examples

# Check formatting and linting
cargo fmt --check
cargo clippy -- -D warnings
```

### Manual Validation Steps

1. **Run the grace period demo** against real speakers to see the feature in action
2. **Monitor network traffic** (optional) to verify subscription churn is eliminated
3. **Test TUI scenarios** by rapidly creating/dropping watches
4. **Verify property access** still works correctly with the new API

### Integration with Existing Code

The new API is mostly backward-compatible for reading:
```rust
// OLD API (still works)
let handle = speaker.volume.watch()?;
if let Some(vol) = handle.value() {
    println!("Volume: {}%", vol.0);
}

// NEW CAPABILITIES
println!("Watch mode: {}", handle.mode());
println!("Has realtime events: {}", handle.has_realtime_events());
```

### Key Benefits Demonstrated

1. **Resource Efficiency**: No more subscription churn in TUI applications
2. **RAII Safety**: No manual cleanup required - handles clean up automatically
3. **Backward Compatibility**: Existing property access patterns still work
4. **Better UX**: Smoother TUI applications without subscription delays

## Architecture Impact

### Reference-Counted Observable Pattern
- First property watcher creates UPnP subscription (ref count 0→1)
- Multiple watchers share same subscription without duplication
- Last watcher dropping triggers cleanup (ref count 1→0)
- Grace period provides hysteresis to prevent rapid ref count fluctuations

### Multi-Layer Integration
```
SDK User Code
    ↓
WatchHandle<P> (RAII + Grace Period)
    ↓
StateManager (Property Management)
    ↓
SonosEventManager (Reference Counting)
    ↓
SonosStream (UPnP Events/Polling)
    ↓
UPnP SOAP Operations
```

This enhancement makes the Sonos SDK much more suitable for interactive applications while maintaining all existing functionality.