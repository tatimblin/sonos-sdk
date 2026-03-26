---
title: "feat: Add watch() grace period with RAII WatchGuard"
type: feat
status: completed
date: 2026-03-25
deepened: 2026-03-25
origin: docs/brainstorms/2026-03-24-watch-grace-period-brainstorm.md
---

# feat: Add watch() grace period with RAII WatchGuard

## Enhancement Summary

**Deepened on:** 2026-03-25
**Research agents used:** architecture-strategist, performance-oracle, pattern-recognition-specialist, code-simplicity-reviewer, security-sentinel, best-practices-researcher, Context7 (tokio docs)

### Key Improvements
1. **Preparatory refactors**: Move `SpeakerId` to `sonos-api`, upgrade command channel to `tokio::sync::mpsc`, switch to `parking_lot::RwLock`, remove redundant `StateManager::subscriptions` field — sets up the cleanest possible foundation.
2. **Simplified grace period: delayed unsubscribe pattern** — Instead of modifying the worker event loop with new Command variants and timer management, use a `std::thread::spawn` + `sleep(50ms)` + `AtomicBool` cancellation pattern directly in `SonosEventManager`. Reduces ~150 lines of worker changes to ~20 lines.
3. **Safety hardening** — `release_watch()` returns `()` (panic-free by design), `parking_lot::RwLock` prevents lock poisoning, shutdown drains pending grace timers.

### New Considerations Discovered
- `WatchGuard` is `Send` but NOT `Sync` (due to `mpsc::Sender` in `SonosEventManager`) — acceptable for TUI use case, should be documented
- `GroupPropertyHandle` uses coordinator IPs that could change during a grace period — document as known limitation

---

## Overview

Add a 50ms grace period to the `watch()` subscription lifecycle, replacing the current `watch()`/`unwatch()` pair with an RAII `WatchHandle<P>` that holds a subscription guard. When the guard is dropped, the underlying UPnP subscription stays alive for 50ms. If `watch()` is called again within that window, the subscription is reused seamlessly. This enables TUI applications to call `watch()` directly inside widget `draw()` methods:

```rust
fn draw(&self, speaker: &Speaker) {
    let vol = speaker.volume.watch()?;  // WatchHandle<Volume>, Derefs to Option<Volume>
    draw_volume_bar(&vol);
    // vol dropped at end of scope — grace period starts
}
// Next frame: watch() called again, grace period cancelled, subscription persists
```

This is a **breaking change**: `unwatch()` is deleted, `watch()` returns `WatchHandle<P>` instead of `WatchStatus<P>`.

## Problem Statement / Motivation

Current `watch()` + `unwatch()` requires separate lifecycle management from rendering. TUI frameworks like ratatui reconstruct widgets every frame — there's no natural place for one-time setup/teardown. Users must track watches separately and use `get()` in draw methods. Additionally, `watch()` is not idempotent for subscription ref counts, making it dangerous to call in loops.

(see brainstorm: docs/brainstorms/2026-03-24-watch-grace-period-brainstorm.md — "The Problem Today" section)

## Proposed Solution

A unified grace period in `SonosEventManager` that coordinates both UPnP subscription lifecycle and watched-set registration through a single delayed-unsubscribe mechanism. New `acquire_watch()`/`release_watch()` methods replace the current separate `register_watch` + `ensure_service_subscribed` calls. A `WatchRegistry` trait bridges the event-manager and state-manager without circular dependencies.

(see brainstorm: docs/brainstorms/2026-03-24-watch-grace-period-brainstorm.md — "Why This Approach" section)

## Technical Approach

### Architecture

**Grace timer granularity: `(IpAddr, Service)`** — matching the existing ref count key in `SonosEventManager::service_refs`. Property-level `acquire_watch()`/`release_watch()` calls increment/decrement this service-level counter. The grace timer starts only when the service ref count hits zero.

**Watched-set cleanup: deferred until grace period expiry** — the `watched` HashSet in `StateManager` is NOT cleaned up on guard drop. It is only cleaned up via the `WatchRegistry` callback when the grace timer fires. This ensures events flow between frames without gaps.

### Research Insights: Grace Period Implementation

**Simplified approach (recommended by simplicity review):** Instead of modifying the worker event loop with new Command variants and complex timer management, implement the grace period as a **delayed unsubscribe** directly in `SonosEventManager`:

