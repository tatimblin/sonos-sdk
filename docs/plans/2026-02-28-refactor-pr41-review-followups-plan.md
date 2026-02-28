---
title: "refactor: PR #41 review follow-ups"
type: refactor
status: completed
date: 2026-02-28
---

# PR #41 Review Follow-ups

Address the remaining suggestions from the PR #41 code review that were not covered by the 7 todo fixes in commit `524f3e4`.

## Tasks

### 1. Add `cancel_sleep_timer()` convenience method

`configure_sleep_timer("")` cancels the timer, but the magic empty string is not discoverable. Add a one-liner convenience method following the pattern of `play()` hiding `speed: "1"`.

#### `sonos-sdk/src/speaker.rs`

```rust
/// Cancel an active sleep timer
pub fn cancel_sleep_timer(&self) -> Result<(), SdkError> {
    self.configure_sleep_timer("")
}
```

No state cache update needed — there's no `SleepTimer` property type.

### 2. Add `SeekTarget` enum replacing `seek(unit, target)` with `seek(target)`

The `seek` method currently takes `(SeekUnit, &str)` where the target format depends on the unit. A single enum with associated data is more ergonomic and prevents mismatched unit/target combinations:

#### `sonos-sdk/src/speaker.rs`

```rust
/// Target for the `seek()` method
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeekTarget {
    /// Seek to a track number (1-based)
    Track(u32),
    /// Seek to an absolute time position (e.g., `"0:02:30"`)
    Time(String),
    /// Seek by a time delta (e.g., `"+0:00:30"` or `"-0:00:10"`)
    Delta(String),
}
```

The `seek` method signature changes from:
```rust
pub fn seek(&self, unit: SeekUnit, target: &str) -> Result<(), SdkError>
```
to:
```rust
pub fn seek(&self, target: SeekTarget) -> Result<(), SdkError>
```

Internally, `SeekTarget` produces both the unit string and target string for the `av_transport::seek()` builder. `SeekUnit` becomes unused and should be removed. Update the re-export in `lib.rs` to export `SeekTarget` instead of `SeekUnit`.

Update the existing test in `test_speaker_action_methods_exist`:
```rust
assert_void(speaker.seek(SeekTarget::Time("0:00:00".to_string())));
```

### 3. Add doc comments about optimistic cache behavior

Write methods now update the state cache optimistically (e.g., `play()` sets `PlaybackState::Playing`). Document this behavior so users/agents understand the cache is best-effort.

#### Module-level note in `sonos-sdk/src/speaker.rs`

Add to the existing module doc comment:

```rust
//! ## Write Operations and State Cache
//!
//! Write methods (e.g., `play()`, `set_volume()`) update the state cache
//! optimistically after the SOAP call succeeds. This means `speaker.volume.get()`
//! reflects the written value immediately. However, if the speaker rejects the
//! command silently, the cache may be stale until the next UPnP event corrects it.
//! Use `speaker.volume.watch()` for authoritative real-time state.
```

#### Method-level notes on the 5 most-used write methods

Add a `///` note to: `play()`, `pause()`, `stop()`, `set_volume()`, `set_mute()`:

```rust
/// Start or resume playback
///
/// Updates the state cache to `PlaybackState::Playing` on success.
pub fn play(&self) -> Result<(), SdkError> {
```

Same pattern for the other 4, referencing the property they update.

#### Module-level note in `sonos-sdk/src/group.rs`

Same pattern for `set_volume()`, `set_mute()`.

### 4. Fix trailing newline in `sonos-api/src/operation/mod.rs`

The file ends with `}` (0x7d) — no trailing newline. Add one. Also check `builder.rs` and `macros.rs` in the same directory (research found they also lack trailing newlines).

### 5. Update PR #41 description

The PR body still references `OperationFailed(String)` and the file change table doesn't mention `sonos-api` changes. Update with `gh pr edit 41 --body`:

- Replace `OperationFailed(String)` references with `ValidationFailed(#[from] ValidationError)`
- Add the `sonos-api` files to the files changed table (macros.rs, mod.rs, operations.rs)
- Add a "Commit 2: Review Fixes" section summarizing the 7 fixes
- Update test counts to reflect final state

### 6. ~~Replace `xml_escape` with `quick-xml`~~ **DROPPED**

Research found that `quick_xml::escape::escape` does **not** escape `'` (apostrophe) — it only handles `&`, `<`, `>`, `"`. The custom `xml_escape()` correctly escapes all 5 XML special characters. The hand-rolled function is actually more correct for SOAP payloads. No change needed.

## Acceptance Criteria

- [x] `cancel_sleep_timer()` method added to Speaker
- [x] `SeekTarget` enum replaces `SeekUnit` + `&str` in `seek()`
- [x] `SeekUnit` removed, `SeekTarget` re-exported from `lib.rs`
- [x] Module-level doc comment about optimistic cache in `speaker.rs`
- [x] Doc notes on `play()`, `pause()`, `stop()`, `set_volume()`, `set_mute()`
- [x] Module-level doc comment about optimistic cache in `group.rs`
- [x] Doc notes on `group.set_volume()`, `group.set_mute()`
- [x] Trailing newline added to `sonos-api/src/operation/mod.rs`
- [x] Trailing newlines added to `builder.rs` and `macros.rs` if missing
- [x] PR #41 description updated to reflect current state
- [x] `cargo build -p sonos-sdk` compiles
- [x] `cargo test -p sonos-sdk` passes
- [x] `cargo clippy -p sonos-sdk` no warnings

## Sources

- PR #41: https://github.com/tatimblin/sonos-sdk/pull/41
- Code review findings: todos/009-015
- `SeekUnit` / `PlayMode` pattern: `sonos-sdk/src/speaker.rs:27-75`
- `configure_sleep_timer`: `sonos-sdk/src/speaker.rs:362`
- `seek`: `sonos-sdk/src/speaker.rs:284`
- `xml_escape`: `sonos-api/src/operation/mod.rs:254-267`
