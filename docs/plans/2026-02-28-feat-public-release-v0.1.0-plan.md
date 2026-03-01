---
title: "feat: Public release of sonos-sdk and sonos-api v0.1.0"
type: feat
status: active
date: 2026-02-28
origin: docs/brainstorms/2026-02-28-public-release-brainstorm.md
---

# Public Release of sonos-sdk and sonos-api v0.1.0

## Overview

Prepare and publish `sonos-sdk` and `sonos-api` to crates.io as v0.1.0, with all supporting infrastructure: crate metadata, licensing, CI/CD, release automation, documentation site, and a root README. Internal crates are renamed with a `sonos-sdk-*` prefix and published as clearly-marked transitive dependencies.

This is a "Big Bang" release — all infrastructure ships in one coordinated effort so the project launches with a polished first impression (see brainstorm: `docs/brainstorms/2026-02-28-public-release-brainstorm.md`).

## Problem Statement

The workspace cannot be published to crates.io. 16 blocking issues exist (documented in the prior Phase 7 plan). Beyond publishing mechanics, the project lacks CI/CD, release automation, a documentation site, and a root README — all essential for an open-source project that hopes to grow a community.

**Note:** This plan supersedes `docs/plans/2026-02-28-feat-phase7-documentation-plan.md`. Where the two documents conflict, this plan is authoritative.

## Proposed Solution

A single large effort organized into 8 sequential phases, each independently verifiable. The phases must execute in order due to dependencies, but tasks within each phase can be parallelized.

## Technical Approach

### Key Design Decisions

All decisions below were made during brainstorming (see brainstorm origin document).

**1. Directory names stay as-is; only Cargo.toml `name` fields change.**

Renaming 7 directories would touch 30+ file paths (workspace members, path deps, AGENTS.md, skills, etc.) for zero functional benefit. Cargo resolves crates by their `name` field, not the directory name. This is standard practice in Rust workspaces.

**2. Use `package` aliases to preserve short import identifiers.**

The `sonos-sdk-*` prefix is for crates.io namespacing only. Source code keeps short imports via Cargo's `package` field:

```toml
# In sonos-api/Cargo.toml — publishes as "sonos-sdk-soap-client" on crates.io
# but source code still writes `use soap_client::SoapClient;`
soap-client = { package = "sonos-sdk-soap-client", path = "../soap-client", version = "0.1.0" }
```

This eliminates ~176 source file changes that would otherwise convert `use soap_client::` to `use sonos_sdk_soap_client::`.

**3. MSRV is 1.80, not 1.75.**

`soap-client/src/lib.rs:11` uses `std::sync::LazyLock`, stabilized in Rust 1.80. The prior Phase 7 plan proposed 1.75 — that was incorrect.

**4. First publish via shell script; release-plz configured afterward.**

release-plz is designed for ongoing releases, not first-time publishing. A shell script with `--dry-run` support is safer for the initial v0.1.0 (if partial failure occurs, recovery is clearer). release-plz is set up after v0.1.0 is live on crates.io.

**5. Fix the 2 pre-existing test failures before CI setup.**

`sonos-stream` has 2 failing tests (`test_filtered_iterator`, `test_sync_iteration`) due to runtime-within-runtime panics. These must be fixed or `#[ignore]`d before CI is added, otherwise CI blocks all PRs from day one.

**6. Internal crates get minimal README with "internal crate" disclaimer.**

Rather than leaving crates.io pages blank, each internal crate gets a 5-line README stating it is an internal dependency of sonos-sdk. The `readme` field is set so crates.io renders it.

### Architecture

```
Public crates (user-facing):
  sonos-sdk          — High-level DOM-like SDK
  sonos-api          — Low-level typed UPnP operations

Internal crates (published as deps, sonos-sdk-* prefix on crates.io):
  soap-client        → sonos-sdk-soap-client
  callback-server    → sonos-sdk-callback-server
  sonos-discovery    → sonos-sdk-discovery
  sonos-state        → sonos-sdk-state
  state-store        → sonos-sdk-state-store
  sonos-stream       → sonos-sdk-stream
  sonos-event-manager → sonos-sdk-event-manager
```

