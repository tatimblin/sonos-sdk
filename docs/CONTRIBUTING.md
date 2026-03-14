# Contributing to Sonos SDK

This guide covers the development workflow, CI/CD pipeline, and release process for contributing to the Sonos SDK.

## Prerequisites

- **Rust toolchain**: stable channel (MSRV 1.80, edition 2021)
- **Components**: `rustfmt`, `clippy` (install via `rustup component add rustfmt clippy`)
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
refactor/state-store-generics
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

Run all four CI checks before pushing. These are the exact commands GitHub Actions runs:

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

To auto-fix formatting: `cargo fmt --all`

**Key flags explained**:
- `--locked` — uses the committed `Cargo.lock`, fails if it's out of date
- `--features sonos-sdk/test-support` — enables test helpers (e.g., `with_groups()`)
- `-D warnings` — treats all warnings as errors (both `RUSTFLAGS` and `RUSTDOCFLAGS`)
- `--workspace` — runs against all crates in the workspace

### 4. After Making Changes

- Update `docs/specs/<crate>.md` if you changed a crate's behavior or API
- Update `docs/STATUS.md` if you completed work on a service layer
- Write concise, balanced unit tests — avoid excessive test counts but cover key paths

## CI Pipeline

All CI runs on `ubuntu-latest` with the stable Rust toolchain.

### On Pull Requests (and pushes to `main`)

Four parallel jobs defined in `.github/workflows/ci.yml`:

| Job | What it checks | Command |
|-----|---------------|---------|
| **rustfmt** | Code formatting | `cargo fmt --all -- --check` |
| **clippy** | Lint violations | `cargo clippy --workspace --all-targets --features sonos-sdk/test-support --locked -- -D warnings` |
| **test** | All workspace tests | `cargo test --workspace --features sonos-sdk/test-support --locked` |
| **doc** | Documentation builds | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` |

CI uses [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) for build caching on clippy, test, and doc jobs.

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
| **Internal** | All others (7 crates) | Version bumped as transitive dependencies, no individual releases or changelogs |

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
│   ├── ci.yml              # PR checks: fmt, clippy, test, doc
│   └── release-plz.yml     # Automated releases on main
├── release-plz.toml        # Release automation config
├── scripts/publish.sh       # Manual publish script
├── docs/
│   ├── SUMMARY.md           # Architecture overview
│   ├── STATUS.md            # Service completion matrix
│   ├── specs/               # Per-crate specifications
│   └── CONTRIBUTING.md      # This file
├── sonos-sdk/               # Public: high-level SDK facade
├── sonos-api/               # Public: type-safe UPnP operations
├── sonos-state/             # Internal: reactive state management
├── sonos-stream/            # Internal: event streaming
├── sonos-event-manager/     # Internal: subscription lifecycle
├── sonos-discovery/         # Internal: SSDP device discovery
├── callback-server/         # Internal: HTTP event reception
├── soap-client/             # Internal: SOAP transport
└── state-store/             # Internal: generic state primitives
```

## Troubleshooting

### `Cargo.lock` out of date

If `--locked` fails, run `cargo update` and commit the updated lock file.

### `test-support` feature errors

The `test-support` feature is defined in `sonos-sdk/Cargo.toml`. It enables test helpers like `with_groups()`. Always include `--features sonos-sdk/test-support` when running tests or clippy.

### Clippy or doc warnings failing CI

CI sets `RUSTFLAGS="-D warnings"` and `RUSTDOCFLAGS="-D warnings"`. Fix all warnings locally before pushing — there is no way to bypass this in CI.
