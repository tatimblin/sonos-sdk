---
status: complete
priority: p2
issue_id: "004"
tags: [code-review, performance, quality]
dependencies: []
---

# Replace SystemTime with Instant for elapsed time tracking in EventDetector

## Problem Statement

`EventDetector` uses `SystemTime::now()` for all elapsed-time calculations. `SystemTime` is subject to clock adjustments (NTP syncs, manual changes, suspend/resume). If the clock jumps backward, `duration_since()` returns `Err`, silently swallowed by `unwrap_or(Duration::ZERO)` — making the detector think events just arrived and preventing timeout detection indefinitely.

`Instant` is monotonic and designed for measuring elapsed time.

## Findings

- **Performance reviewer** (High priority, low effort): "Replace SystemTime with Instant for all elapsed-time tracking. This prevents silent failures from clock adjustments and is a one-line-per-callsite change."

## Proposed Solutions

### Option A: Replace SystemTime with Instant (Recommended)
- Change `last_event_times: HashMap<RegistrationId, SystemTime>` to `HashMap<RegistrationId, Instant>`
- Update all `SystemTime::now()` calls to `Instant::now()`
- `Instant::duration_since()` is infallible — remove `unwrap_or` calls
- **Pros**: Correct, simpler, slightly faster (no syscall overhead)
- **Cons**: None
- **Effort**: Small (15 min)
- **Risk**: None

## Technical Details

**Affected files:**
- `sonos-stream/src/subscription/event_detector.rs` — struct fields, `record_event()`, `should_start_polling()`, `should_stop_polling()`, `start_monitoring()`, `stats()`, tests

## Acceptance Criteria

- [ ] All `SystemTime` usage in event_detector.rs replaced with `Instant`
- [ ] `unwrap_or(Duration::ZERO)` calls removed (Instant::duration_since is infallible)
- [ ] Tests still pass

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Performance reviewer — high priority |

## Resources

- PR: #36
- File: `sonos-stream/src/subscription/event_detector.rs`
