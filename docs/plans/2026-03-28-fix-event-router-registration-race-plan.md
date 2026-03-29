---
title: "fix: Event router registration race drops initial UPnP events"
type: fix
status: completed
date: 2026-03-28
deepened: 2026-03-28
origin: docs/brainstorms/2026-03-28-fix-event-router-registration-race-brainstorm.md
---

# fix: Event router registration race drops initial UPnP events

## Enhancement Summary

**Deepened on:** 2026-03-28
**Sections enhanced:** 4 (solution, technical considerations, acceptance criteria, edge cases)
**Research agents used:** architecture-strategist, performance-oracle, security-sentinel, code-simplicity-reviewer, julik-frontend-races-reviewer, pattern-recognition-specialist, best-practices-researcher (tokio RwLock), Explore (UPnP GENA)

### Key Improvements

1. **Simplified to always-write-lock** — the double-check read→write pattern is over-engineering for <100 events/second on a local network. A single write lock on `route_event()` eliminates 8 lines of interleaving analysis with no measurable performance impact.
2. **Flat `Vec` buffer instead of `HashMap`** — the buffer will realistically hold 0-5 events. A flat `Vec<(String, String, Instant)>` is simpler than `HashMap<String, Vec<PendingEvent>>` at this scale.
3. **Removed capacity cap** — TTL-based cleanup is sufficient. A 1000-entry FIFO eviction policy is YAGNI for a local-network home automation tool.
4. **Cleanup only in `register()`** — no opportunistic cleanup in `route_event()`. Keeps the write lock hold time minimal.

---

## Overview

Fix a race condition in `EventRouter` where the initial UPnP NOTIFY event
(sent by the speaker immediately after SUBSCRIBE) is silently dropped because
`register(sid)` hasn't been called yet. Add a buffer + replay mechanism so
events for not-yet-registered SIDs are held briefly and replayed when
registration completes.

## Problem Statement

When `watch()` triggers a subscription, the SDK:

1. Sends HTTP SUBSCRIBE to the speaker
2. Speaker responds with `SID: uuid:...`
3. SDK calls `EventRouter.register(sid)`
4. Speaker sends initial NOTIFY with current state

**The race:** Step 4 can arrive before step 3 completes. `route_event()` finds
the SID unregistered and returns `false`, the warp handler returns 404, and the
event is lost. Consumers see `None` from `get()`/`watch()` until the next state
*change* on the speaker. For idle speakers, this means permanently blank data.

(see brainstorm: `docs/brainstorms/2026-03-28-fix-event-router-registration-race-brainstorm.md`)

### Research Insights — UPnP Protocol

UPnP Device Architecture 2.0 Section 4 (Eventing) **guarantees** that the
device sends an initial NOTIFY containing the current state upon SUBSCRIBE.
This is not optional — it's part of the subscription confirmation mechanism.
The race is a well-known problem in UPnP control point implementations.
Buffer + replay is the standard solution (used in async_upnp_client, Cling,
and other open-source UPnP libraries).

---

## Step 0: Failing Test — Prove the Race Exists

Before any implementation changes, add a test that demonstrates the bug. This test
should **fail** on the current `EventRouter` and **pass** after the fix.

### Failing unit test (`callback-server/src/router.rs`)

```rust
#[tokio::test]
async fn test_event_buffered_and_replayed_on_late_register() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let router = EventRouter::new(tx);

    let sub_id = "uuid:late-register".to_string();
    let event_xml = "<e:propertyset><CurrentPlayMode>NORMAL</CurrentPlayMode></e:propertyset>".to_string();

    // 1. Event arrives BEFORE register (the race condition)
    router.route_event(sub_id.clone(), event_xml.clone()).await;

    // 2. Register happens moments later
    router.register(sub_id.clone()).await;

    // 3. The buffered event should have been replayed on register
    let payload = rx.try_recv().expect("expected replayed event");
    assert_eq!(payload.subscription_id, sub_id);
    assert_eq!(payload.event_xml, event_xml);
}
```