```rust
// In SonosEventManager, add one field:
pending_unsubscribes: Mutex<HashMap<(IpAddr, Service), Arc<AtomicBool>>>

// In release_watch(), when ref count hits 0:
let cancelled = Arc::new(AtomicBool::new(false));
self.pending_unsubscribes.lock().insert((ip, service), Arc::clone(&cancelled));
let tx = self.command_tx.clone();
std::thread::spawn(move || {
    std::thread::sleep(Duration::from_millis(50));
    if !cancelled.load(Ordering::SeqCst) {
        let _ = tx.send(Command::Unsubscribe { ip, service });
    }
});

// In acquire_watch(), when ref count goes from 0 to 1:
if let Some(flag) = self.pending_unsubscribes.lock().remove(&(ip, service)) {
    flag.store(true, Ordering::SeqCst);
    // No subscribe needed — the subscription is still alive
}
```

**Why simpler is better:**
- Zero worker changes — the existing `Command::Subscribe`/`Unsubscribe`/`Shutdown` variants are sufficient
- No `StartGracePeriod`/`CancelGracePeriod` commands needed
- No `Pin<Box<Sleep>>` HashMap or `select_next_grace_timer` helper
- The worker file (`sonos-event-manager/src/worker.rs`) stays completely unchanged
- ~20 lines of logic vs ~150 lines in the worker-based approach

**Tradeoff:** This spawns one short-lived OS thread per grace period (per service-per-speaker transition through zero). At realistic scale (10 speakers, 3 services, 60fps), that is at most 30 threads spawned per second, each sleeping for 50ms. This is negligible on modern systems.

### Research Insights: Worker Channel Upgrade (Optional, Recommended)

**Replace `std::sync::mpsc` with `tokio::sync::mpsc::unbounded_channel`:**

The current worker polls `command_rx.try_recv()` every 10ms via `tokio::time::sleep(Duration::from_millis(10))`. This is a workaround for using a synchronous channel in an async context. Switching to `tokio::sync::mpsc::UnboundedSender` (whose `.send()` method is synchronous) eliminates the polling entirely:

```rust
// tokio::sync::mpsc::UnboundedSender::send() is sync — works from SonosEventManager methods
// tokio::sync::mpsc::UnboundedReceiver::recv() is async — works in tokio::select!

tokio::select! {
    biased;  // Guarantees deterministic branch priority (commands first)

    Some(cmd) = command_rx.recv() => {
        // Immediate wakeup, no 10ms delay
        handle_command(cmd).await;
    }
    Some(event) = events.next_async() => {
        let _ = event_tx.send(event);
    }
}
```

**Benefits:** Eliminates 10ms command latency, reduces idle CPU wake-ups from 100/sec to zero, and enables `biased;` for deterministic branch priority. This is a standalone improvement that can be done before or alongside the grace period feature.

**Source:** tokio docs confirm `UnboundedSender::send()` is sync and safe to call from non-async code.

### Data Flow

```
PropertyHandle::watch()
  → SonosEventManager::acquire_watch(speaker_id, key, ip, service)
    → If pending unsubscribe for (ip, service): cancel it via AtomicBool, skip subscribe
    → Else: increment service ref count (0→1 triggers Command::Subscribe)
    → Add (speaker_id, key) to watched set via WatchRegistry
    → Return WatchGuard

WatchGuard::Drop
  → SonosEventManager::release_watch(speaker_id, key, ip, service)
    → Decrement service ref count
    → If count hits 0:
      → Spawn thread with 50ms sleep + conditional Command::Unsubscribe
      → Record (speaker_id, key) as pending-unregister for WatchRegistry cleanup
    → Else: no-op (other guards still alive)

Delayed thread fires (50ms elapsed, not cancelled)
  → Send Command::Unsubscribe to worker (existing handler)
  → Call watch_registry.unregister_watches_for_service(ip, service) to clean watched set

Delayed thread cancelled (watch() called before 50ms, AtomicBool set to true)
  → Thread wakes, sees cancelled flag, exits without action
```

### New Types

#### sonos-event-manager

```rust
/// Trait for managing the watched-property set.
/// Defined in sonos-event-manager, implemented by StateManager in sonos-state.
/// Uses SpeakerId directly (moved to sonos-api in Phase 0).
pub trait WatchRegistry: Send + Sync + 'static {
    fn register_watch(&self, speaker_id: &SpeakerId, key: &'static str);
    fn unregister_watches_for_service(&self, ip: IpAddr, service: Service);
}
```

