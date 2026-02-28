---
status: complete
priority: p2
issue_id: "010"
tags: [code-review, architecture, agent-native]
dependencies: []
---

# Add typed enums for string-typed operation parameters

## Problem Statement

Three Speaker methods accept `&str` where the underlying API validates against a fixed allowlist, but the SDK gives no compile-time discoverability of valid values. An agent or user calling `speaker.set_play_mode("shuffle")` gets a runtime error because the valid value is `"SHUFFLE"`.

## Findings

- **Source**: Agent-Native Reviewer, Architecture Strategist
- **Affected methods**:
  - `seek(unit: &str, target: &str)` — unit must be `"REL_TIME"`, `"TRACK_NR"`, or `"TIME_DELTA"`
  - `set_play_mode(mode: &str)` — must be `"NORMAL"`, `"SHUFFLE"`, `"REPEAT_ALL"`, etc.
  - `configure_sleep_timer(duration: &str)` — must be `HH:MM:SS` format or `""` to cancel
- **Impact**: Runtime validation errors instead of compile-time type safety. Agents cannot discover valid values through type introspection.

## Proposed Solutions

### Option A: SDK-level enums with internal string conversion
```rust
pub enum SeekUnit { RelTime, TrackNr, TimeDelta }
pub enum PlayMode { Normal, RepeatAll, Shuffle, ShuffleNoRepeat, ... }
```
- **Pros**: Self-documenting API, compile-time safety
- **Cons**: More types to maintain, must stay in sync with sonos-api validation
- **Effort**: Small
- **Risk**: Low

### Option B: Keep `&str` but add constants
```rust
pub mod play_modes { pub const NORMAL: &str = "NORMAL"; ... }
```
- **Pros**: Minimal change, still discoverable
- **Cons**: No compile-time enforcement
- **Effort**: Small
- **Risk**: Low

## Recommended Action

Option A for `SeekUnit` and `PlayMode`. For sleep timer duration, add a `cancel_sleep_timer()` convenience method and accept `&str` for the flexible case.

## Acceptance Criteria

- [ ] `seek` accepts a typed `SeekUnit` enum
- [ ] `set_play_mode` accepts a typed `PlayMode` enum
- [ ] Enums have `Display` impl for debug output
- [ ] Existing tests updated

## Resources

- PR #41: https://github.com/tatimblin/sonos-sdk/pull/41
