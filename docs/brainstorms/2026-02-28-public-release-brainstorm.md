# Public Release of sonos-sdk and sonos-api

**Date:** 2026-02-28
**Status:** Brainstorm
**Approach:** Big Bang — single coordinated effort shipping everything at once

## What We're Building

A complete public release of `sonos-sdk` (high-level DOM-like API) and `sonos-api` (low-level typed UPnP operations) to crates.io as v0.1.0, with all supporting infrastructure for an open-source project that can grow a community.

### Public Crates
- **sonos-sdk** — Primary user-facing crate. Sync-first, DOM-like API for controlling Sonos speakers.
- **sonos-api** — Lower-level typed operations for users who need direct UPnP control.

### Internal Crates (Published as Dependencies)
All transitive dependencies must be on crates.io but will be namespaced and documented as internal:
- `sonos-sdk-discovery` (was `sonos-discovery`) — also avoids existing name collision
- `sonos-sdk-state` (was `sonos-state`)
- `sonos-sdk-state-store` (was `state-store`)
- `sonos-sdk-stream` (was `sonos-stream`)
- `sonos-sdk-event-manager` (was `sonos-event-manager`)
- `sonos-sdk-callback-server` (was `callback-server`)
- `sonos-sdk-soap-client` (was `soap-client`)

Each internal crate gets:
- "This is an internal crate for sonos-sdk and is not intended for direct use" warning in README and rustdoc
- Minimal public documentation
- Published to crates.io (required for dependency resolution)

## Why This Approach

**Big Bang release** chosen over layered rollout because:
- First impressions matter — launching with complete CI, docs, and polish signals a serious project
- Avoids partial states where some pieces are public without supporting infrastructure
- Single coordinated review ensures consistency across all crates
- The codebase is feature-complete through Phase 6; this is purely packaging/infra work

## Key Decisions

### 1. Versioning: v0.1.0
- Signals "early but usable" to the Rust community
- Sets expectations that breaking changes may occur before 1.0
- All crates start at 0.1.0

### 2. License: MIT OR Apache-2.0
- Standard Rust ecosystem dual-license
- Maximum compatibility (matches tokio, serde, etc.)
- LICENSE-MIT and LICENSE-APACHE files at workspace root
- Consistent across all crates

### 3. Crate Naming: `sonos-sdk-*` prefix for internal crates
- Namespaces internal crates under the project umbrella
- Avoids generic names (`soap-client`, `callback-server`) on crates.io's flat namespace
- Makes ownership/relationship clear to anyone browsing crates.io
- Avoids the `sonos-discovery` name collision on crates.io

### 4. Release Automation: release-plz
- Fully automated release flow analyzing conventional commits
- Generates changelogs automatically
- Opens release PRs when changes are detected
- Publishes to crates.io on merge
- Requires adopting conventional commit discipline

### 5. CI: Standard Rust workflow
- `cargo fmt --check` — formatting
- `cargo clippy` — lints
- `cargo test` — all workspace tests
- `cargo doc --no-deps` — documentation builds
- Runs on every PR to main

### 6. Documentation Site: mdBook + GitHub Pages
- Rust ecosystem standard (used by The Rust Book, Tokio)
- Markdown-based, simple authoring
- Built-in search
- GitHub Actions deploys to GitHub Pages on push to main
- Content migrated from existing `docs/` directory

### 7. Community: Minimal but welcoming
- Great root README with Contributing section
- No formal CONTRIBUTING.md, CODE_OF_CONDUCT.md, issue templates yet
- Add formal community files when there's actual community activity
- Lower barrier to shipping v0.1.0

## Scope of Work

