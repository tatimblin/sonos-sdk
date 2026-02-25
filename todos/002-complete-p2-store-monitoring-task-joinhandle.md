---
status: complete
priority: p2
issue_id: "002"
tags: [code-review, security, quality]
dependencies: []
---

# Store monitoring task JoinHandle for cancellation on shutdown

## Problem Statement

`EventDetector::start_monitoring()` spawns a `tokio::spawn` task that runs an infinite loop, but the `JoinHandle` is silently discarded. This means:
1. The monitoring task cannot be cancelled during `EventBroker::shutdown()` — it leaks.
2. Multiple calls to `start_monitoring()` would create duplicate monitoring loops.
3. The task holds `Arc` clones that prevent cleanup until the runtime shuts down.

## Findings

- **Security reviewer** (MEDIUM): "Resource leak on shutdown. The monitoring task continues running after EventBroker::shutdown() is called."
- **Performance reviewer**: The task will continue ticking every 10 seconds after broker shutdown, sending to a closed channel.

## Proposed Solutions

### Option A: Return JoinHandle and store in background_tasks (Recommended)
- Change `start_monitoring()` to return `JoinHandle<()>`
- Store it in `EventBroker::background_tasks` so it's aborted during `shutdown()`
- Add `AtomicBool` guard against multiple calls
- **Pros**: Follows existing pattern for other background tasks
- **Cons**: Requires changing `start_monitoring()` signature
- **Effort**: Small (20 min)
- **Risk**: Low

### Option B: Add CancellationToken
- Pass a `tokio_util::sync::CancellationToken` into the monitoring loop
- Token cancelled during broker shutdown
- **Pros**: Clean cancellation, no task abort
- **Cons**: Adds dependency on tokio_util, more complex
- **Effort**: Medium
- **Risk**: Low

## Technical Details

**Affected files:**
- `sonos-stream/src/subscription/event_detector.rs` — `start_monitoring()` method
- `sonos-stream/src/broker.rs` — `start_background_processing()` to store the handle

## Acceptance Criteria

- [ ] `start_monitoring()` returns `JoinHandle<()>`
- [ ] Handle stored in `EventBroker::background_tasks`
- [ ] Multiple calls to `start_monitoring()` are guarded against
- [ ] Monitoring task is cancelled during `EventBroker::shutdown()`

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Security reviewer flagged as MEDIUM |

## Resources

- PR: #36
- File: `sonos-stream/src/subscription/event_detector.rs:161-236`