#### Research Insight: SpeakerId Moved to sonos-api

`SpeakerId` is currently defined in `sonos-state` (`sonos-state/src/model/id_types.rs:34`), but `sonos-event-manager` does not depend on `sonos-state`. Phase 0 moves `SpeakerId` to `sonos-api` (which both crates depend on), enabling the trait to use the proper type directly. `unregister_watches_for_service(ip, service)` avoids passing property keys through the command channel entirely — the `StateManager` knows which keys belong to which service and removes them all.

```rust
/// RAII guard — each instance holds one ref count.
/// Not Clone, not Copy. Each guard is exactly one subscription hold.
#[must_use = "dropping the guard immediately starts the grace period"]
pub struct WatchGuard {
    event_manager: Arc<SonosEventManager>,
    speaker_id: SpeakerId,
    property_key: &'static str,
    ip: IpAddr,
    service: Service,
}

// Compile-time assertion: WatchGuard must be Send
const _: () = {
    fn assert_send<T: Send>() {}
    fn _check() { assert_send::<WatchGuard>(); }
};

impl fmt::Debug for WatchGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatchGuard")
            .field("speaker_id", &self.speaker_id)
            .field("property_key", &self.property_key)
            .field("ip", &self.ip)
            .field("service", &self.service)
            .finish()
    }
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        // release_watch returns () — panic-free by design
        self.event_manager.release_watch(
            &self.speaker_id, self.property_key,
            self.ip, self.service,
        );
    }
}
```

#### Research Insight: release_watch Must Return ()

The security review identified that `release_watch()` must never panic since it is called from `Drop`. Making it return `()` instead of `Result` enforces this at the type level — the implementation must handle all errors internally (log + continue):

```rust
/// Called from WatchGuard::Drop. Must never panic.
/// All errors are logged and silently absorbed.
pub fn release_watch(
    &self,
    speaker_id: &SpeakerId,
    property_key: &'static str,
    ip: IpAddr,
    service: Service,
) {
    let should_start_grace = match self.service_refs.write() {
        Ok(mut refs) => { /* decrement logic */ }
        Err(_poisoned) => {
            tracing::warn!("service_refs lock poisoned in release_watch");
            return;
        }
    };
    if should_start_grace {
        // Spawn delayed unsubscribe thread
        // ...
    }
}
```

#### sonos-sdk

```rust
/// Replaces WatchStatus<P>. Holds a snapshot of the current value
/// along with a subscription guard. Dropping the handle starts the
/// grace period — the subscription persists for 50ms.
///
/// Not Clone — each handle is one subscription hold.
#[must_use = "dropping the handle starts the grace period"]
pub struct WatchHandle<P> {
    value: Option<P>,
    mode: WatchMode,
    _guard: WatchGuard,
}

impl<P> Deref for WatchHandle<P> {
    type Target = Option<P>;
    fn deref(&self) -> &Self::Target { &self.value }
}

impl<P> WatchHandle<P> {
    /// Returns the watch mode (Events, Polling, or CacheOnly).
    pub fn mode(&self) -> WatchMode { self.mode }

    /// Convenience: returns a reference to the inner value, if available.
    /// Equivalent to `(*handle).as_ref()` but more ergonomic.
    pub fn value(&self) -> Option<&P> { self.value.as_ref() }

    /// Returns true if a value has been received from the device.
    pub fn has_value(&self) -> bool { self.value.is_some() }

    /// Returns true if real-time UPnP events are active.
    pub fn has_realtime_events(&self) -> bool { self.mode == WatchMode::Events }
}

impl<P: fmt::Debug> fmt::Debug for WatchHandle<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WatchHandle")
            .field("value", &self.value)
            .field("mode", &self.mode)
            .finish()
    }
}
```

#### Research Insight: WatchHandle Ergonomics

The pattern review noted that `Deref<Target = Option<P>>` is acceptable but unconventional. The `value()` method provides `Option<&P>` which is more idiomatic than `&Option<P>` in most Rust APIs. `has_realtime_events()` is preserved from the deleted `WatchStatus` to minimize gratuitous API breakage.

### Implementation Phases

#### Phase 0: Foundational Refactors

**Crates:** sonos-api, sonos-event-manager, sonos-state

Four preparatory changes that set up the cleanest possible foundation for the grace period:

