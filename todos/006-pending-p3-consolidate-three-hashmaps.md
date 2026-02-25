---
status: pending
priority: p3
issue_id: "006"
tags: [code-review, performance, architecture, quality]
dependencies: []
---

# Consolidate EventDetector's 3 HashMaps into single MonitoredRegistration struct

## Problem Statement

`EventDetector` maintains 3 separate `Arc<RwLock<HashMap<RegistrationId, _>>>` that are always inserted/removed together:
- `last_event_times` — SystemTime (or Instant after todo #004)
- `registration_pairs` — SpeakerServicePair
- `polling_activated` — bool

This creates 3 separate lock acquisitions in the monitoring loop (O(n) per tick), a TOCTOU race window between acquisitions, and inconsistent cleanup states during unregistration.

## Findings

- **Simplicity reviewer**: "All three maps are keyed on the same RegistrationId and are always inserted/removed together. This is a classic sign that the values belong in a single struct."
- **Performance reviewer**: "Reduces lock contention from O(n) to O(1) per tick."
- **Architecture reviewer**: "registration_pairs duplicates data that already exists in SpeakerServiceRegistry."

## Proposed Solutions

### Option A: Single struct HashMap
```rust
struct MonitoredRegistration {
    last_event_time: Instant,
    pair: SpeakerServicePair,
    polling_activated: bool,
}
registrations: Arc<RwLock<HashMap<RegistrationId, MonitoredRegistration>>>,
```
- **Effort**: Medium (1 hour)
- **Risk**: Low

### Option B: Pass Arc<SpeakerServiceRegistry> to EventDetector
- Eliminates `registration_pairs` entirely by reading from the existing registry
- **Effort**: Medium
- **Risk**: Low — adds coupling to registry

## Technical Details

**Affected files:**
- `sonos-stream/src/subscription/event_detector.rs` — struct, all methods
- `sonos-stream/src/broker.rs` — registration calls

## Acceptance Criteria

- [ ] Single HashMap with composite value struct
- [ ] `register_pair()` merged into `register_subscription()`
- [ ] `unregister_subscription()` is a single `remove()` call
- [ ] Tests pass

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Consensus across simplicity, performance, architecture |

## Resources

- PR: #36
