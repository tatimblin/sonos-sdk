# Contributing to Sonos SDK

This guide covers the development workflow, CI/CD pipeline, and release process for contributing to the Sonos SDK.

## Prerequisites

- **Rust toolchain**: pinned to **1.98.1** by `rust-toolchain.toml`. `rustup` installs it
  automatically on first `cargo` invocation in the repo, so no manual selection is needed.
  The pin exists so local `cargo clippy` matches CI exactly — lints that move between
  rustc releases otherwise fire in only one of the two places
- **MSRV**: **1.98** (`[workspace.package] rust-version`), edition 2021. The floor comes
  from `ureq` 3.4, an edition-2024 crate. The `msrv` CI job builds the workspace with
  exactly this toolchain
- **Components**: `rustfmt`, `clippy` — declared in `rust-toolchain.toml`, so rustup
  installs them with the pinned toolchain
- **Cargo.lock**: committed — always use `--locked` for reproducible builds

## Development Workflow

### 1. Before You Start

- Read the SPEC file for the crate you're working on: `docs/specs/<crate-name>.md`
- Check `docs/STATUS.md` for the current implementation status
- Review `docs/SUMMARY.md` for architecture context and data flow diagrams

### 2. Branch and Commit Conventions

**Branch naming** follows `<type>/<short-description>`:

```
feat/add-alarm-clock-service
fix/polling-fallback-race
refactor/property-handle-generics
```

