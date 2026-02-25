---
status: complete
priority: p3
issue_id: "008"
tags: [code-review, quality]
dependencies: ["001"]
---

# Skip EventDetector registration in force-polling mode

## Problem Statement

When `force_polling_mode` is true, the broker registers with `EventDetector` for monitoring (`broker.rs:460-461`), but polling is already started unconditionally. No UPnP events will arrive, so the monitoring loop will detect a "timeout" and try to send a duplicate polling request. The `polling_activated` map suppresses this duplicate, but the simpler fix is to not register in the first place.

## Findings

- **Simplicity reviewer**: "In force-polling mode, registering with EventDetector for monitoring is pointless. If you never register, you don't need the deduplication map."

## Proposed Solutions

### Option A: Skip EventDetector registration in force-polling branch
- Remove the `register_subscription` and `register_pair` calls from the force-polling branch
- **Effort**: Small (5 min)
- **Risk**: None

## Technical Details

**Affected files:**
- `sonos-stream/src/broker.rs` — lines 460-461 in force-polling branch

## Acceptance Criteria

- [ ] Force-polling branch does not call `event_detector.register_subscription()` or `register_pair()`
- [ ] Tests pass

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Simplicity reviewer |

## Resources

- PR: #36