Directory names on disk remain unchanged. Only the `name` field in each crate's `Cargo.toml` changes.

### Implementation Phases

#### Phase 0: Pre-Flight Fixes

Fix blockers that affect all subsequent phases.

- [x] **Fix 2 failing tests in `sonos-stream`** — `test_filtered_iterator` and `test_sync_iteration` in `sonos-stream/src/events/iterator.rs` panic with "Cannot start a runtime from within a runtime". Either fix the runtime nesting or add `#[ignore]` with a `// TODO:` comment and tracking issue.
- [x] **Remove phantom dependencies** — these bloat the published dependency tree:
  - [x] Remove `soap-client` from `sonos-stream/Cargo.toml` (unused in source)
  - [x] Move `sonos-discovery` from `[dependencies]` to `[dev-dependencies]` in `sonos-stream/Cargo.toml` (only used in examples)
  - [x] Remove `soap-client` from `callback-server/Cargo.toml` (unused in source)
- [x] **Fix deprecated annotation** — change `#[deprecated(since = "0.2.0")]` to `since = "0.1.0"` in `soap-client/src/lib.rs:69`
- [x] **Verify clean build** — `cargo build --workspace && cargo test --workspace` passes (81/81 tests, or 79 + 2 ignored)

**Success criteria:** `cargo test --workspace` reports 0 failures.

#### Phase 1: Workspace Metadata & Licensing

Centralize metadata and establish licensing.

- [ ] **Create LICENSE-MIT** at workspace root (full MIT license text, copyright holder: Tristan Timblin)
- [ ] **Create LICENSE-APACHE** at workspace root (full Apache 2.0 license text)
- [ ] **Add `[workspace.package]`** to root `Cargo.toml`:

```toml
[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.80"
license = "MIT OR Apache-2.0"
repository = "https://github.com/tatimblin/sonos-sdk"
keywords = ["sonos", "upnp", "audio", "smart-home"]
categories = ["multimedia::audio", "network-programming"]
```

- [ ] **Update all 9 crate Cargo.toml files** to inherit workspace fields:

  **Public crates** (`sonos-sdk`, `sonos-api`):
  - Inherit: version, edition, rust-version, license, repository, keywords, categories
  - Add: `description`, `readme = "README.md"`
  - `sonos-api`: remove `publish = false`, remove placeholder repository URL

  **Internal crates** (all 7):
  - Inherit: version, edition, rust-version, license, repository
  - Add: `description` (one-line, e.g., "Internal implementation detail of sonos-sdk")
  - Add: `readme = "README.md"` (pointing to minimal internal README)
  - Remove `publish = false` from `soap-client` and `callback-server`
  - Remove `authors = ["Claude Code"]` from `sonos-stream` and `sonos-event-manager`
  - Add `//! Internal implementation detail of sonos-sdk. Not intended for direct use.` to `lib.rs` of each internal crate (if not already present)

- [ ] **Add `exclude` fields** to crates with internal files:
  - `sonos-state/Cargo.toml`: `exclude = ["plans/"]`

- [ ] **Verify** — `cargo build --workspace` succeeds

#### Phase 2: Crate Renaming

Rename the 7 internal crates on crates.io while preserving source code ergonomics.

- [ ] **Update `name` field** in each internal crate's `Cargo.toml`:

| File | Old name | New name |
|------|----------|----------|
| `soap-client/Cargo.toml` | `soap-client` | `sonos-sdk-soap-client` |
| `callback-server/Cargo.toml` | `callback-server` | `sonos-sdk-callback-server` |
| `sonos-discovery/Cargo.toml` | `sonos-discovery` | `sonos-sdk-discovery` |
| `sonos-state/Cargo.toml` | `sonos-state` | `sonos-sdk-state` |
| `state-store/Cargo.toml` | `state-store` | `sonos-sdk-state-store` |
| `sonos-stream/Cargo.toml` | `sonos-stream` | `sonos-sdk-stream` |
| `sonos-event-manager/Cargo.toml` | `sonos-event-manager` | `sonos-sdk-event-manager` |

- [ ] **Update all inter-crate dependency declarations** to use `package` aliases + version fields. Every path dependency becomes a triple: `package` (new crates.io name) + `path` (unchanged directory) + `version` ("0.1.0"). The dependency key (left side of `=`) stays the same as the old crate name so all `use` statements remain unchanged.