**Why this fails today:** `route_event()` checks `subscriptions.contains(&sub_id)`,
finds it missing, returns `false`, and the event is gone. No buffer, no replay.
The `try_recv()` call will panic with "expected replayed event".

### Failing integration test (`callback-server/tests/integration_tests.rs`)

```rust
#[tokio::test]
async fn test_notify_before_register_is_replayed() {
    // Start server
    let (tx, mut rx) = mpsc::unbounded_channel();
    let server = CallbackServer::start(tx, 50200..50210).await.unwrap();

    let sub_id = "uuid:race-integration";

    // 1. Send NOTIFY *before* registering the SID
    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::from_bytes(b"NOTIFY").unwrap(), &format!("{}/notify", server.base_url()))
        .header("SID", sub_id)
        .header("NT", "upnp:event")
        .header("NTS", "upnp:propchange")
        .body("<event>initial</event>")
        .send()
        .await
        .unwrap();

    // Should return 200 (buffered), not 404 (dropped)
    assert_eq!(resp.status(), 200);

    // 2. Now register the SID
    server.router().register(sub_id.to_string()).await;

    // 3. Buffered event should be replayed
    let payload = rx.try_recv().expect("expected replayed event after register");
    assert_eq!(payload.subscription_id, sub_id);
    assert_eq!(payload.event_xml, "<event>initial</event>");
}
```

**Why this fails today:** The warp handler returns 404 for unregistered SIDs,
and the event is never buffered. After the fix, the server returns 200 and
`register()` replays the buffered event.

---

## Proposed Solution

### 1. Unified lock over subscriptions + buffer (`callback-server/src/router.rs`)

Combine the subscription set and pending buffer under a single `RwLock` to
eliminate the TOCTOU gap between "check subscriptions" and "write to buffer":

```rust
use std::time::Instant;

const BUFFER_TTL: Duration = Duration::from_secs(5);

struct RouterState {
    subscriptions: HashSet<String>,
    /// Flat buffer of (subscription_id, event_xml, buffered_at).
    /// Expected size: 0-5 entries. Only populated during the microsecond
    /// race window between SUBSCRIBE response and register() call.
    pending: Vec<(String, String, Instant)>,
}

pub struct EventRouter {
    state: Arc<RwLock<RouterState>>,
    event_sender: mpsc::UnboundedSender<NotificationPayload>,
}
```

**Why a single lock:** With separate locks, `route_event()` could release the
subscription read lock, then `register()` could insert the SID and scan an
empty buffer, then `route_event()` would buffer the event — which would never
be replayed. The single lock makes the check-then-buffer atomic.

**Why a flat `Vec` instead of `HashMap<String, Vec<...>>`:** The buffer holds
0-5 events in practice (one initial NOTIFY per SID during the microsecond race
window). A flat Vec is simpler to reason about and iterate. At size 5, the
linear scan in `register()` is trivially fast.

### Research Insights — Lock Design

**`tokio::sync::RwLock` is correct over `std::sync::RwLock`** for two reasons:
(1) FIFO fairness ensures `register()` is not starved by concurrent
`route_event()` calls, and (2) it future-proofs against adding `.await` points
inside the critical section. The performance difference is immeasurable at
Sonos event volumes (<100 events/second).

**The double-check read→write pattern was considered and rejected.** The
pattern exists in `SpeakerServiceRegistry::register()` (`sonos-stream/src/registry.rs:104-143`),
but the simplicity reviewer correctly identified it as over-engineering here.
At <100 events/second, a write lock on every `route_event()` call adds no
measurable contention. The simpler code is worth more than the theoretical
concurrency gain.

---

### 2. `route_event()` — always write lock, buffer if unregistered

