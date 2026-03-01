---
title: "feat: Phase 3 — Documentation for public release"
type: feat
status: completed
date: 2026-03-01
origin: docs/plans/2026-02-28-feat-public-release-v0.1.0-plan.md
---

# Phase 3 — Documentation for Public Release

## Overview

Update READMEs, rustdoc, and crate-level documentation for the v0.1.0 public release on crates.io. This is Phase 3 of the [public release plan](2026-02-28-feat-public-release-v0.1.0-plan.md). Phases 0–2 (pre-flight fixes, workspace metadata, crate renaming) are complete.

## Current State Assessment

### What exists

- `sonos-sdk/README.md` — comprehensive but missing Speaker Actions, Group Management, Group Properties sections
- `sonos-api/README.md` — comprehensive, good for contributors
- All 7 internal crates have READMEs — some detailed (sonos-state, sonos-stream, sonos-event-manager, state-store), some minimal (soap-client, callback-server)
- `sonos-sdk/src/lib.rs` — crate-level doc exists but doesn't mention action methods or group management
- Most key structs (Speaker, Group, SonosSystem) have `# Example` doc blocks
- Several methods already have `# Example` blocks (seek, set_play_mode, coordinator, members, groups, create_group, etc.)
- **No root workspace README.md exists**

### What needs to be done

| Task | File(s) | Effort |
|------|---------|--------|
| Create root README.md | `/README.md` | Medium |
| Update sonos-sdk README | `sonos-sdk/README.md` | Medium |
| Add internal crate disclaimers | 7 READMEs | Small |
| Add SdkError doc comments | `sonos-sdk/src/error.rs` | Small |
| Update lib.rs crate doc | `sonos-sdk/src/lib.rs` | Small |
| Add example blocks to key methods | `sonos-sdk/src/speaker.rs`, `sonos-sdk/src/group.rs` | Small |
| Verify cargo doc | — | Small |

## Acceptance Criteria

- [x] Root README.md exists with elevator pitch, quick start, features, architecture, contributing, license
- [x] sonos-sdk/README.md includes Speaker Actions, Group Management, Group Properties sections
- [x] sonos-sdk/README.md has `[dependencies]` snippet and correct License section
- [x] sonos-sdk/README.md links use absolute URLs (not relative `../sonos-api`)
- [x] 7 internal crate READMEs have "internal implementation detail" disclaimer at top
- [x] All 9 SdkError variants have `///` doc comments
- [x] lib.rs crate-level doc mentions action methods and group management
- [x] `play()`, `set_volume()`, `set_mute()`, `join_group()`, `leave_group()`, `dissolve()` have `# Example` blocks
- [x] `cargo doc --workspace --no-deps` builds with no warnings

## Implementation Tasks

### Task 1: Create root workspace README.md

**File:** `/README.md`

Content structure:
- **Header + badges** — crate version, docs.rs, CI status, license
- **Elevator pitch** — one paragraph: "Rust SDK for Sonos speakers via UPnP/SOAP. Sync-first, DOM-like API with reactive state management."
- **Quick start** — 3-step: add dependency, discover speakers, control them (show get/fetch/watch + play/set_volume)
- **Feature highlights** — bullet list: sync-first API, DOM-like property access, three access patterns (get/fetch/watch), automatic UPnP subscriptions, speaker actions (play/pause/volume/etc.), group management (create/dissolve/join), firewall fallback (automatic polling), type safety
- **Architecture overview** — simplified diagram showing public vs internal crates:
  ```
  sonos-sdk  (high-level, sync-first)
  sonos-api  (low-level, type-safe UPnP)
  ```
- **Links** — docs.rs, crates.io, GitHub Pages (when available)
- **Contributing** — how to build (`cargo build`), test (`cargo test`), lint (`cargo clippy`), submit PRs
- **Network note** — "Requires Sonos speakers on the local network. Discovery uses SSDP multicast on port 1400."
- **License** — "MIT OR Apache-2.0" with links to both LICENSE files

### Task 2: Update sonos-sdk/README.md

**File:** `sonos-sdk/README.md`

Changes:
1. **Add `[dependencies]` snippet** after Quick Start:
   ```toml
   [dependencies]
   sonos-sdk = "0.1"
   ```

2. **Add Speaker Actions section** after Available Properties:
   - Basic playback: play, pause, stop, next, previous
   - Volume/EQ: set_volume, set_mute, set_bass, set_treble, set_loudness, set_relative_volume
   - Seek: seek with SeekTarget variants
   - Play mode: set_play_mode, set_crossfade_mode
   - Queue: add_uri_to_queue, remove_all_tracks_from_queue, save_queue
   - Sleep timer: configure_sleep_timer, cancel_sleep_timer
   - Code example showing play + set_volume

3. **Add Group Management section**:
   - Listing groups: `system.groups()`
   - Creating groups: `system.create_group(&coordinator, &[&members])`
   - Dissolving groups: `group.dissolve()`
   - Joining/leaving: `speaker.join_group(&group)`, `speaker.leave_group()`
   - Code example showing group creation and dissolve

