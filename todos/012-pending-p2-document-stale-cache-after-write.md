---
status: complete
priority: p2
issue_id: "012"
tags: [code-review, documentation]
dependencies: []
---

# Document stale-cache behavior after write operations

## Problem Statement

Write methods (`set_volume`, `play`, etc.) do NOT update the state cache. After `speaker.set_volume(50)`, calling `speaker.volume.get()` returns the old value until a UPnP event arrives or `speaker.volume.fetch()` is called. This is architecturally correct but undocumented — users and agents may assume the cache reflects the write immediately.

## Findings

- **Source**: Architecture Strategist, Agent-Native Reviewer
- **Location**: All 33 write methods in `sonos-sdk/src/speaker.rs` and `sonos-sdk/src/group.rs`
- **Impact**: User confusion when `get()` returns stale data after a write

## Proposed Solutions

### Option A: Add doc comment to most-used write methods
Add a note to `set_volume`, `set_mute`, `play`, `pause`, `stop` at minimum:
```rust
/// Set speaker volume (0-100)
///
/// Note: The cached value from `speaker.volume.get()` will not update
/// until a UPnP event arrives. Call `speaker.volume.fetch()` for an
/// immediate read-back, or use `speaker.volume.watch()` for automatic updates.
```
- **Pros**: Clear guidance at the point of use
- **Cons**: Repetitive across methods
- **Effort**: Small
- **Risk**: None

### Option B: Module-level documentation
Add a section to the speaker module doc comment explaining the write-then-read pattern.
- **Pros**: Single place, comprehensive
- **Cons**: Less visible when reading individual method docs
- **Effort**: Small
- **Risk**: None

## Recommended Action

Option A for the 5-6 most common write methods, plus Option B for the module overview.

## Acceptance Criteria

- [ ] At least `set_volume`, `play`, `set_mute` doc comments mention stale cache
- [ ] Module doc or crate doc mentions the write/cache relationship

## Resources

- PR #41: https://github.com/tatimblin/sonos-sdk/pull/41