**Commit messages** follow [Conventional Commits](https://www.conventionalcommits.org/) — this is required because `release-plz` parses them to generate changelogs and determine version bumps.

```
feat(api): add AlarmClock service operations
fix(stream): handle timeout during firewall detection
refactor(state): simplify decoder registration
docs: update STATUS.md after completing GroupManagement
ci: add MSRV check to CI pipeline
test(sdk): add integration tests for Speaker actions
chore: update dependencies
```

**Format**: `<type>(<scope>): <description>`

| Type | Meaning | Version bump |
|------|---------|-------------|
| `feat` | New feature | minor |
| `fix` | Bug fix | patch |
| `refactor` | Code restructuring (no behavior change) | patch |
| `docs` | Documentation only | none |
| `ci` | CI/CD changes | none |
| `test` | Test additions or changes | none |
| `chore` | Maintenance tasks | none |
| `perf` | Performance improvement | patch |

**Scope** is the crate name without prefix: `api`, `sdk`, `stream`, `state`, `discovery`, `ci`.

### 3. Run CI Checks Locally

These four cover the gating jobs and catch nearly everything. Run them before pushing;
they are the exact commands GitHub Actions runs.

```bash
# 1. Formatting
cargo fmt --all -- --check

# 2. Linting (warnings are errors)
cargo clippy --workspace --all-targets --features sonos-sdk/test-support --locked -- -D warnings

# 3. Tests
cargo test --workspace --features sonos-sdk/test-support --locked

# 4. Documentation (warnings are errors)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

The remaining jobs need extra tooling or a second toolchain. Reach for them when a PR
touches dependencies or feature wiring:

```bash
# Feature wiring — nothing may rely on an unnamed default feature of a dependency
cargo check --workspace --no-default-features --locked
cargo check --workspace --all-targets --no-default-features --features sonos-sdk/test-support --locked

# Supply chain (advisories, licenses, bans, sources; config in deny.toml)
cargo install cargo-deny --locked && cargo deny check

# Unused dependencies
cargo install cargo-machete --locked && cargo machete

# MSRV. RUSTUP_TOOLCHAIN is required: it is the only thing that outranks
# rust-toolchain.toml, and without it this builds on 1.98.1 and proves nothing.
rustup toolchain install 1.98
RUSTUP_TOOLCHAIN=1.98 cargo build --workspace --locked
```

To auto-fix formatting: `cargo fmt --all`

**Key flags explained**:
- `--locked` — uses the committed `Cargo.lock`, fails if it's out of date
- `--features sonos-sdk/test-support` — enables test helpers (e.g., `with_groups()`, `from_devices_offline()`)
- `-D warnings` — treats all warnings as errors (both `RUSTFLAGS` and `RUSTDOCFLAGS`)
- `--workspace` — runs against all crates in the workspace

### 4. After Making Changes

- Update `docs/specs/<crate>.md` if you changed a crate's behavior or API
- Update `docs/STATUS.md` if you completed work on a service layer.
  `python .claude/skills/add-service/scripts/service_status.py --all` reads the tree and
  must agree with it
- Write concise, balanced unit tests — avoid excessive test counts but cover key paths
- If the fix turned on a non-obvious causal chain that the code and tests do not explain
  on their own, add an entry to `docs/solutions/<category>/`. That directory is for
  findings a future debugger would otherwise have to rediscover — not for routine fixes,
  whose commit message and diff already say enough. See
  [docs/solutions/logic-errors/watch-after-fetch-event-suppression.md](solutions/logic-errors/watch-after-fetch-event-suppression.md)
  for the shape: symptom, investigation, root cause, fix, prevention rule

## CI Pipeline

All CI runs on `ubuntu-latest`. Every gating job is pinned to Rust 1.98.1 (the
`PINNED_TOOLCHAIN` env, asserted equal to `rust-toolchain.toml` by `toolchain-sync`), so
a CI failure reproduces with a plain local `cargo` command.

### On Pull Requests (and pushes to `main`)

Ten parallel jobs defined in `.github/workflows/ci.yml`:

| Job | What it checks | Command |
|-----|---------------|---------|
| **toolchain sync** | `rust-toolchain.toml` matches `PINNED_TOOLCHAIN` | shell assertion |
| **rustfmt** | Code formatting | `cargo fmt --all -- --check` |
| **clippy** | Lint violations | `cargo clippy --workspace --all-targets --features sonos-sdk/test-support --locked -- -D warnings` |
| **test** | All workspace tests | `cargo test --workspace --features sonos-sdk/test-support --locked` |
| **doc** | Documentation builds | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` |
| **msrv** | Library targets build on the declared MSRV | `RUSTUP_TOOLCHAIN=1.98 cargo build --workspace --locked` |
| **cargo-deny** | Advisories, licenses, bans, sources | `cargo deny check --hide-inclusion-graph` |
| **cargo-machete** | Unused dependencies | `cargo machete` |
| **no-default-features** | No reliance on unnamed dependency default features | `cargo check --workspace --no-default-features --locked` (+ an `--all-targets` pass) |
| **canary (stable, advisory)** | Lints from a newer rustc than the pin | `cargo fmt` + `cargo clippy` on `@stable` |

`canary` is `continue-on-error: true`. A new upstream lint should prompt a deliberate
toolchain bump, not block an unrelated PR. Every other job gates.

The `msrv` job asserts `Cargo.toml`'s `rust-version` equals the `MSRV` env before
installing anything, and builds lib targets only — dev-dependencies are not part of the
MSRV promise to downstream users.

CI uses [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) for build caching on
every job that compiles.

**Concurrency**: duplicate runs for the same branch are cancelled automatically.

**Environment**:
```yaml
CARGO_TERM_COLOR: always
CARGO_INCREMENTAL: 0          # disabled for reproducibility
RUSTFLAGS: "-D warnings"      # warnings = errors
```

### On Merge to `main`

Two additional jobs run via `.github/workflows/release-plz.yml`:

1. **release-plz release** — publishes new versions to crates.io if a release PR was merged
2. **release-plz PR** — opens/updates a release PR based on conventional commit analysis

Both jobs are restricted to the repository owner (`tatimblin`).

## Release Process

Releases are automated by [release-plz](https://release-plz.ieni.dev/), configured in `release-plz.toml`.

### How It Works

1. Conventional commits on `main` are analyzed automatically
2. `release-plz` opens a PR with version bumps and changelog updates
3. Merging the release PR triggers crate publishing to crates.io
4. GitHub releases and git tags are created for public crates

### Public vs Internal Crates

| Category | Crates | Behavior |
|----------|--------|----------|
| **Public** | `sonos-api`, `sonos-sdk` | Full pipeline: semver checks, changelogs, GitHub releases, crates.io publish |
| **Internal** | All others (6 crates) | Version bumped as transitive dependencies, no individual releases or changelogs |

**Tag format**: `<package>-v<version>` (e.g., `sonos-api-v0.2.0`)

The `sonos-sdk` changelog includes changes from all internal crates so that users see the full picture.

### Manual Publishing (First Release Only)

The `scripts/publish.sh` script handles ordered publication for initial releases:

```bash
# Validate all packages
./scripts/publish.sh --dry-run

# Publish to crates.io (25s delay between crates for index propagation)
./scripts/publish.sh
```

Publishes in dependency order: leaf crates first (no workspace deps), then non-leaf crates ascending.

## Workspace Structure Quick Reference

```
sonos-sdk/
├── .github/workflows/
│   ├── ci.yml              # PR checks: 10 jobs (see CI Pipeline above)
│   ├── deploy-docs.yml     # Publishes website/ to GitHub Pages
│   └── release-plz.yml     # Automated releases on main
├── rust-toolchain.toml     # Pins Rust 1.98.1 + rustfmt/clippy
├── release-plz.toml        # Release automation config
├── deny.toml               # cargo-deny policy and exceptions
├── clippy.toml             # Clippy tuning (lint levels live in Cargo.toml)
├── scripts/publish.sh      # Manual publish script
├── .claude/skills/         # Code-generator skills for adding a service
├── website/                # Astro Starlight site (user-facing docs)
├── docs/
│   ├── SUMMARY.md           # Architecture overview
│   ├── STATUS.md            # Service completion matrix
│   ├── specs/               # Per-crate specifications
│   ├── plans/               # Dated implementation plans
│   ├── brainstorms/         # Dated exploration notes
│   ├── solutions/           # Debugging findings worth keeping
│   └── CONTRIBUTING.md      # This file
├── sonos-sdk/               # Public: high-level SDK facade
├── sonos-api/               # Public: type-safe UPnP operations
├── sonos-state/             # Internal: reactive state management
├── sonos-stream/            # Internal: event streaming
├── sonos-event-manager/     # Internal: subscription lifecycle
├── sonos-discovery/         # Internal: SSDP device discovery
├── callback-server/         # Internal: HTTP event reception
└── soap-client/             # Internal: SOAP transport
```

## Troubleshooting

### `Cargo.lock` out of date

If `--locked` fails, run `cargo update` and commit the updated lock file.

### `test-support` feature errors

The `test-support` feature is defined in `sonos-sdk/Cargo.toml`. It enables test helpers like `with_groups()`. Always include `--features sonos-sdk/test-support` when running tests or clippy.

### Lints differ between local and CI

They should not: `rust-toolchain.toml` pins the same 1.98.1 CI uses, and `toolchain-sync`
fails if the two drift. If you see a divergence, check that `RUSTUP_TOOLCHAIN` is not set
in your shell — it overrides the file.

### Clippy or doc warnings failing CI

CI sets `RUSTFLAGS="-D warnings"` and `RUSTDOCFLAGS="-D warnings"`. Fix all warnings locally before pushing — there is no way to bypass this in CI.
