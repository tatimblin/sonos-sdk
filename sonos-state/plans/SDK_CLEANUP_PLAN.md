# Plan: Cleanup and Extend sonos-sdk

## Overview

Simplify sonos-sdk to be a minimal wrapper over sonos-state, with `SonosSystem` as the main entry point that provides `get_speakers()`, `get_speaker_by_id()`, and a blocking `iter()` for ratatui render loops.

## Key Design Decisions

1. **sonos-sdk is a thin wrapper** - Re-exports `Speaker` from sonos-state, no local Speaker type
2. **iter() is blocking/synchronous** - For ratatui render loops, not async. Sync APIs are preferred throughout.
3. **watch() registers properties in WatchCache** - Calls to `watch()` add property to WatchCache which filters `iter()` output
4. **Configurable cleanup timeout** - Via `StateManagerBuilder` (re-exported from sonos-state)
5. **Reference-counted service subscriptions** - Multiple property watchers share UPnP subscriptions via `PropertySubscriptionManager`

---

## Breaking Changes

This refactor introduces breaking changes for existing sonos-sdk consumers.

### Speaker Type Migration

The current `sonos-sdk::Speaker` has **2 properties**:
- `volume: VolumeHandle`
- `playback_state: PlaybackStateHandle`

The new `sonos-state::Speaker` (re-exported) has **9 properties**:
- `volume: PropertyHandle<Volume>`
- `mute: PropertyHandle<Mute>`
- `bass: PropertyHandle<Bass>`
- `treble: PropertyHandle<Treble>`
- `loudness: PropertyHandle<Loudness>`
- `playback_state: PropertyHandle<PlaybackState>`
- `position: PropertyHandle<Position>`
- `current_track: PropertyHandle<CurrentTrack>`
- `group_membership: PropertyHandle<GroupMembership>`

### Migration Guide

| Change | Old (sonos-sdk) | New (sonos-state re-export) |
|--------|-----------------|----------------------------|
| Field name | `speaker.ip` | `speaker.ip_address` |
| Property handle type | `VolumeHandle` | `PropertyHandle<Volume>` |
| Available properties | 2 | 9 |
| Import path | `sonos_sdk::Speaker` | `sonos_sdk::Speaker` (re-export) |

**New properties available**: Mute, Bass, Treble, Loudness, Position, CurrentTrack, GroupMembership

---

## Files to Modify

### sonos-sdk (simplify)

| File | Action | Changes |
|------|--------|---------|
| `sonos-sdk/src/lib.rs` | Modify | Re-export Speaker, PropertyHandle, StateManagerBuilder from sonos-state; remove local modules |
| `sonos-sdk/src/system.rs` | Modify | Add `get_speakers()`, `get_speaker_by_id()`, `iter()`; remove SonosSystemBuilder |
| `sonos-sdk/src/speaker.rs` | Delete | Use sonos-state::Speaker instead |
| `sonos-sdk/src/property/` | Delete | Use sonos-state::PropertyHandle instead |
| `sonos-sdk/src/error.rs` | Modify | Define proper error hierarchy wrapping errors from sonos-state, sonos-api, sonos-discovery |

### sonos-state (extend)

| File | Action | Changes |
|------|--------|---------|
| `sonos-state/src/reactive.rs` | Modify | Add `StateManagerBuilder`, `iter()` method |
| `sonos-state/src/watch_cache.rs` | Modify | Extend for `iter()` filtering support |
| `sonos-state/src/property_handle.rs` | Modify | Add `fetch()` method (Volume only for initial scope) |
| `sonos-state/src/speaker_handle.rs` | Modify | Add IP address and API client for `fetch()` support |
| `sonos-state/src/lib.rs` | Modify | Export new public types including StateManagerBuilder |

---

## New Types

### StateManagerBuilder (sonos-state)

Re-exported by sonos-sdk. No separate `SonosSystemBuilder` needed.

```rust
pub struct StateManagerBuilder {
    cleanup_timeout: Duration,
}

impl StateManagerBuilder {
    pub fn new() -> Self;
    pub fn cleanup_timeout(mut self, timeout: Duration) -> Self;
    pub fn build(self) -> Result<StateManager, StateError>;  // sync preferred
}
```

### Error Hierarchy (sonos-sdk)

```rust
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("State error: {0}")]
    State(#[from] sonos_state::StateError),

    #[error("API error: {0}")]
    Api(#[from] sonos_api::ApiError),

    #[error("Discovery error: {0}")]
    Discovery(#[from] sonos_discovery::DiscoveryError),

    #[error("Invalid IP address")]
    InvalidIpAddress,
}
```

