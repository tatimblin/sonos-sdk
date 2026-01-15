# SDK Cleanup: Updated Implementation Guide

This document explains how the SDK_CLEANUP_PLAN.md should now be implemented following the sonos-state sync-first refactor.

## Overview

The sonos-state refactor (SONOS_STATE_REFACTOR_PLAN.md) significantly changed the architecture:

| Aspect | Before Refactor | After Refactor |
|--------|-----------------|----------------|
| StateManager API | Async (`new().await`) | Sync (`new()`) |
| Event consumption | `change_stream()` async | `iter()` blocking sync |
| Property watching | `PropertyWatcher` with `.changed().await` | `watch()` returns current value, changes via `iter()` |
| Internal storage | `tokio::sync::watch` channels | `std::sync::mpsc` + `RwLock<HashMap>` |
| Modules | 7 modules (~2700 lines) | 4 modules (~800 lines) |

This makes the SDK_CLEANUP_PLAN.md significantly simpler to implement.

---

## Part 1: Changes Already Complete in sonos-state

These items from the plan are **already implemented**:

### StateManager is Sync-First
```rust
// Plan said: StateManagerBuilder with cleanup_timeout
// Reality: Already implemented in state.rs

let manager = StateManager::new()?;  // Sync, no .await
// OR with builder:
let manager = StateManager::builder()
    .cleanup_timeout(Duration::from_secs(300))
    .build()?;
```

### Speaker Has 9 Properties
```rust
// Plan said: Speaker should have 9 PropertyHandle<P> fields
// Reality: Already implemented in speaker.rs

pub struct Speaker {
    pub id: SpeakerId,
    pub name: String,
    pub room_name: String,
    pub ip_address: IpAddr,
    pub port: u16,
    pub model_name: String,

    pub volume: PropertyHandle<Volume>,
    pub mute: PropertyHandle<Mute>,
    pub bass: PropertyHandle<Bass>,
    pub treble: PropertyHandle<Treble>,
    pub loudness: PropertyHandle<Loudness>,
    pub playback_state: PropertyHandle<PlaybackState>,
    pub position: PropertyHandle<Position>,
    pub current_track: PropertyHandle<CurrentTrack>,
    pub group_membership: PropertyHandle<GroupMembership>,
}
```

### PropertyHandle has get/watch/fetch Pattern
```rust
// Plan said: PropertyHandle<P> with get(), watch(), fetch()
// Reality: Already implemented in speaker.rs

impl<P: Property> PropertyHandle<P> {
    pub fn get(&self) -> Option<P>;               // Cached, instant
    pub fn watch(&self) -> Result<Option<P>>;     // Register for iter() + return current
    pub fn unwatch(&self);                        // Unregister
    pub fn is_watched(&self) -> bool;             // Check status
    pub fn fetch(&self) -> Result<Option<P>>;     // Currently stubbed (returns cached)
}
```

### ChangeIterator for Blocking Consumption
```rust
// Plan said: iter() should be blocking with std::sync::mpsc
// Reality: Already implemented in iter.rs

let iter = manager.iter();

// Multiple patterns:
for event in iter { }                                    // Blocking forever
for event in iter.try_iter() { }                         // Non-blocking batch
if let Some(e) = iter.recv_timeout(Duration::from_secs(1)) { }  // With timeout
```

### WatchCache Merged into StateManager
```rust
// Plan said: Extend WatchCache or merge into StateManager
// Reality: WatchCache is gone, functionality is in StateManager

// StateManager now has:
watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>
```

---

## Part 2: sonos-sdk Changes Required

The sonos-sdk crate is **out of date** and still uses async patterns that no longer exist in sonos-state.

### Current sonos-sdk State (Broken)

```rust
// system.rs - BROKEN: uses async APIs that don't exist
StateManager::new().await    // StateManager::new() is now sync
state_manager.add_devices(devices).await  // add_devices() is now sync

// property/handles.rs - BROKEN: uses PropertyWatcher that doesn't exist
pub async fn watch(&self) -> Result<PropertyWatcher<$property_type>, SdkError>
// PropertyWatcher was removed in the refactor

// lib.rs - BROKEN: re-exports types that don't exist
pub use sonos_state::{PropertyWatcher, ...};  // PropertyWatcher is gone
```