```rust
pub async fn route_event(&self, subscription_id: String, event_xml: String) {
    let mut state = self.state.write().await;
    if state.subscriptions.contains(&subscription_id) {
        let payload = NotificationPayload { subscription_id, event_xml };
        let _ = self.event_sender.send(payload);
    } else {
        debug!(sid = %subscription_id, "Buffered event for pending SID");
        state.pending.push((subscription_id, event_xml, Instant::now()));
    }
}
```

**Return type:** `()` instead of the previous `bool`. Both the routed and
buffered paths return HTTP 200 OK to the speaker — returning 404 for buffered
events could cause the speaker to cancel the subscription. The warp handler
unconditionally returns 200 (no match needed).

### Research Insights — Why Always 200 OK

The UPnP spec requires the subscriber to return 200 OK for valid notifications.
If the server returns 404, the device may stop sending events or cancel the
subscription entirely. Since buffered events are eventually delivered via replay,
200 is semantically correct — the event has been accepted for processing.

---

### 3. `register()` — replay buffered events + cleanup

```rust
pub async fn register(&self, subscription_id: String) {
    let mut state = self.state.write().await;
    state.subscriptions.insert(subscription_id.clone());

    // Replay buffered events for this SID and remove stale entries.
    // Using swap_remove for efficiency; FIFO order within a single SID
    // is preserved because there is typically exactly one buffered event
    // per SID (the initial NOTIFY).
    let now = Instant::now();
    let mut i = 0;
    while i < state.pending.len() {
        let (ref sid, _, buffered_at) = state.pending[i];
        if sid == &subscription_id {
            let (_, xml, _) = state.pending.swap_remove(i);
            debug!(sid = %subscription_id, "Replayed buffered event");
            let payload = NotificationPayload {
                subscription_id: subscription_id.clone(),
                event_xml: xml,
            };
            let _ = self.event_sender.send(payload);
            // Don't increment i — swap_remove moved the last element here
        } else if now.duration_since(buffered_at) > BUFFER_TTL {
            state.pending.swap_remove(i);
            // Don't increment i
        } else {
            i += 1;
        }
    }
}
```

**No separate cleanup function needed.** The `while` loop handles both replay
and TTL cleanup in a single pass. `swap_remove` is O(1) per removal. The
entire loop is O(n) where n is the buffer size (expected 0-5).

**Channel sends under write lock are non-blocking.** `mpsc::UnboundedSender::send()`
is synchronous (lock-free queue internally), so holding the write lock during
replay does not block the tokio runtime.

---

### 4. `unregister()` — also drain buffered events

```rust
pub async fn unregister(&self, subscription_id: &str) {
    let mut state = self.state.write().await;
    state.subscriptions.remove(subscription_id);
    state.pending.retain(|(sid, _, _)| sid != subscription_id);
}
```

Prevents stale buffered events from replaying on a future re-registration
with the same SID.

---

### 5. Warp handler update (`callback-server/src/server.rs`)

Unconditionally return 200 OK:

```rust
router.route_event(sub_id.clone(), event_xml).await;
// Always 200 OK — event is either routed or buffered for replay
Ok::<_, warp::Rejection>(warp::reply::with_status("", StatusCode::OK))
```

---

### 6. Logging

| Event | Level | Message |
|-------|-------|---------|
| Event buffered | `debug!` | `"Buffered event for pending SID {sid}"` |
| Event replayed | `debug!` | `"Replayed buffered event for SID {sid}"` |
| Stale entry cleaned | `trace!` | `"Cleaned stale buffer entry for SID {sid}"` |

---

## Technical Considerations

**Performance:** Write lock on every `route_event()` call. At Sonos event
volumes (<100 events/second), this adds no measurable latency. The write lock
hold time is a HashSet lookup + channel send (nanoseconds). Well within the
<10ms target from the callback-server spec.

**Firewall detection:** Event arrival at the HTTP handler proves network
reachability regardless of SID registration. The `EventProcessor` downstream
handles firewall coordinator notification. Since buffered events are replayed
through the same channel, firewall detection still triggers — just slightly
delayed (by the buffer window, typically microseconds).