### WatchCache Extensions (sonos-state internal)

Extend existing `WatchCache` instead of creating new `WatchedPropertyRegistry`:

```rust
impl WatchCache {
    // Existing methods...

    // New methods for iter() filtering:

    /// Get all currently watched (SpeakerId, property_key) pairs
    pub(crate) fn watched_properties(&self) -> Vec<(SpeakerId, &'static str)>;

    /// Check if a ChangeEvent matches any watched property
    pub(crate) fn matches_watched(&self, event: &ChangeEvent) -> bool;
}
```

---

## Final API Surface

### sonos-sdk

```rust
// Main entry point
pub struct SonosSystem;

impl SonosSystem {
    pub fn new() -> Result<Self, SdkError>;  // sync
    pub fn get_speakers(&self) -> Vec<Speaker>;
    pub fn get_speaker_by_id(&self, id: &SpeakerId) -> Option<Speaker>;
    pub fn iter(&self) -> impl Iterator<Item = ChangeEvent>;  // blocking
}

// Re-exports from sonos-state
pub use sonos_state::{
    Speaker, PropertyHandle, StateManagerBuilder,
    Volume, Mute, Bass, Treble, Loudness,
    PlaybackState, Position, CurrentTrack, GroupMembership,
    SpeakerId, ChangeEvent,
};
```

### sonos-state::PropertyHandle<P>

```rust
impl<P: Property> PropertyHandle<P> {
    pub fn get(&self) -> Option<P>;              // Cached read (instant)
    pub fn fetch(&self) -> Result<P>;            // API call + update state (sync)
    pub fn watch(&self) -> Result<Option<P>>;    // Returns current, registers in WatchCache (sync)
}
```

### sonos-state::Speaker

```rust
pub struct Speaker {
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
```

**Note**: Topology is intentionally excluded - it relates to groups, not individual speakers, and will be addressed in a future iteration.

---

## Implementation: watch() and iter() Relationship

### Key Insight
- UPnP subscriptions are at **service level** (e.g., RenderingControl includes volume, mute, bass, etc.)
- `watch()` subscribes to service BUT registers specific property in WatchCache for `iter()` filtering
- `iter()` only emits events for **watched properties** (not all service properties)

### Reference Counting Mechanism

Service subscriptions are managed through reference counting in `PropertySubscriptionManager`:

1. **Property watch creates WatchCache entry**: When `watch()` is called on a property (e.g., Volume), it gets added to WatchCache.

2. **Reference counting increments**: Watching a property increments `subscription_refs` in `PropertySubscriptionManager` for the corresponding service (e.g., RenderingControl).

3. **Shared subscriptions**: When another property using the same service is watched (e.g., Mute), the ref count increments to 2. Both properties share the same UPnP subscription.

4. **Cleanup on ref count zero**: Service unsubscription only happens when ref count drops to 0. Stopping Volume watch while Mute is still active keeps RenderingControl subscription alive (ref count = 1).

5. **Service failure handling**: If a service stops (timeout, network error), WatchCache clears values for all properties associated with that service.

### Example

```
speaker.volume.watch()
    |-- Subscribes to RenderingControl service (if not already, ref count 0->1)
    |-- Registers "volume" in WatchCache
    +-- Returns current value

speaker.mute.watch()
    |-- RenderingControl already subscribed (ref count 1->2)
    |-- Registers "mute" in WatchCache
    +-- Returns current value

// RenderingControl events include volume, mute, bass, treble, loudness
// But only "volume" and "mute" were watched, so:

iter() emits:
    [x] volume change events
    [x] mute change events
    [ ] bass change events (not watched)
    [ ] treble change events (not watched)
```

### Data Flow

```
speaker.volume.watch()
    |-- WatchCache ensures UPnP subscription (service-level, ref counted)
    |-- WatchCache.register(speaker_id, "volume")
    +-- Returns current value

system.iter() in render loop
    |-- Receives ALL events from subscribed services
    |-- Filters via WatchCache.matches_watched(event)
    +-- Emits ChangeEvent only for watched properties
```

---

## Implementation: iter() Async-to-Sync Bridge

The `iter()` method is synchronous (blocking) by design for ratatui render loops.

### Implementation Approach

```rust
pub struct ChangeIterator {
    rx: std::sync::mpsc::Receiver<ChangeEvent>,
}

impl Iterator for ChangeIterator {
    type Item = ChangeEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.recv().ok()  // Blocks until event or channel closed
    }
}
```

### Internal Architecture

