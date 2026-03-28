# Watch Grace Period / Debounced Unsubscribe

**Date:** 2026-03-24
**Status:** Brainstorm
**Crates affected:** sonos-event-manager, sonos-state, sonos-sdk

## What We're Building

A grace period mechanism for `watch()` that prevents UPnP subscription churn between TUI frame redraws. When a watch is released (via RAII guard drop), the underlying subscription stays alive for 50ms. If the same property is re-watched within that window, the subscription is reused seamlessly — no teardown, no re-subscribe.

This enables TUI applications to call `watch()` directly inside widget rendering code, declaring data dependencies inline where they're used, instead of managing watch lifecycle separately from the draw loop.

### The Problem Today

Current `watch()` + `unwatch()` is designed for long-lived setup/teardown:

```rust
// Setup phase (once)
speaker.volume.watch()?;
speaker.playback_state.watch()?;

// Render loop (uses get() only)
loop {
    let vol = speaker.volume.get();
    draw_widget(vol);
}

// Teardown (once)
speaker.volume.unwatch();
speaker.playback_state.unwatch();
```

In a TUI framework like ratatui, widgets are reconstructed each frame. There's no natural place for one-time setup — watches must be tracked separately from the rendering code. The desired pattern is:

```rust
// Inside widget draw() — called every frame
fn draw(&self, speaker: &Speaker) {
    let vol = speaker.volume.watch()?;  // value + RAII guard in one
    draw_volume_bar(&vol);
    // vol dropped at end of scope — grace period starts
}
// Next frame: watch() called again, grace period cancelled
```

### Additional Problem: Ref Count Inflation

`watch()` is NOT idempotent for subscription ref counts. Each call to `ensure_service_subscribed()` increments the counter. Calling `watch()` 60 times/sec without matching `unwatch()` calls would inflate the ref count unboundedly. The WatchGuard approach solves this by pairing each `watch()` with exactly one drop.

## Why This Approach

### Unified grace period in sonos-event-manager

The grace period lives in a single place — `SonosEventManager` — and coordinates both:
1. **UPnP service subscriptions** (the expensive network operation)
2. **Watched-set registration** (controls whether ChangeEvents are forwarded)

This avoids having two independent debounce timers that could drift or behave inconsistently.

### RAII WatchGuard

`watch()` returns a `WatchGuard` instead of the current `WatchStatus`. The guard's `Drop` impl triggers the grace period. This is idiomatic Rust and fits naturally into TUI frame lifecycles where scopes map to widget lifetimes.

### Breaking change: Remove `unwatch()`

The explicit `unwatch()` method is deleted. WatchGuard is the only mechanism. This simplifies the API surface and eliminates the footgun of unbalanced watch/unwatch calls.

## Key Decisions

1. **Grace period duration: 50ms** — covers 1-3 frame gaps at 60fps, tight enough for quick cleanup of genuinely abandoned watches.

2. **RAII WatchGuard over explicit unwatch()** — guard drop starts the grace period; re-watching cancels it. No manual lifecycle management.

3. **Delete unwatch()** — breaking change is acceptable. WatchGuard is the sole mechanism.

4. **Single unified timer in sonos-event-manager** — one grace period coordinates both the UPnP subscription and the watched-set registration. Uses a `WatchRegistry` trait callback to reach into the state-manager without circular dependencies.

5. **Ref counting preserved for simultaneous watches** — two widgets watching `volume` on the same speaker produces ref count 2. Both must drop their guards before the grace period starts.

6. **Event-manager unified API** — new `acquire_watch()` / `release_watch()` methods replace the current separate `register_watch` + `ensure_service_subscribed` calls. `PropertyHandle::watch()` makes one call instead of two.

## Design Sketch

### New types

```rust
// sonos-event-manager
pub struct WatchGuard {
    event_manager: Arc<SonosEventManager>,
    speaker_id: SpeakerId,
    property_key: &'static str,
    ip: IpAddr,
    service: Service,
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        self.event_manager.release_watch(
            &self.speaker_id, self.property_key,
            self.ip, self.service,
        );
    }
}

// Trait to avoid circular dependency (event-manager -> state-manager)
pub trait WatchRegistry: Send + Sync {
    fn register_watch(&self, speaker_id: &SpeakerId, key: &'static str);
    fn unregister_watch(&self, speaker_id: &SpeakerId, key: &'static str);
}
```

### Grace period flow

```
watch() called
  → event_manager.acquire_watch(speaker_id, key, ip, service)
    → If grace timer active for (ip, service): cancel timer, increment ref count
    → Else: register_watch + ensure_service_subscribed (existing behavior)
    → Returns WatchGuard

WatchGuard dropped
  → event_manager.release_watch(speaker_id, key, ip, service)
    → Decrement ref count
    → If ref count hits 0: start 50ms grace timer for (ip, service)
    → Record (speaker_id, key) as pending-unregister

Grace timer fires (50ms elapsed, no re-acquisition)
  → Send Command::Unsubscribe to worker
  → Call watch_registry.unregister_watch() for all pending keys

Grace timer cancelled (watch() called before 50ms)
  → Clear ALL pending-unregister entries for this (ip, service)
  → Resume normal ref counting
  → Note: if only some properties are re-watched, abandoned ones
    stay in the watched set until the service subscription eventually
    drops. This is a minor inefficiency (unused ChangeEvents emitted),
    not a correctness issue.
```

### watch() return type change

```rust
// Before
pub fn watch(&self) -> Result<WatchStatus<P>, SdkError>

// After
pub fn watch(&self) -> Result<WatchHandle<P>, SdkError>

pub struct WatchHandle<P> {
    value: Option<P>,           // current cached value
    mode: WatchMode,            // Events, Polling, or CacheOnly
    _guard: WatchGuard,         // RAII subscription hold
}

// Deref to Option<P> — use the handle as if it were the value
impl<P> Deref for WatchHandle<P> {
    type Target = Option<P>;
    fn deref(&self) -> &Self::Target { &self.value }
}
```

This enables the one-liner pattern:
```rust
let vol = speaker.volume.watch()?;  // WatchHandle<Volume>
draw_volume_bar(&vol);              // Deref to &Option<Volume>
// vol dropped → grace period starts
```

## Implementation Constraints

- **Drop can't be async**: `WatchGuard::drop()` must be synchronous. `release_watch()` will send a message to the event-manager's existing async worker (which already processes `Command::Subscribe`/`Unsubscribe`).
- **WatchGuard is not Clone**: Each guard represents exactly one ref count hold. Cloning would require incrementing the ref count, adding complexity for no clear use case.
- **`is_watched()` stays**: Only `unwatch()` is removed. `is_watched()` remains as a read-only check on the watched set.

## Open Questions

None — all questions resolved during brainstorming.