**0a. Move `SpeakerId` to `sonos-api`**

Move `SpeakerId` (and any related ID types) from `sonos-state/src/model/id_types.rs` to `sonos-api`. Both `sonos-event-manager` and `sonos-state` already depend on `sonos-api`, so this eliminates the circular dependency workaround. `sonos-state` re-exports the type for backward compatibility.

Files changed:
- `sonos-api/src/lib.rs` — add SpeakerId type (or new `sonos-api/src/types/` module)
- `sonos-state/src/model/id_types.rs` — remove SpeakerId, re-export from sonos-api
- All crates importing SpeakerId — update import paths

**0b. Replace `std::sync::mpsc` with `tokio::sync::mpsc::unbounded_channel`**

The current worker polls the command channel every 10ms. Switching to tokio's async channel eliminates polling, enables `biased;` select for deterministic command priority, and reduces idle CPU wake-ups from 100/sec to zero.

Files changed:
- `sonos-event-manager/src/manager.rs` — `command_tx` type: `mpsc::Sender<Command>` → `tokio::sync::mpsc::UnboundedSender<Command>`
- `sonos-event-manager/src/worker.rs` — remove 10ms sleep, use `command_rx.recv()` directly in `tokio::select! { biased; ... }`

**0c. Replace `std::sync::RwLock` with `parking_lot::RwLock`**

Prevents lock poisoning, which would permanently disable the watch system if any thread panics while holding the lock. Critical for Drop safety since `release_watch()` runs during panic unwinding.

Files changed:
- `sonos-event-manager/src/manager.rs` — `service_refs` RwLock type change
- `sonos-event-manager/Cargo.toml` — add `parking_lot` dependency (already transitive via `dashmap`)
- `sonos-state/src/state.rs` — `watched` and `store` RwLock type changes (optional, same reasoning)

**0d. Remove redundant `StateManager::subscriptions` field**

`StateManager::subscriptions: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>` mirrors `SonosEventManager::service_refs`. Remove it and let the event-manager be the single authority for subscription ref counting.

Files changed:
- `sonos-state/src/state.rs` — remove `subscriptions` field and all references to it

**Tests:** Existing test suite passes after each sub-phase. No new tests needed — these are mechanical refactors.

**Why first:** These are standalone improvements that simplify the grace period implementation and are easier to review in isolation. Each sub-phase can be a separate commit.

#### Phase 1: WatchRegistry Trait + Grace Period in SonosEventManager

**Crate:** sonos-event-manager

**Deliverables:**
- Define `WatchRegistry` trait with `SpeakerId` (moved to sonos-api in Phase 0)
- Add `acquire_watch()` and `release_watch()` methods to `SonosEventManager`
- Add `WatchGuard` struct with `Drop` impl, `Debug`, `#[must_use]`, compile-time `Send` assertion
- Add `pending_unsubscribes: Mutex<HashMap<(IpAddr, Service), Arc<AtomicBool>>>` field
- Add `set_watch_registry()` method (OnceLock pattern, matching existing `event_manager` field on StateManager)
- `release_watch()` returns `()` — all errors handled internally
- Shutdown handler drains pending grace timers: cancel all AtomicBool flags, send all pending Unsubscribe commands, call `unregister_watches_for_service` for each

**Files changed:**
- `sonos-event-manager/src/manager.rs` — new methods, WatchGuard struct, pending_unsubscribes field
- `sonos-event-manager/src/lib.rs` — export WatchRegistry trait, WatchGuard

**Note:** Worker file (`worker.rs`) is NOT modified — grace period is handled entirely in the manager.

**Tests:**
- Grace timer fires after 50ms when no re-acquisition (use controlled thread timing)
- Grace timer cancelled when `acquire_watch()` arrives within 50ms
- Ref count increments/decrements correctly with multiple guards
- Guard drop with disconnected worker does not panic
- Guard drop with poisoned lock does not panic
- Multiple concurrent grace timers for different (ip, service) pairs
- Shutdown drains all pending grace timers

**Success criteria:** Grace period mechanics work at the event-manager level with a mock WatchRegistry.

#### Phase 2: StateManager WatchRegistry Implementation

**Crate:** sonos-state