```
[Async UPnP Event Loop]
    |-- Receives events from callback-server or polling
    |-- Filters through WatchCache.matches_watched()
    +-- Sends to std::sync::mpsc::Sender<ChangeEvent>

[Sync iter() in render thread]
    |-- Calls rx.recv() (blocking)
    +-- Returns ChangeEvent to ratatui loop
```

### Implementation Details

| Concern | Decision |
|---------|----------|
| Channel type | `std::sync::mpsc` (unbounded) |
| Blocking behavior | `recv()` blocks indefinitely until event or shutdown |
| Buffer size | Unbounded (events are small, throughput is low) |
| Shutdown | Dropping `SonosSystem` closes sender, `recv()` returns `None` |

### Non-blocking Variant (Future)

Consider adding `try_iter()` for non-blocking scenarios:

```rust
impl SonosSystem {
    pub fn try_iter(&self) -> impl Iterator<Item = ChangeEvent>;  // Non-blocking
}
```

---

## Implementation: fetch() on PropertyHandle

### Initial Scope

For the first iteration, only `Volume` needs `fetch()` support. Other properties can be added incrementally following the same pattern.

### Implementation

Add to PropertyHandle:
- `speaker_ip: IpAddr` - For API calls
- `api_client: SonosClient` - Shared API client

```rust
impl<P: Property> PropertyHandle<P> {
    pub fn fetch(&self) -> Result<P, StateError> {
        // 1. Match on P to determine operation
        // 2. Execute via api_client
        // 3. Update state_manager with new value
        // 4. Return value
    }
}
```

### Operation Mapping (expand incrementally)

| Property | Service | API Operation | Status |
|----------|---------|---------------|--------|
| Volume | RenderingControl | GetVolume | Initial scope |
| Mute | RenderingControl | GetMute | Future |
| Bass | RenderingControl | GetBass | Future |
| Treble | RenderingControl | GetTreble | Future |
| Loudness | RenderingControl | GetLoudness | Future |
| PlaybackState | AVTransport | GetTransportInfo | Future |
| Position | AVTransport | GetPositionInfo | Future |
| CurrentTrack | AVTransport | GetPositionInfo | Future (DIDL-Lite parsing) |
| GroupMembership | ZoneGroupTopology | GetZoneGroupState | Future (XML parsing) |

---

## Implementation Sequence

### Phase 1: sonos-state changes

1. Add `StateManagerBuilder` with `cleanup_timeout`
2. Extend `WatchCache` with `watched_properties()` and `matches_watched()` methods
3. Modify `PropertyHandle` - add `fetch()` (Volume only), ensure `watch()` registers in WatchCache
4. Modify `Speaker` - add IP + API client for fetch support
5. Add `iter()` method to StateManager (blocking, filtered by WatchCache)
6. Implement async-to-sync bridge with `std::sync::mpsc`

### Phase 2: sonos-sdk simplification

1. Delete `speaker.rs` and `property/` directory
2. Update `lib.rs` - re-export from sonos-state (Speaker, PropertyHandle, StateManagerBuilder, etc.)
3. Define `SdkError` hierarchy in `error.rs`
4. Update `system.rs` - add `get_speakers()`, `get_speaker_by_id()`, `iter()` (remove SonosSystemBuilder)

### Phase 3: Testing

1. Test `fetch()` updates state and returns value (Volume)
2. Test `watch()` registers property in WatchCache and returns current value
3. Test `iter()` only emits events for watched properties
4. Test reference counting - multiple properties sharing service subscription
5. Test cleanup timeout configuration via StateManagerBuilder

---

## Verification

1. **Build**: `cargo build -p sonos-sdk`
2. **Test watch/fetch/iter integration**:
   ```rust
   let system = SonosSystem::new()?;
   let speaker = system.get_speakers()[0].clone();

   // Watch volume - subscribes to RenderingControl, registers in WatchCache
   let vol = speaker.volume.watch()?;
   println!("Current volume: {:?}", vol);

   // Fetch hits API, updates state, returns fresh value
   let fresh_vol = speaker.volume.fetch()?;

   // iter() only emits volume events (mute/bass/etc filtered out)
   for event in system.iter() {
       // Will see: volume changes
       // Won't see: mute, bass, treble changes (not watched)
       println!("Watched property changed: {:?}", event);
   }
   ```
3. **Test filtering**: Watch volume only, verify mute changes don't appear in iter()
4. **Test ref counting**: Watch volume and mute, verify single RenderingControl subscription
5. **Run existing tests**: `cargo test -p sonos-state`

---

## Out of Scope (Future Iterations)

- **Topology/Groups**: Relates to speaker groups, not individual speakers
- **fetch() for all properties**: Start with Volume, add others incrementally
- **try_iter()**: Non-blocking variant for advanced use cases