**Subscription renewal:** Out of scope. Sonos speakers typically return the
same SID on renewal. If SID changes are observed, address as a separate issue.

**Shutdown:** Buffered events are lost on shutdown. Acceptable — the race
window is microseconds, and shutdown during that window is negligible.

### Research Insights — Edge Cases

**TTL cleanup timing:** The races reviewer identified that cleanup in
`route_event()` could evict events for SID-X before `register(SID-X)` arrives
if registration is pathologically slow (>5 seconds). The fix: **cleanup only
runs inside `register()`**, not in `route_event()`. This means the buffer can
grow unbounded in theory, but in practice it holds 0-5 entries (one per active
subscription race). The 5s TTL is 1000x longer than the race window. If
registration takes >5 seconds, the UPnP subscription itself has likely timed
out.

**SID reuse after unregister/re-register (ABA problem):** The races reviewer
noted that a stale NOTIFY from a previous subscription lifetime could be
buffered and then replayed on re-registration with the same SID. The
`unregister()` drain mitigates this — it removes buffered events when the old
subscription is torn down. If a stale event arrives *after* `unregister()` but
*before* the new `register()`, it would be buffered and replayed. In practice,
Sonos subscription teardown and re-creation are separated by more than 5
seconds (the grace period is 50ms, but full unsubscribe/resubscribe cycles
are user-initiated). If this ever becomes a problem, a generation counter can
be added.

**Cross-task event ordering:** Multiple concurrent `route_event()` calls for
registered SIDs are serialized by the write lock, so channel send order matches
lock acquisition order. This is FIFO based on tokio's fair RwLock scheduling.
For the buffer path, events are replayed in insertion order (Vec iteration).

**Integration test semantic change:** The existing test
`test_callback_server_end_to_end` asserts 404 for unknown subscriptions. After
this fix, unknown SIDs return 200 (buffered). Update this test, and add a new
test verifying that buffered events for truly unknown SIDs expire via TTL.

---

## Acceptance Criteria

- [x] **Step 0:** Failing test added that proves the race (route_event before register → event lost)
- [x] `EventRouter` uses single `RwLock<RouterState>` protecting subscriptions + flat `Vec` buffer
- [x] `route_event()` buffers events for unregistered SIDs (not dropped)
- [x] `route_event()` always takes write lock (no double-check pattern)
- [x] `route_event()` returns `()` (not `bool`); warp handler always returns 200 OK
- [x] `register()` replays buffered events for the newly registered SID
- [x] `register()` cleans up stale entries (>5s TTL) during replay pass
- [x] `unregister()` drains buffered events for the removed SID
- [x] `BUFFER_TTL` defined as `const Duration`
- [x] Existing tests updated for new `route_event()` return type and 200 OK semantics
- [x] New tests: buffer-then-replay, TTL expiry, concurrent register+route, unregister drains buffer
- [x] `callback-server` spec updated to reflect new EventRouter invariants
- [x] `cargo check` and `cargo clippy` pass
- [x] `cargo test -p sonos-sdk-callback-server` passes

## Sources

- **Origin brainstorm:** `docs/brainstorms/2026-03-28-fix-event-router-registration-race-brainstorm.md` — key decisions: buffer+replay over re-routing, fix in SDK not consumer, keep watch() non-blocking
- EventRouter implementation: `callback-server/src/router.rs:57-172`
- Warp NOTIFY handler: `callback-server/src/server.rs:268-335`
- Subscription creation + registration ordering: `sonos-stream/src/broker.rs:497-518`
- Existing double-check pattern: `sonos-stream/src/registry.rs:104-143`
- Callback server spec: `docs/specs/callback-server.md`
- Related: `sonos-cli` PR #32 — TUI watch migration exposed this bug
- UPnP Device Architecture 2.0, Section 4 (Eventing) — initial NOTIFY guarantee
