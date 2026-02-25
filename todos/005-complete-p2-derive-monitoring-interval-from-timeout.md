---
status: complete
priority: p2
issue_id: "005"
tags: [code-review, performance, architecture]
dependencies: []
---

# Derive monitoring interval from event_timeout instead of hardcoding 10s

## Problem Statement

The monitoring loop in `start_monitoring()` hardcodes a 10-second interval. With `event_timeout = 5s` (valid config), the monitoring interval is longer than the timeout itself — timeouts may never be detected within a single cycle. With `fast_polling()` preset (`event_timeout = 15s`), detection latency is up to 25s from last event, slower than expected for a "fast" mode.

## Findings

- **Performance reviewer**: "Derive the monitoring interval from event_timeout to ensure proportional detection latency."
- **Architecture reviewer**: "The test must wait up to 15 seconds because it has no control over the monitoring interval. This makes the test slow and fragile."

## Proposed Solutions

### Option A: Derive from event_timeout (Recommended)
```rust
let check_interval = (event_timeout / 3).max(Duration::from_secs(1));
let mut interval = tokio::time::interval(check_interval);
```
- **Pros**: Proportional, single line change, makes tests faster
- **Cons**: None
- **Effort**: Small (10 min)
- **Risk**: None

## Technical Details

**Affected files:**
- `sonos-stream/src/subscription/event_detector.rs` — line 169

## Acceptance Criteria

- [ ] Monitoring interval derived from `event_timeout` (not hardcoded 10s)
- [ ] Test `test_event_timeout_sends_polling_request` timeout can be reduced
- [ ] Custom `event_timeout < 10s` configs work correctly

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Performance + Architecture reviewers |

## Resources

- PR: #36
- File: `sonos-stream/src/subscription/event_detector.rs:169`