### Required Changes to sonos-sdk

#### 1. Delete Redundant Files

| File | Action | Reason |
|------|--------|--------|
| `sonos-sdk/src/speaker.rs` | Delete | Use `sonos_state::Speaker` |
| `sonos-sdk/src/property/mod.rs` | Delete | Use `sonos_state::PropertyHandle` |
| `sonos-sdk/src/property/handles.rs` | Delete | Use `sonos_state::PropertyHandle` |

#### 2. Update `sonos-sdk/src/lib.rs`

```rust
//! # Sonos SDK - Clean API for Sonos Control

// Main entry point
pub use system::SonosSystem;

// Re-export from sonos-state (the source of truth)
pub use sonos_state::{
    // Types
    Speaker, PropertyHandle, SpeakerId, GroupId, SpeakerInfo,
    ChangeEvent, ChangeIterator,
    StateManager, StateManagerBuilder,

    // Properties
    Volume, Mute, Bass, Treble, Loudness,
    PlaybackState, Position, CurrentTrack, GroupMembership, Topology,

    // Error
    StateError,
};

// SDK-specific error
pub use error::SdkError;

mod error;
mod system;
```

#### 3. Update `sonos-sdk/src/system.rs`

```rust
use std::sync::Arc;
use sonos_state::{StateManager, Speaker, SpeakerId, ChangeIterator};
use sonos_discovery;
use crate::SdkError;

/// Main system entry point
pub struct SonosSystem {
    state_manager: Arc<StateManager>,
}

impl SonosSystem {
    /// Create with automatic device discovery (SYNC - no .await)
    pub fn new() -> Result<Self, SdkError> {
        let state_manager = Arc::new(StateManager::new().map_err(SdkError::StateError)?);

        let devices = sonos_discovery::get();
        state_manager.add_devices(devices).map_err(SdkError::StateError)?;

        Ok(Self { state_manager })
    }

    /// Do not allow create from pre-discovered devices

    /// Get all speakers (SYNC)
    pub fn speakers(&self) -> Vec<Speaker> {
        self.state_manager.speakers()
    }

    /// Get speaker by ID (SYNC)
    pub fn get_speaker_by_id(&self, id: &SpeakerId) -> Option<Speaker> {
        self.state_manager.speaker(id)
    }

    /// Get speaker by name (SYNC)
    pub fn get_speaker_by_name(&self, name: &str) -> Option<Speaker> {
        self.state_manager.speakers()
            .into_iter()
            .find(|s| s.name == name)
    }

    /// Blocking iterator over change events
    pub fn iter(&self) -> ChangeIterator {
        self.state_manager.iter()
    }
}
```

