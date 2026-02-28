---
status: complete
priority: p3
issue_id: "013"
tags: [code-review, quality, agent-native]
dependencies: []
---

# Change delegate_coordination_to to accept &SpeakerId

## Problem Statement

`delegate_coordination_to(new_coordinator: &str, ...)` accepts a raw string where `SpeakerId` would be more type-safe and consistent with the rest of the SDK API.

## Findings

- **Source**: Pattern Recognition Specialist, Agent-Native Reviewer
- **Location**: `sonos-sdk/src/speaker.rs` line 388
- **Impact**: Agents holding a `Speaker` reference must know to use `.id.as_str()` instead of passing `&speaker.id`

## Proposed Solutions

Change signature to `new_coordinator: &SpeakerId` and convert internally with `.as_str().to_string()`.

## Acceptance Criteria

- [ ] Method accepts `&SpeakerId`
- [ ] Internal conversion to String for UPnP layer

## Resources

- PR #41: https://github.com/tatimblin/sonos-sdk/pull/41