**Deliverables:**
- Implement `WatchRegistry` for `StateManager`
- Wire `StateManager` as the `WatchRegistry` when event manager is initialized
- `unregister_watches_for_service` removes all `(speaker_id, key)` entries for properties belonging to the given service
- Remove direct `register_watch()`/`unregister_watch()` from public API (now internal via trait)
- Keep `is_watched()` as a public read-only check
- `StateManager::subscriptions` field already removed in Phase 0d

**Files changed:**
- `sonos-state/src/state.rs` — impl WatchRegistry for StateManager, wire into event manager init

**Tests:**
- `WatchRegistry::register_watch` adds to watched set
- `WatchRegistry::unregister_watches_for_service` removes correct entries
- Events continue to flow during grace period (watched set stays populated)
- Events stop after grace period expires (watched set cleaned up)

**Success criteria:** StateManager correctly participates in the grace period lifecycle via the trait.

#### Phase 3: SDK API Changes (WatchHandle + Remove unwatch)

**Crate:** sonos-sdk

**Deliverables:**
- Change `PropertyHandle::watch()` to return `Result<WatchHandle<P>, SdkError>`
- Implement `WatchHandle<P>` with `Deref<Target = Option<P>>`, `mode()`, `value()`, `has_value()`, `has_realtime_events()`, `Debug`
- Add `#[must_use]` to `WatchHandle`
- Delete `PropertyHandle::unwatch()` method
- Apply same changes to `GroupPropertyHandle::watch()` / `GroupPropertyHandle::unwatch()`
- Remove `WatchStatus<P>` struct (replaced by `WatchHandle<P>`)
- `PropertyHandle::watch()` now makes a single call to `event_manager.acquire_watch()` instead of separate `register_watch` + `ensure_service_subscribed`
- Update `is_watched()` semantics: returns true if a guard is alive OR grace period is active (automatic — watched set stays populated during grace period)
- Make `ensure_service_subscribed()` and `release_service_subscription()` internal-only (`pub(crate)`)

**Files changed:**
- `sonos-sdk/src/property/handles.rs` — WatchHandle struct, watch()/unwatch() changes, GroupPropertyHandle parity
- `sonos-sdk/src/lib.rs` — update exports (WatchHandle replaces WatchStatus)

**Tests:**
- `watch()` returns WatchHandle that derefs to Option<P>
- `WatchHandle::mode()`, `value()`, `has_value()`, `has_realtime_events()` work correctly
- Dropping WatchHandle triggers grace period (integration test with event-manager)
- Multiple guards from different watch() calls accumulate ref count
- GroupPropertyHandle::watch() returns WatchHandle with grace period

**Success criteria:** SDK compiles with new API, examples updated.

#### Phase 4: Example Updates + Documentation

**Deliverables:**
- Update `sonos-sdk/examples/sdk_demo.rs` — remove explicit unwatch(), use WatchHandle
- Update `sonos-sdk/examples/basic_usage_sdk.rs` — demonstrate WatchHandle Deref pattern
- Update `sonos-sdk/examples/smart_dashboard.rs` — adapt to new API
- Update crate-level documentation and CLAUDE.md code examples
- Update `docs/specs/sonos-event-manager.md` with grace period documentation
- Update `docs/specs/sonos-state.md` with WatchRegistry implementation
- Update `docs/specs/sonos-sdk.md` with new watch() API
- Document that `WatchHandle` captures a snapshot value (not a live reference)
- Document that `WatchGuard`/`WatchHandle` is `Send` but not `Sync`

**Success criteria:** All examples compile and run, documentation reflects new API.

## System-Wide Impact

- **Interaction graph**: `PropertyHandle::watch()` → `SonosEventManager::acquire_watch()` → (if first) `Command::Subscribe` → worker → `broker.register_speaker_service()`. `WatchGuard::Drop` → `release_watch()` → (if last) spawns delayed thread → (if expired) `Command::Unsubscribe` → worker → `broker.unregister_speaker_service()` + `WatchRegistry::unregister_watches_for_service()`.
- **Error propagation**: `acquire_watch()` can fail (lock poisoned, worker disconnected). `release_watch()` returns `()` — all errors logged and absorbed (called from Drop). Grace timer expiry errors are logged but don't propagate.
- **State lifecycle risks**: During grace period, the watched set has entries for properties that aren't actively guarded. This is intentional — events continue to flow. The only risk is if a grace period never fires (thread leak), leaving stale watched entries. Shutdown must cancel all pending timers.
- **API surface parity**: Both `PropertyHandle` and `GroupPropertyHandle` get the same treatment. The `system.iter()` pattern continues to work alongside guard-based watching.