Example transformation for `sonos-api/Cargo.toml`:
```toml
# Before:
soap-client = { path = "../soap-client" }
sonos-discovery = { path = "../sonos-discovery" }

# After:
soap-client = { package = "sonos-sdk-soap-client", path = "../soap-client", version = "0.1.0" }
sonos-discovery = { package = "sonos-sdk-discovery", path = "../sonos-discovery", version = "0.1.0" }
```

Full dependency update list (after phantom dep cleanup from Phase 0):

| Consumer Crate | Dependency Key | Package (new crates.io name) | File |
|---|---|---|---|
| sonos-api | soap-client | sonos-sdk-soap-client | `sonos-api/Cargo.toml` |
| sonos-api | sonos-discovery | sonos-sdk-discovery | `sonos-api/Cargo.toml` |
| sonos-stream | callback-server | sonos-sdk-callback-server | `sonos-stream/Cargo.toml` |
| sonos-stream | sonos-api | sonos-api | `sonos-stream/Cargo.toml` |
| sonos-stream (dev) | sonos-discovery | sonos-sdk-discovery | `sonos-stream/Cargo.toml` |
| sonos-event-manager | sonos-api | sonos-api | `sonos-event-manager/Cargo.toml` |
| sonos-event-manager | sonos-stream | sonos-sdk-stream | `sonos-event-manager/Cargo.toml` |
| sonos-event-manager | sonos-discovery | sonos-sdk-discovery | `sonos-event-manager/Cargo.toml` |
| sonos-event-manager (dev) | sonos-state | sonos-sdk-state | `sonos-event-manager/Cargo.toml` |
| sonos-state | sonos-api | sonos-api | `sonos-state/Cargo.toml` |
| sonos-state | sonos-stream | sonos-sdk-stream | `sonos-state/Cargo.toml` |
| sonos-state | sonos-event-manager | sonos-sdk-event-manager | `sonos-state/Cargo.toml` |
| sonos-state | sonos-discovery | sonos-sdk-discovery | `sonos-state/Cargo.toml` |
| sonos-state | state-store | sonos-sdk-state-store | `sonos-state/Cargo.toml` |
| sonos-state (dev) | sonos-discovery | sonos-sdk-discovery | `sonos-state/Cargo.toml` |
| sonos-state (dev) | sonos-event-manager | sonos-sdk-event-manager | `sonos-state/Cargo.toml` |
| sonos-sdk | sonos-state | sonos-sdk-state | `sonos-sdk/Cargo.toml` |
| sonos-sdk | sonos-api | sonos-api | `sonos-sdk/Cargo.toml` |
| sonos-sdk | sonos-discovery | sonos-sdk-discovery | `sonos-sdk/Cargo.toml` |
| sonos-sdk | sonos-event-manager | sonos-sdk-event-manager | `sonos-sdk/Cargo.toml` |