4. **Add Group Properties section** to Available Properties table:
   | Property | Type | Description |
   |----------|------|-------------|
   | `volume` | `GroupVolume` (u16) | Group volume (0-100) |
   | `mute` | `GroupMute` (bool) | Group mute state |
   | `volume_changeable` | `GroupVolumeChangeable` (bool) | Whether group volume can be changed |

5. **Fix License section**: "MIT License" → "MIT OR Apache-2.0"

6. **Fix See Also links**: change relative `../sonos-api` → absolute `https://crates.io/crates/sonos-api`

### Task 3: Create/update internal crate READMEs

**Files:** 7 internal crate READMEs

For crates with existing detailed READMEs (sonos-state, sonos-stream, sonos-event-manager, state-store, sonos-discovery), **add disclaimer at top** preserving the existing content (useful for contributors):

```markdown
> **Internal crate** — this is an implementation detail of [sonos-sdk](https://crates.io/crates/sonos-sdk).
> Its API may change without notice between versions.

# existing content...
```

For crates with minimal READMEs (soap-client, callback-server), **replace with short crates.io-facing README**:

```markdown
# sonos-sdk-soap-client

Internal implementation detail of [sonos-sdk](https://crates.io/crates/sonos-sdk).
This crate is not intended for direct use. Its API may change without notice.
```

Mapping:

| Crate directory | crates.io name | Action |
|------|------|--------|
| soap-client | sonos-sdk-soap-client | Replace (minimal existing) |
| callback-server | sonos-sdk-callback-server | Replace (minimal existing) |
| sonos-discovery | sonos-sdk-discovery | Add disclaimer at top |
| sonos-state | sonos-sdk-state | Add disclaimer at top |
| state-store | sonos-sdk-state-store | Add disclaimer at top |
| sonos-stream | sonos-sdk-stream | Add disclaimer at top |
| sonos-event-manager | sonos-sdk-event-manager | Add disclaimer at top |

### Task 4: Add doc comments to SdkError

**File:** `sonos-sdk/src/error.rs`

Add `///` doc comments to all 9 variants. Current state: none have doc comments (only `#[error]` messages which don't appear in rustdoc).

```rust
/// Errors returned by the sonos-sdk API.
pub enum SdkError {
    /// The internal state manager encountered an error (e.g., failed to start event processing).
    StateError(...),

    /// A UPnP SOAP call to the speaker failed (network error, malformed response, etc.).
    ApiError(...),

    /// The event manager failed to initialize or manage UPnP subscriptions.
    EventManager(String),

    /// No speaker with the given name or ID was found in the system.
    SpeakerNotFound(String),

    /// A speaker's IP address could not be parsed from the discovery response.
    InvalidIpAddress,

    /// A property watcher's channel was closed unexpectedly.
    WatcherClosed,

    /// A `fetch()` call failed to retrieve the property value from the speaker.
    FetchFailed(String),

    /// An operation's parameters failed validation (e.g., volume > 100, bass out of range).
    ValidationFailed(...),

    /// The requested operation is not valid in the current state (e.g., removing coordinator from its own group).
    InvalidOperation(String),
}
```

### Task 5: Update lib.rs crate-level doc

**File:** `sonos-sdk/src/lib.rs`

Update the crate-level doc comment to add sections for:

1. **Speaker Actions** — mention play/pause/stop, volume control, seek, queue ops
2. **Group Management** — mention groups(), create_group(), dissolve(), join/leave
3. **Group Properties** — mention group.volume, group.mute, group.volume_changeable

Insert after the "Available Properties" section, before "Architecture".

### Task 6: Add `# Example` blocks to key methods

**Files:** `sonos-sdk/src/speaker.rs`, `sonos-sdk/src/group.rs`

Methods needing `# Example` blocks (those listed in the plan):

| Method | File | Line |
|--------|------|------|
| `Speaker::play()` | speaker.rs | ~265 |
| `Speaker::set_volume()` | speaker.rs | ~574 |
| `Speaker::set_mute()` | speaker.rs | ~592 |
| `Speaker::join_group()` | speaker.rs | ~553 |
| `Speaker::leave_group()` | speaker.rs | ~561 |
| `Group::dissolve()` | group.rs | ~297 |

All examples use `rust,ignore` since they require network access.

### Task 7: Verify cargo doc

Run `cargo doc --workspace --no-deps` and ensure no warnings.

## Success Criteria

- `cargo doc --workspace --no-deps` builds with 0 warnings
- Root README.md renders correctly on GitHub (check formatting, code blocks, links)
- Internal crate READMEs have disclaimers visible on crates.io

## Sources

- **Origin plan:** [docs/plans/2026-02-28-feat-public-release-v0.1.0-plan.md](2026-02-28-feat-public-release-v0.1.0-plan.md) — Phase 3 section
- **Brainstorm:** [docs/brainstorms/2026-02-28-public-release-brainstorm.md](../brainstorms/2026-02-28-public-release-brainstorm.md)
- SdkError: `sonos-sdk/src/error.rs`
- Speaker methods: `sonos-sdk/src/speaker.rs`
- Group methods: `sonos-sdk/src/group.rs`
- System methods: `sonos-sdk/src/system.rs`
- Crate-level doc: `sonos-sdk/src/lib.rs`