### Research Insight: GroupPropertyHandle Coordinator Change

The architecture review identified that `GroupPropertyHandle` uses the coordinator's speaker IP for UPnP subscriptions. If the group coordinator changes during a grace period (topology event), the stale coordinator IP would be used for the delayed unsubscribe. This is an edge case — the subscription would fail to unsubscribe on the old coordinator (harmless, UPnP subscriptions timeout after 1800s) and a new subscription would be created on the new coordinator. Document as a known limitation.

## Acceptance Criteria

- [x] `watch()` returns `WatchHandle<P>` implementing `Deref<Target = Option<P>>`
- [x] `WatchHandle` holds an RAII `WatchGuard` that triggers grace period on drop
- [x] Grace period of 50ms prevents UPnP unsubscribe/resubscribe churn between frames
- [x] Re-calling `watch()` within 50ms cancels the grace timer seamlessly
- [x] Multiple simultaneous guards for the same property accumulate ref counts correctly
- [x] `unwatch()` method is removed from both `PropertyHandle` and `GroupPropertyHandle`
- [x] Events continue to flow during the grace period (watched set stays populated)
- [x] `WatchGuard::Drop` does not panic even if worker is disconnected or lock is poisoned
- [x] Grace timers are cleaned up on shutdown
- [x] All existing examples compile and run with the new API
- [x] Spec files updated for sonos-event-manager, sonos-state, sonos-sdk
- [x] `WatchGuard` has `Debug`, `#[must_use]`, compile-time `Send` assertion, non-Clone
- [x] `release_watch()` returns `()` (panic-free by design)

## Dependencies & Risks

| Risk | Mitigation |
|------|-----------|
| Breaking change to public API | Acceptable per brainstorm decision. Bump minor version. |
| SpeakerId circular dependency | Resolved in Phase 0a: SpeakerId moved to sonos-api |
| WatchGuard::Drop panic safety | `release_watch()` returns `()`, handles all errors internally |
| Lock poisoning permanently disables watch system | Resolved in Phase 0c: `parking_lot::RwLock` (no poisoning) |
| Grace timer race with new watch() | AtomicBool cancellation is checked after sleep — no race possible |
| Thread spawn overhead at 60fps | At most 30 threads/sec (10 speakers * 3 services), each sleeping 50ms. Negligible. |
| todo/004 (event-init not propagated to rediscovered speakers) | Pre-existing bug, not worsened meaningfully. Address separately. |
| Partial service re-acquisition leaves stale watched entries | Acceptable minor inefficiency; documented in brainstorm |
| WatchGuard is Send but not Sync | Acceptable for TUI single-thread rendering. Documented. |
| std::mem::forget(guard) leaks ref count | Known Rust limitation (same as MutexGuard). Documented with #[must_use]. |

## Sources & References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-03-24-watch-grace-period-brainstorm.md](docs/brainstorms/2026-03-24-watch-grace-period-brainstorm.md) — Key decisions carried forward: 50ms grace period, RAII WatchGuard, unified timer in event-manager, delete unwatch().

### Internal References

- Event manager ref counting: `sonos-event-manager/src/manager.rs:47-62` (service_refs field)
- Current watch() implementation: `sonos-sdk/src/property/handles.rs:294-327`
- Current unwatch() implementation: `sonos-sdk/src/property/handles.rs:341-357`
- Worker event loop: `sonos-event-manager/src/worker.rs:59-155`
- Watched set: `sonos-state/src/state.rs:242-269` (StateManager fields)
- GroupPropertyHandle: `sonos-sdk/src/property/handles.rs:792-836`
- SpeakerId (to be moved): `sonos-state/src/model/id_types.rs:34` → `sonos-api`
- StateManager::subscriptions (to be removed): `sonos-state/src/state.rs:249-250`

### External References

- tokio `biased;` select macro: ensures deterministic branch priority in `tokio::select!`
- tokio `UnboundedSender::send()`: sync-safe send for async channels — ideal for sync-to-async messaging
- `parking_lot::RwLock`: non-poisoning RwLock — prevents permanent system disable on panic
- `tokio_util::time::DelayQueue`: purpose-built dynamic timer set (alternative if worker-based approach preferred)
- RAII guard patterns: `std::sync::MutexGuard`, `tokio::sync::OwnedMutexGuard` — conventions for Drop-based resource management
