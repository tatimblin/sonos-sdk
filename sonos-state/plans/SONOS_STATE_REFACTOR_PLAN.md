# Plan: Refactor sonos-state (Sync-First)

## Goal

Consolidate sonos-state from 6+ modules down to ~2-3, with sync-first design. Remove async complexity.

## Current State (Bloated)

| File | Lines | Problem |
|------|-------|---------|
| `reactive.rs` | 628 | Async wrapper over core StateManager |
| `state_manager.rs` | 160 | Core logic buried under async layer |
| `change_iterator.rs` | 1180 | Massive async-first iterator system |
| `watch_cache.rs` | 280 | Over-engineered async cleanup |
| `watcher.rs` | 100 | Just `block_on()` wrapper |
| `property_handle.rs` | 160 | Delegates to async WatchCache |
| `speaker_handle.rs` | 200 | Thin wrapper, 8x duplicated Arc refs |

**Total**: ~2700 lines across 7 modules for property access

## Target State (Lean)

| File | Lines | Purpose |
|------|-------|---------|
| `state.rs` | ~300 | StateManager + StateStore (merged) |
| `property.rs` | ~200 | PropertyHandle with get/fetch/watch |
| `speaker.rs` | ~150 | Speaker struct with property handles |
| `iter.rs` | ~150 | Sync ChangeIterator |

**Total**: ~800 lines across 4 modules

---

## Architecture: Sync-First

### Core Principle

All public APIs are synchronous. Internal event processing uses a background thread (not async runtime).

```
[Background Thread]                    [User Thread]

UPnP Events (callback-server)          speaker.volume.get()     -> StateStore read
    |                                  speaker.volume.watch()   -> StateStore read + register
    v                                  speaker.volume.fetch()   -> blocking HTTP call
std::sync::mpsc::Sender                system.iter()            -> mpsc::Receiver
    |                                      |
    +-------> StateStore <-----------------+
              (Arc<RwLock<HashMap>>)
```

### No Tokio in Public API

- No `async fn` in public API
- No `.await` required by users
- Background work uses `std::thread::spawn`, not tokio
- HTTP calls use `ureq` (blocking), not `reqwest`

---

## Files to Delete

| File | Reason |
|------|--------|
| `reactive.rs` | Replaced by sync `state.rs` |
| `watcher.rs` | No longer needed (was block_on wrapper) |
| `watch_cache.rs` | Merged into `state.rs` |
| `state_manager.rs` | Merged into `state.rs` |
| `change_iterator.rs` | Replaced by simple `iter.rs` |

## Files to Keep (Modified)

| File | Changes |
|------|---------|
| `property_handle.rs` → `property.rs` | Simplify, remove async |
| `speaker_handle.rs` → `speaker.rs` | Keep structure, remove Arc duplication |
| `store.rs` | Merge into `state.rs` |
| `property.rs` (traits) | Keep as-is |
| `decoders/` | Keep as-is |
| `model/` | Keep as-is |
| `error.rs` | Keep as-is |
| `lib.rs` | Update exports |

---

## New Module: `state.rs`

Merges: `state_manager.rs` + `reactive.rs` + `watch_cache.rs` + `store.rs`

```rust
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, RwLock, mpsc};
use std::thread;
use std::time::Duration;

use crate::model::{SpeakerId, SpeakerInfo};
use crate::property::Property;
use crate::{ChangeEvent, Result};

/// Core state manager - sync-first design
pub struct StateManager {
    /// Property values: (speaker_id, property_key) -> value
    store: Arc<RwLock<StateStore>>,

    /// Watched properties for iter() filtering
    watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,

    /// Service subscription ref counts
    subscriptions: Arc<RwLock<HashMap<(SpeakerId, Service), usize>>>,

    /// Channel for change events (to iter())
    event_tx: mpsc::Sender<ChangeEvent>,
    event_rx: Arc<Mutex<mpsc::Receiver<ChangeEvent>>>,

    /// Background event processor handle
    _worker: thread::JoinHandle<()>,
}

impl StateManager {
    /// Create new StateManager (sync)
    pub fn new() -> Result<Self> { ... }

    /// Create with builder
    pub fn builder() -> StateManagerBuilder { ... }

    /// Add discovered devices
    pub fn add_devices(&self, devices: Vec<Device>) -> Result<()> { ... }

    /// Get all speakers
    pub fn speakers(&self) -> Vec<Speaker> { ... }

    /// Get speaker by ID
    pub fn speaker(&self, id: &SpeakerId) -> Option<Speaker> { ... }

    /// Blocking iterator over change events (filtered by watched properties)
    pub fn iter(&self) -> ChangeIterator { ... }

    // === Internal methods ===

    /// Register a property as watched (called by PropertyHandle::watch)
    pub(crate) fn register_watch(&self, speaker_id: &SpeakerId, property_key: &'static str) { ... }

    /// Unregister a property watch
    pub(crate) fn unregister_watch(&self, speaker_id: &SpeakerId, property_key: &'static str) { ... }

    /// Get property value from store
    pub(crate) fn get_property<P: Property>(&self, speaker_id: &SpeakerId) -> Option<P> { ... }

    /// Set property value in store (called by background worker)
    pub(crate) fn set_property<P: Property>(&self, speaker_id: &SpeakerId, value: P) { ... }
}

pub struct StateManagerBuilder {
    cleanup_timeout: Duration,
}

impl StateManagerBuilder {
    pub fn cleanup_timeout(mut self, timeout: Duration) -> Self { ... }
    pub fn build(self) -> Result<StateManager> { ... }
}

/// Internal state store
struct StateStore {
    /// Speaker metadata
    speakers: HashMap<SpeakerId, SpeakerInfo>,
    /// Property values: type-erased storage
    properties: HashMap<(SpeakerId, &'static str), Box<dyn Any + Send + Sync>>,
}
```