#### 4. Update `sonos-sdk/src/error.rs`

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SdkError {
    #[error("State error: {0}")]
    StateError(#[from] sonos_state::StateError),

    #[error("API error: {0}")]
    ApiError(#[from] sonos_api::ApiError),

    #[error("Discovery error: {0}")]
    DiscoveryError(#[from] sonos_discovery::DiscoveryError),

    #[error("Speaker not found: {0}")]
    SpeakerNotFound(String),
}
```

---

## Part 3: Items Still TODO in sonos-state

These items from the original plan still need implementation:

### 1. `fetch()` Implementation (Stubbed)

Current state in `sonos-state/src/speaker.rs:137-140`:
```rust
// TODO: Implement actual fetch via sonos-api
// For now, return cached value
pub fn fetch(&self) -> Result<Option<P>> {
    Ok(self.get())
}
```

Implementation needed:
```rust
pub fn fetch(&self) -> Result<P> {
    // Match on P::KEY to determine which sonos-api operation to call
    // For Volume: call rendering_control::GetVolume
    // For PlaybackState: call av_transport::GetTransportInfo
    // etc.

    let value = match P::KEY {
        "volume" => {
            // Call sonos-api GetVolume operation
            // Update state_manager
            // Return value
        }
        // ... other properties
        _ => return Err(StateError::FetchNotSupported(P::KEY)),
    };

    Ok(value)
}
```

**Recommendation**: Start with Volume only (as original plan suggested). Add others incrementally.

### 2. Reference-Counted Subscriptions (Infrastructure Exists, Not Wired)

The `subscriptions` field exists in StateManager but isn't actively used:
```rust
// state.rs
subscriptions: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>,
```

This should manage actual UPnP subscriptions:
- Increment ref count when first property of a service is watched
- Decrement when last property of a service is unwatched
- Only send UPnP SUBSCRIBE when ref count goes 0 -> 1
- Only send UPnP UNSUBSCRIBE when ref count goes 1 -> 0

### 3. Background Worker Thread (Not Implemented)

The `_worker` field exists but is set to `None`:
```rust
// state.rs
_worker: Option<JoinHandle<()>>,
// Set to: _worker: None
```

If real-time UPnP events are desired, this needs:
- Thread that runs callback-server (or polling) internally
- Receives UPnP events
- Updates StateManager store
- Sends to event_tx channel for iter()

**Note**: The sync-first refactor intentionally deferred this. The current implementation works but without live UPnP events.

---

## Part 4: Updated API Usage Example

```rust
use sonos_sdk::{SonosSystem, Volume, PlaybackState};

fn main() -> Result<(), sonos_sdk::SdkError> {
    // Create system (SYNC - no tokio, no .await)
    let system = SonosSystem::new()?;

    // Get speaker
    let speaker = system.get_speaker_by_name("Living Room")
        .ok_or_else(|| sonos_sdk::SdkError::SpeakerNotFound("Living Room".into()))?;

    // Property access patterns:

    // 1. get() - instant cached read
    if let Some(vol) = speaker.volume.get() {
        println!("Cached volume: {}", vol.0);
    }

    // 2. watch() - register for changes + get current
    let current = speaker.volume.watch()?;
    println!("Watching volume, current: {:?}", current);

    // 3. iter() - blocking event loop (for ratatui, etc.)
    for event in system.iter() {
        println!("Property changed: {} on {:?}", event.property_key, event.speaker_id);

        // Re-read the property
        if event.property_key == "volume" {
            if let Some(vol) = speaker.volume.get() {
                println!("New volume: {}", vol.0);
            }
        }
    }

    Ok(())
}
```

---

## Part 5: Implementation Sequence

### Phase 1: Make sonos-sdk Compile (Critical)

1. Delete `sonos-sdk/src/speaker.rs`
2. Delete `sonos-sdk/src/property/` directory
3. Update `sonos-sdk/src/lib.rs` with re-exports from sonos-state
4. Update `sonos-sdk/src/system.rs` to sync API
5. Update `sonos-sdk/src/error.rs`
6. Run `cargo build -p sonos-sdk` - fix any remaining issues

### Phase 2: Verify Examples Work

1. Update any sonos-sdk examples to use new sync API
2. Test `get()` returns cached values
3. Test `watch()` registers and `iter()` emits events
4. Run `cargo test -p sonos-sdk`

### Phase 3: Implement fetch() (Optional, Can Defer)

1. Add `fetch()` implementation for Volume in sonos-state
2. Wire up sonos-api GetVolume operation
3. Test fetch updates cache and returns fresh value
4. Add other properties incrementally

### Phase 4: Wire Up UPnP Events (Optional, Can Defer)

1. Implement background worker thread in StateManager
2. Integrate with callback-server or polling mechanism
3. Test real-time event propagation through iter()

---

## Summary: What Changed From Original Plan

| Plan Item | Original Approach | Updated Approach |
|-----------|-------------------|------------------|
| Async-to-sync bridge | Complex tokio::mpsc bridging | Not needed - sonos-state is already sync |
| WatchCache extensions | Extend WatchCache for iter() filtering | WatchCache removed, `watched` HashSet in StateManager |
| PropertyWatcher | Keep and adapt for sync | Removed entirely, use watch() + iter() |
| SonosSystemBuilder | Create separate builder | Re-export StateManagerBuilder (no separate builder) |
| Speaker type | Re-export from sonos-state | Re-export from sonos-state (confirmed) |
| iter() implementation | Complex async-to-sync bridge | Already implemented with std::sync::mpsc |

The refactor made the SDK cleanup **much simpler** - it's now primarily about deleting redundant code from sonos-sdk and re-exporting from sonos-state.