**Note:** `sonos-api` keeps its crate name (it's a public crate, not renamed), so dependencies on `sonos-api` use `version = "0.1.0"` and `path` but no `package` field.

- [ ] **Verify** — `cargo build --workspace && cargo test --workspace` passes. Cargo.lock will regenerate with new crate names (expect a large diff — this is normal).

#### Phase 3: Documentation

Update READMEs and rustdoc for the public release.

- [ ] **Create root workspace README.md** (`/README.md`):
  - Project elevator pitch (Rust SDK for Sonos speakers via UPnP/SOAP)
  - Quick start code example showing `sonos-sdk` usage (discovery → properties → actions)
  - Feature highlights (DOM-like API, reactive state, group management, firewall fallback)
  - Architecture overview (simplified dependency diagram)
  - Links to: docs.rs for API docs, GitHub Pages for guides, crates.io for installation
  - Contributing section (how to build, test, submit PRs)
  - License section (MIT OR Apache-2.0 with links to LICENSE files)
  - Note: "Requires Sonos speakers on the local network. Discovery uses SSDP multicast on port 1400."

- [ ] **Update `sonos-sdk/README.md`**:
  - Add **Speaker Actions** section (play, pause, stop, set_volume, set_mute, seek, set_play_mode)
  - Add **Group Management** section (groups, create_group, dissolve, join, leave)
  - Add **Group Properties** section (group.volume, group.mute, group.volume_changeable)
  - Update Quick Start to include an action method example and `[dependencies]` snippet
  - Expand Error Handling to cover all SdkError variants
  - Fix relative links (`../sonos-api`) to absolute URLs (`https://crates.io/crates/sonos-api`)
  - Fix License section: "MIT License" → "MIT OR Apache-2.0"

- [ ] **Create minimal READMEs for internal crates** (5 lines each):
  ```markdown
  # sonos-sdk-soap-client

  Internal implementation detail of [sonos-sdk](https://crates.io/crates/sonos-sdk).
  This crate is not intended for direct use. Its API may change without notice.
  ```
  Create for: soap-client, callback-server, sonos-discovery, sonos-state, state-store, sonos-stream, sonos-event-manager. (Note: some already have READMEs — replace the content with the short internal disclaimer for the crates.io-facing README, or keep the existing detailed README if it's useful for contributors and just add the disclaimer at the top.)

- [ ] **Add doc comments to `SdkError`** — all 9 variants in `sonos-sdk/src/error.rs`, each explaining when the error occurs

- [ ] **Update `sonos-sdk/src/lib.rs` crate-level doc** — mention action methods and group management (Phase 5-6 features)

- [ ] **Add `# Example` blocks** to key methods:
  - `Speaker::play()`, `Speaker::set_volume()`, `Speaker::set_mute()`
  - `Speaker::join_group()`, `Speaker::leave_group()`
  - `Group::dissolve()`

- [ ] **Verify** — `cargo doc --workspace --no-deps` builds with no warnings

#### Phase 4: Publish Script & Dry Run

Create the publish script and validate everything before going live.

- [ ] **Create `scripts/publish.sh`**:

```bash
#!/bin/bash
set -euo pipefail

# Publish all workspace crates to crates.io in dependency order.
# Usage: ./scripts/publish.sh [--dry-run]
#
# Leaf crates (no workspace deps) are published first.
# After phantom dep cleanup, callback-server is also a leaf.

CRATES=(
  sonos-sdk-soap-client
  sonos-sdk-discovery
  sonos-sdk-state-store
  sonos-sdk-callback-server
  sonos-api
  sonos-sdk-stream
  sonos-sdk-event-manager
  sonos-sdk-state
  sonos-sdk
)

for crate in "${CRATES[@]}"; do
  echo "==> Publishing $crate..."
  cargo publish -p "$crate" "$@"
  if [[ "${1:-}" != "--dry-run" ]]; then
    echo "    Waiting 25s for crates.io index propagation..."
    sleep 25
  fi
done
echo "All crates published!"
```

- [ ] **Run dry-run** — `chmod +x scripts/publish.sh && ./scripts/publish.sh --dry-run`
- [ ] **Verify** — all 9 crates pass `cargo publish --dry-run` with no errors
- [ ] **Check package sizes** — `cargo package -p sonos-sdk --list` for each public crate, ensure no unexpected files (plans/, brainstorms/)

**Success criteria:** `./scripts/publish.sh --dry-run` exits 0.

#### Phase 5: CI Setup (GitHub Actions)

Add CI that runs on every PR and push to main.

- [ ] **Create `.github/workflows/ci.yml`**:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  CARGO_INCREMENTAL: 0
  RUSTFLAGS: "-D warnings"

jobs:
  fmt:
    name: rustfmt
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all -- --check

  clippy:
    name: clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --workspace --all-targets -- -D warnings

  test:
    name: test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace --locked

  doc:
    name: doc
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo doc --workspace --no-deps
        env:
          RUSTDOCFLAGS: "-D warnings"
```

- [ ] **Verify** — push a test branch, confirm all 4 jobs pass

#### Phase 6: First Publish to crates.io

The actual v0.1.0 release.

- [ ] **Ensure `CARGO_REGISTRY_TOKEN` is available** — get from [crates.io/settings/tokens](https://crates.io/settings/tokens) with `publish-new` and `publish-update` scopes
- [ ] **Run the publish script** — `./scripts/publish.sh`
- [ ] **Verify on crates.io** — check that all 9 crates appear, READMEs render correctly, dependency links work
- [ ] **Verify on docs.rs** — check that `sonos-sdk` and `sonos-api` documentation builds (may take 15-30 minutes)
- [ ] **Create GitHub Release** — tag `v0.1.0` on the publish commit, write release notes summarizing the initial release

#### Phase 7: release-plz Setup

Configure automated releases for future versions. This runs AFTER v0.1.0 is live.

- [ ] **Create `release-plz.toml`** at workspace root:

```toml
[workspace]
git_release_enable = false
git_tag_name = "{{ package }}-v{{ version }}"

# Internal crates: bump versions but don't publish or create releases
[[package]]
name = "sonos-sdk-soap-client"
publish = false
changelog_update = false
git_release_enable = false

[[package]]
name = "sonos-sdk-callback-server"
publish = false
changelog_update = false
git_release_enable = false

[[package]]
name = "sonos-sdk-discovery"
publish = false
changelog_update = false
git_release_enable = false

[[package]]
name = "sonos-sdk-stream"
publish = false
changelog_update = false
git_release_enable = false

[[package]]
name = "sonos-sdk-event-manager"
publish = false
changelog_update = false
git_release_enable = false

[[package]]
name = "sonos-sdk-state"
publish = false
changelog_update = false
git_release_enable = false

[[package]]
name = "sonos-sdk-state-store"
publish = false
changelog_update = false
git_release_enable = false

# Public crates: full release pipeline
[[package]]
name = "sonos-api"
git_release_enable = true
semver_check = true

[[package]]
name = "sonos-sdk"
git_release_enable = true
semver_check = true
changelog_include = [
  "sonos-api",
  "sonos-sdk-state",
  "sonos-sdk-discovery",
  "sonos-sdk-stream",
  "sonos-sdk-event-manager",
  "sonos-sdk-callback-server",
  "sonos-sdk-soap-client",
  "sonos-sdk-state-store",
]
```

- [ ] **Create `.github/workflows/release-plz.yml`**:

```yaml
name: Release-plz

permissions: {}

on:
  push:
    branches: [main]

jobs:
  release-plz-release:
    name: Release-plz release
    runs-on: ubuntu-latest
    if: ${{ github.repository_owner == 'tatimblin' }}
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - uses: release-plz/action@v0.5
        with:
          command: release
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

  release-plz-pr:
    name: Release-plz PR
    runs-on: ubuntu-latest
    if: ${{ github.repository_owner == 'tatimblin' }}
    permissions:
      contents: write
      pull-requests: write
    concurrency:
      group: release-plz-${{ github.ref }}
      cancel-in-progress: false
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - uses: release-plz/action@v0.5
        with:
          command: release-pr
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

- [ ] **Add `CARGO_REGISTRY_TOKEN`** as a repository secret in GitHub Settings → Secrets → Actions
- [ ] **Adopt conventional commits** — all future commits to main follow the format: `feat:`, `fix:`, `refactor:`, `docs:`, `ci:`, `chore:`, `test:`, `perf:`

#### Phase 8: mdBook Documentation Site

Set up the documentation site and deploy to GitHub Pages.

- [ ] **Create `book.toml`** at workspace root:

```toml
[book]
title = "Sonos SDK"
description = "Documentation for the Sonos SDK — a Rust SDK for controlling Sonos speakers via UPnP/SOAP."
language = "en"
src = "book/src"

[build]
build-dir = "book/output"
create-missing = false

[output.html]
git-repository-url = "https://github.com/tatimblin/sonos-sdk"
edit-url-template = "https://github.com/tatimblin/sonos-sdk/edit/main/book/src/{path}"

[output.html.search]
enable = true

[output.html.playground]
runnable = false
```

- [ ] **Create `book/src/SUMMARY.md`**:

```markdown
# Summary

[Introduction](README.md)

# Getting Started

- [Installation](getting-started/installation.md)
- [Quick Start](getting-started/quick-start.md)
- [Device Discovery](getting-started/discovery.md)

# User Guide

- [Properties: get, fetch, watch](guide/properties.md)
- [Speaker Actions](guide/speaker-actions.md)
- [Group Management](guide/group-management.md)
- [Error Handling](guide/error-handling.md)

# API Reference

- [sonos-sdk](api/sonos-sdk.md)
- [sonos-api](api/sonos-api.md)

# Architecture

- [Overview](architecture/overview.md)
- [Service Completion Matrix](architecture/status.md)

# Guides

- [Adding New Services](guides/adding-services.md)
- [Watchable Properties Reference](guides/watchable-properties.md)
```

- [ ] **Create book content pages** — migrate and adapt content from existing `docs/` files and crate READMEs. Key mappings:
  - `docs/SUMMARY.md` → `book/src/architecture/overview.md`
  - `docs/STATUS.md` → `book/src/architecture/status.md`
  - `docs/adding-services.md` → `book/src/guides/adding-services.md`
  - `docs/watchable-properties.md` → `book/src/guides/watchable-properties.md`
  - `sonos-sdk/README.md` → informs `book/src/getting-started/quick-start.md`
  - API reference pages are thin bridges to docs.rs

- [ ] **Add `book/output/` to `.gitignore`**

- [ ] **Create `.github/workflows/deploy-book.yml`**:

```yaml
name: Deploy Documentation

on:
  push:
    branches: [main]
    paths:
      - 'book/**'
      - 'book.toml'
  workflow_dispatch:

concurrency:
  group: pages
  cancel-in-progress: false

permissions:
  contents: read
  pages: write
  id-token: write

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install mdBook
        run: |
          tag=$(curl -s 'https://api.github.com/repos/rust-lang/mdbook/releases/latest' | jq -r '.tag_name')
          url="https://github.com/rust-lang/mdbook/releases/download/${tag}/mdbook-${tag}-x86_64-unknown-linux-gnu.tar.gz"
          mkdir -p bin
          curl -sSL "$url" | tar -xz --directory=bin
          echo "$PWD/bin" >> $GITHUB_PATH
      - name: Build book
        run: mdbook build
      - uses: actions/configure-pages@v4
      - uses: actions/upload-pages-artifact@v3
        with:
          path: book/output

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

- [ ] **Enable GitHub Pages** — in repo Settings → Pages, set Source to "GitHub Actions"
- [ ] **Verify** — push to main, confirm the docs site deploys and is accessible

### Post-Release Checklist

After all phases are complete:

- [ ] **Update `CLAUDE.md`** — reflect new crate names in examples and architecture descriptions
- [ ] **Update `AGENTS.md`** — update crate classification (public vs internal) and any directory references
- [ ] **Update `docs/STATUS.md`** — mark Phase 7 complete
- [ ] **Update `.claude/skills/`** — update any skill files that reference old crate names in code patterns

## System-Wide Impact

### Interaction Graph

Renaming crates touches the `name` field in 7 Cargo.toml files and the `package` alias in ~20 dependency declarations. `use` statements in `.rs` files are **not affected** due to the package alias strategy. Cargo.lock regenerates entirely.

### Error & Failure Propagation

Publishing failures are contained per-crate. If crate N fails to publish, crates N+1 through 9 will also fail (dependency not found). Recovery: fix the issue for crate N and re-run the script from that point.

### State Lifecycle Risks

Partial publish is the main risk. If 4 of 9 crates publish successfully but crate 5 fails, the first 4 are live on crates.io with no way to un-publish. This is acceptable because leaf crates are harmless on their own — they have no users until `sonos-sdk` (published last) is live.

### API Surface Parity

The public API of `sonos-sdk` and `sonos-api` does not change. This is purely packaging and infrastructure work. No breaking changes to any type, trait, or method signature.

## Acceptance Criteria

### Functional Requirements

- [ ] `cargo publish --dry-run` succeeds for all 9 crates
- [ ] All 9 crates are published to crates.io as v0.1.0
- [ ] `sonos-sdk` and `sonos-api` README renders correctly on crates.io
- [ ] Internal crates show "internal implementation detail" disclaimer on crates.io
- [ ] docs.rs builds documentation for `sonos-sdk` and `sonos-api` successfully
- [ ] GitHub Actions CI runs on PRs (fmt, clippy, test, doc — all pass)
- [ ] mdBook documentation site is live on GitHub Pages
- [ ] `cargo test --workspace` passes with 0 failures

### Non-Functional Requirements

- [ ] LICENSE-MIT and LICENSE-APACHE exist at workspace root
- [ ] All crates declare `license = "MIT OR Apache-2.0"` (consistent)
- [ ] All inter-crate path deps include `version = "0.1.0"`
- [ ] No `authors = ["Claude Code"]` in any crate
- [ ] No `publish = false` on any crate that needs publishing
- [ ] Root README exists with quick start, contributing section, and license
- [ ] SdkError variants have doc comments
- [ ] Key Speaker/Group methods have `# Example` blocks

### Quality Gates

- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo doc --workspace --no-deps` builds with no warnings
- [ ] `cargo fmt --all -- --check` passes

## Publish Order (Verified)

After phantom dep cleanup, the correct dependency-ordered publish sequence:

```
Leaves (no workspace deps):
  1. sonos-sdk-soap-client
  2. sonos-sdk-discovery
  3. sonos-sdk-state-store
  4. sonos-sdk-callback-server

Level 1:
  5. sonos-api           → sonos-sdk-soap-client, sonos-sdk-discovery

Level 2:
  6. sonos-sdk-stream    → sonos-api, sonos-sdk-callback-server

Level 3:
  7. sonos-sdk-event-manager → sonos-api, sonos-sdk-stream, sonos-sdk-discovery

Level 4:
  8. sonos-sdk-state     → sonos-api, sonos-sdk-stream, sonos-sdk-event-manager,
                           sonos-sdk-discovery, sonos-sdk-state-store

Level 5:
  9. sonos-sdk           → sonos-sdk-state, sonos-api, sonos-sdk-discovery,
                           sonos-sdk-event-manager
```

## Dependencies & Prerequisites

- **crates.io account** with publishing token
- **GitHub repo settings** — Pages enabled with "GitHub Actions" source, `CARGO_REGISTRY_TOKEN` secret added
- **Conventional commit discipline** adopted for all future commits (required for release-plz changelog generation)

## Risk Analysis & Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Partial publish failure | Medium | High | Publish script has `set -euo pipefail`; dry-run first; leaf crates are harmless alone |
| crates.io name squatting | Low | High | `sonos-sdk` and `sonos-api` are verified available; `sonos-sdk-*` prefix is unique |
| MSRV incorrect | Low | Medium | Set to 1.80 based on `LazyLock` usage; add MSRV CI check in future |
| Cargo.lock merge conflicts | High | Low | Expected — regenerates entirely on rename. Review as a single large diff. |
| release-plz misconfiguration | Medium | Low | Configured after v0.1.0 is live; first publish is manual |

## Sources & References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-02-28-public-release-brainstorm.md](../brainstorms/2026-02-28-public-release-brainstorm.md) — Key decisions: Big Bang approach, `sonos-sdk-*` prefix for internal crates, MIT OR Apache-2.0, release-plz, mdBook, v0.1.0
- **Prior Phase 7 plan:** [docs/plans/2026-02-28-feat-phase7-documentation-plan.md](2026-02-28-feat-phase7-documentation-plan.md) — Superseded by this plan. Publishing infrastructure tasks carried forward; CI/mdBook/release-plz added.

### Internal References

- Dependency graph: verified from all 9 `Cargo.toml` files
- Phantom deps: `sonos-stream/Cargo.toml:13-14`, `callback-server/Cargo.toml:16`
- LazyLock usage: `soap-client/src/lib.rs:11`
- Deprecated annotation: `soap-client/src/lib.rs:69`
- Failing tests: `sonos-stream/src/events/iterator.rs:339`
- SdkError: `sonos-sdk/src/error.rs`

### External References

- release-plz docs: https://release-plz.dev/docs
- mdBook user guide: https://rust-lang.github.io/mdBook/
- crates.io publishing: https://doc.rust-lang.org/cargo/reference/publishing.html
- Swatinem/rust-cache: https://github.com/Swatinem/rust-cache
- dtolnay/rust-toolchain: https://github.com/dtolnay/rust-toolchain