---

## New Module: `property.rs`

Simplified PropertyHandle - all sync.

```rust
use std::net::IpAddr;
use std::sync::Arc;
use std::marker::PhantomData;

use sonos_api::SonosClient;
use crate::state::StateManager;
use crate::property::Property;
use crate::Result;

/// Handle for accessing a speaker property
pub struct PropertyHandle<P: Property> {
    speaker_id: SpeakerId,
    speaker_ip: IpAddr,
    state_manager: Arc<StateManager>,
    api_client: SonosClient,
    _phantom: PhantomData<P>,
}

impl<P: Property> PropertyHandle<P> {
    /// Get current cached value (instant, no network)
    pub fn get(&self) -> Option<P> {
        self.state_manager.get_property::<P>(&self.speaker_id)
    }

    /// Fetch fresh value from device (blocking network call)
    pub fn fetch(&self) -> Result<P> {
        // 1. Call sonos-api operation based on P::SERVICE + P::KEY
        // 2. Update state_manager
        // 3. Return value
        todo!("Initial scope: Volume only")
    }

    /// Register for change events and return current value
    ///
    /// After calling watch(), changes to this property will appear in iter().
    pub fn watch(&self) -> Result<Option<P>> {
        // 1. Ensure service subscription exists (ref count)
        // 2. Register (speaker_id, P::KEY) in watched set
        // 3. Return current value
        self.state_manager.register_watch(&self.speaker_id, P::KEY);
        Ok(self.get())
    }

    /// Stop watching this property
    pub fn unwatch(&self) {
        self.state_manager.unregister_watch(&self.speaker_id, P::KEY);
    }
}

impl<P: Property> Clone for PropertyHandle<P> { ... }
impl<P: Property> Drop for PropertyHandle<P> {
    fn drop(&mut self) {
        // Optionally auto-unwatch on drop
        // Or require explicit unwatch() - TBD
    }
}
```

---

## New Module: `iter.rs`

Simple sync iterator.

```rust
use std::sync::mpsc;
use crate::ChangeEvent;

/// Blocking iterator over property change events
pub struct ChangeIterator {
    rx: mpsc::Receiver<ChangeEvent>,
}

impl Iterator for ChangeIterator {
    type Item = ChangeEvent;

    fn next(&mut self) -> Option<Self::Item> {
        // Blocks until event or channel closed
        self.rx.recv().ok()
    }
}

impl ChangeIterator {
    /// Non-blocking: get all currently available events
    pub fn try_iter(&self) -> impl Iterator<Item = ChangeEvent> + '_ {
        std::iter::from_fn(|| self.rx.try_recv().ok())
    }

    /// Blocking with timeout
    pub fn recv_timeout(&self, timeout: Duration) -> Option<ChangeEvent> {
        self.rx.recv_timeout(timeout).ok()
    }
}

/// A change event for a watched property
#[derive(Debug, Clone)]
pub struct ChangeEvent {
    pub speaker_id: SpeakerId,
    pub property_key: &'static str,
    pub timestamp: Instant,
}
```

---

## New Module: `speaker.rs`

Simplified Speaker struct.

