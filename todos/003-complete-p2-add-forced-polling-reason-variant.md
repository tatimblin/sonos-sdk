---
status: complete
priority: p2
issue_id: "003"
tags: [code-review, architecture]
dependencies: []
---

# Add PollingReason::ForcedPolling variant for force_polling_mode

## Problem Statement

When `force_polling_mode` is true, `register_speaker_service()` sets `firewall_status = FirewallStatus::Blocked` and `polling_reason = Some(PollingReason::FirewallBlocked)`. This tells the caller that the firewall is blocking callbacks, but nothing is actually blocked — the system chose to skip UPnP entirely. Consumers interpreting `FirewallStatus::Blocked` to mean "the network is blocking callbacks" would be misled.

## Findings

- **Architecture reviewer**: "Consider adding a PollingReason::ForcedPolling variant so consumers can distinguish 'user chose to skip UPnP' from 'firewall detected blocking callbacks.'"

## Proposed Solutions

### Option A: Add PollingReason::ForcedPolling variant (Recommended)
- Add `ForcedPolling` to `PollingReason` enum in `broker.rs`
- Use it in the force-polling branch instead of `FirewallBlocked`
- Update `Display` impl
- **Pros**: Clear semantic distinction, no breaking change
- **Cons**: One more enum variant
- **Effort**: Small (15 min)
- **Risk**: None

## Technical Details

**Affected files:**
- `sonos-stream/src/broker.rs` — `PollingReason` enum, `register_speaker_service()` force-polling branch

## Acceptance Criteria

- [ ] `PollingReason::ForcedPolling` variant added
- [ ] Force-polling branch uses `ForcedPolling` instead of `FirewallBlocked`
- [ ] `Display` impl updated
- [ ] Test updated

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-24 | Identified during PR #36 review | Architecture reviewer |

## Resources

- PR: #36
- File: `sonos-stream/src/broker.rs:48-57, 456-457`