### Cargo.toml Overhaul
- [ ] Add `[workspace.package]` to root Cargo.toml with shared metadata (version, edition, license, repository, authors, rust-version)
- [ ] All crate Cargo.toml files inherit from workspace package
- [ ] Add version specifiers to all inter-crate path dependencies (e.g., `version = "0.1.0"`)
- [ ] Remove `publish = false` from sonos-api, soap-client, callback-server
- [ ] Add `publish = false` to integration-example (if re-enabled)
- [ ] Fix placeholder repository URL in sonos-api
- [ ] Remove `authors = ["Claude Code"]` from sonos-stream, sonos-event-manager
- [ ] Fix `#[deprecated(since = "0.2.0")]` to `"0.1.0"` in soap-client
- [ ] Clean phantom dependencies (sonos-stream, callback-server)

### Crate Renaming
- [ ] `sonos-discovery` → `sonos-sdk-discovery`
- [ ] `sonos-state` → `sonos-sdk-state`
- [ ] `state-store` → `sonos-sdk-state-store`
- [ ] `sonos-stream` → `sonos-sdk-stream`
- [ ] `sonos-event-manager` → `sonos-sdk-event-manager`
- [ ] `callback-server` → `sonos-sdk-callback-server`
- [ ] `soap-client` → `sonos-sdk-soap-client`
- [ ] Update all inter-crate dependency references
- [ ] Update all `use` statements and imports
- [ ] Rename directories to match new crate names
- [ ] Update workspace members list

### License
- [ ] Create LICENSE-MIT at workspace root
- [ ] Create LICENSE-APACHE at workspace root
- [ ] Ensure all Cargo.toml files reference `MIT OR Apache-2.0`

### Root README
- [ ] Project overview and elevator pitch
- [ ] Quick start code example (sonos-sdk usage)
- [ ] Feature highlights
- [ ] Architecture overview (simplified)
- [ ] Link to docs site, API docs on docs.rs
- [ ] Contributing section
- [ ] License section

### Per-Crate README Updates
- [ ] sonos-sdk README: add action methods, group management docs (Phase 5-6 features)
- [ ] sonos-api README: review and update for current state
- [ ] Internal crate READMEs: add "internal crate" warnings
- [ ] Fix relative links that break on crates.io

### Rustdoc
- [ ] Add doc comments to `SdkError` variants
- [ ] Update sonos-sdk lib.rs crate docs for action methods and group management
- [ ] Verify `cargo doc --no-deps` builds cleanly with no warnings

### CI/CD (GitHub Actions)
- [ ] `.github/workflows/ci.yml` — PR checks (fmt, clippy, test, doc)
- [ ] `.github/workflows/release.yml` — release-plz automation
- [ ] `.github/workflows/docs.yml` — mdBook build + GitHub Pages deploy

### mdBook Documentation Site
- [ ] `book.toml` configuration at workspace root
- [ ] `book/src/SUMMARY.md` — table of contents
- [ ] Getting Started guide
- [ ] Architecture overview (from existing docs/SUMMARY.md)
- [ ] API reference links (to docs.rs)
- [ ] Property reference (from existing watchable-properties.md)
- [ ] GitHub Pages deployment

### release-plz Configuration
- [ ] `release-plz.toml` at workspace root
- [ ] Configure crate publish order (soap-client → discovery → ... → sonos-sdk)
- [ ] Configure changelog generation
- [ ] Configure which crates to release (skip internal-only)
- [ ] Test release flow with dry-run

## Publish Order

Crates must be published in dependency order (leaf dependencies first):

```
1. sonos-sdk-soap-client      (no workspace deps)
2. sonos-api                   (depends on soap-client)
3. sonos-sdk-discovery         (depends on soap-client, reqwest)
4. sonos-sdk-callback-server   (depends on discovery)
5. sonos-sdk-state-store       (no workspace deps)
6. sonos-sdk-stream            (depends on api, callback-server)
7. sonos-sdk-event-manager     (depends on stream)
8. sonos-sdk-state             (depends on api, stream, event-manager, state-store)
9. sonos-sdk                   (depends on api, state, discovery)
```

## Open Questions

_None — all key decisions resolved during brainstorm._
