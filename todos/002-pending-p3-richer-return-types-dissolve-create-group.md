---
status: complete
priority: p3
issue_id: "002"
tags: [code-review, api-design, sonos-sdk]
dependencies: []
---

# Consider richer return types for dissolve() and create_group()

## Problem Statement

`Group::dissolve()` and `SonosSystem::create_group()` both return `Result<(), SdkError>`, discarding `AddMemberResponse` data from individual operations. Callers have no visibility into per-speaker results.

## Findings

- **Source:** agent-native-reviewer
- **Location:** `sonos-sdk/src/group.rs` — `dissolve()`, `sonos-sdk/src/system.rs` — `create_group()`
- **Evidence:** `dissolve()` calls `remove_speaker()` per member (returns `()`). `create_group()` calls `add_speaker()` per member but discards the `AddMemberResponse`.
- **Impact:** Very low — most callers only care about success/failure. The topology events provide the real post-mutation state.

## Proposed Solutions

### Option A: Keep as-is (Recommended)
- `Result<(), SdkError>` is the simplest API
- Topology events provide authoritative post-mutation state
- Users who need per-operation results can call `add_speaker()` individually
- **Pros:** Simple API, matches convention
- **Cons:** No per-speaker operation visibility
- **Effort:** None
- **Risk:** None

### Option B: Return Vec of results
- `dissolve() -> Result<Vec<()>, SdkError>` or `create_group() -> Result<Vec<AddMemberResponse>, SdkError>`
- **Pros:** Richer info for callers
- **Cons:** More complex API, partial failure semantics unclear (what index failed?)
- **Effort:** Small
- **Risk:** Low, but API churn

## Recommended Action

Option A — Keep as-is. The simplicity is correct for the use case.

## Work Log

| Date | Action | Learnings |
|------|--------|-----------|
| 2026-02-28 | Created from PR #42 code review | Topology events are the authoritative source of post-mutation state |

## Resources

- PR #42: https://github.com/tatimblin/sonos-sdk/pull/42