```rust
use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::SonosClient;
use crate::state::StateManager;
use crate::property::PropertyHandle;
use crate::model::{SpeakerId, SpeakerInfo};
use crate::{Volume, Mute, Bass, Treble, Loudness, PlaybackState, Position, CurrentTrack, GroupMembership};

/// Handle for a Sonos speaker
#[derive(Clone)]
pub struct Speaker {
    // Metadata
    pub id: SpeakerId,
    pub name: String,
    pub room_name: String,
    pub ip_address: IpAddr,
    pub port: u16,
    pub model_name: String,

    // Property handles (9 total)
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

impl Speaker {
    pub(crate) fn new(
        info: SpeakerInfo,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        // Create all property handles sharing the same Arc<StateManager>
        // No more Arc<WatchCache> - that's internal to StateManager now
        ...
    }
}
```

---

## Background Event Processing

Replace tokio async with std::thread.

```rust
impl StateManager {
    fn spawn_event_worker(
        store: Arc<RwLock<StateStore>>,
        watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
        event_tx: mpsc::Sender<ChangeEvent>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            // Setup: create callback server or polling mechanism
            // This may internally use tokio for the HTTP server,
            // but that's an implementation detail hidden from users

            loop {
                // Receive raw UPnP event (blocking)
                let raw_event = receive_upnp_event(); // blocking call

                // Decode into property changes
                let changes = decode_event(raw_event);

                // Update store
                for (speaker_id, property_key, value) in changes {
                    store.write().unwrap().set(speaker_id, property_key, value);

                    // Check if watched
                    if watched.read().unwrap().contains(&(speaker_id, property_key)) {
                        let _ = event_tx.send(ChangeEvent {
                            speaker_id,
                            property_key,
                            timestamp: Instant::now(),
                        });
                    }
                }
            }
        })
    }
}
```

---

## Migration: What Happens to Existing Code

### sonos-stream dependency

Current `reactive.rs` depends on `sonos-stream` for event streaming. Options:

1. **Keep sonos-stream internal**: Background thread uses sonos-stream internally, but doesn't expose async to users
2. **Replace with blocking**: If sonos-stream is async-only, consider blocking alternative

Recommendation: Option 1 - hide async internally.

### callback-server dependency

The callback server (warp-based) is inherently async. Options:

1. **Run in background thread with own tokio runtime**: Isolate async to internal implementation
2. **Use polling fallback only**: Remove callback server, always poll

Recommendation: Option 1 - keep callback server but isolate.

```rust
fn spawn_event_worker(...) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        // Create isolated tokio runtime for internal async work
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            // Internal async event loop
            // But users never see this
        });
    })
}
```

---

## Implementation Sequence

### Phase 1: Create new modules (don't delete old yet)

1. Create `state.rs` with StateManager (sync API)
2. Create `iter.rs` with ChangeIterator
3. Update `property.rs` (rename from property_handle.rs, make sync)
4. Update `speaker.rs` (rename from speaker_handle.rs, simplify)

### Phase 2: Migrate internals

1. Move StateStore into `state.rs`
2. Move watched property tracking from WatchCache into StateManager
3. Implement background event worker
4. Wire up iter() to mpsc channel

### Phase 3: Delete old modules

1. Delete `reactive.rs`
2. Delete `watcher.rs`
3. Delete `watch_cache.rs`
4. Delete `state_manager.rs`
5. Delete `change_iterator.rs`

### Phase 4: Update exports

1. Update `lib.rs` exports
2. Update sonos-sdk to use new API
3. Run tests

---

## API Comparison

### Before (Async)

```rust
// Create system (async)
let manager = StateManager::new().await?;
manager.add_devices(devices).await?;

// Watch property (async)
let volume = speaker.volume.watch().await?;

// Iterate (async stream or block_on wrapper)
let mut stream = manager.change_stream();
while let Some(event) = stream.next().await {
    ...
}
```

### After (Sync)

```rust
// Create system (sync)
let manager = StateManager::new()?;
manager.add_devices(devices)?;

// Watch property (sync)
let volume = speaker.volume.watch()?;

// Iterate (blocking)
for event in manager.iter() {
    ...
}

// Or non-blocking
for event in manager.iter().try_iter() {
    ...
}
```

---

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| sonos-stream is async-only | Isolate in background thread with own runtime |
| callback-server needs async | Same - isolate in background thread |
| Breaking change for existing users | sonos-sdk is the public API, internal changes OK |
| Loss of async flexibility | Can add async wrappers later if needed |

---

## Success Criteria

- [ ] No `async fn` in public API
- [ ] No `.await` required by users
- [ ] `iter()` is blocking (std::sync::mpsc)
- [ ] Total lines reduced from ~2700 to ~800
- [ ] Module count reduced from 7 to 4
- [ ] All existing tests pass (adapted for sync)
- [ ] ratatui example works with sync iter()
