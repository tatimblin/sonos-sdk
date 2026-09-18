# Public Release of sonos-sdk and sonos-api

**Date:** 2026-02-28
**Status:** Landed
**Approach:** Big Bang — single coordinated effort shipping everything at once

## What It Explored

Taking the workspace from a private multi-crate project to published crates: which crates
are public API versus implementation detail, how to name them on crates.io, licensing, CI,
and where user-facing documentation lives.

## What Shipped

**Crate naming.** Internal crates publish under a `sonos-sdk-` prefix while their source
directories and `use` paths keep the short names:

| Directory | Package name | Classification |
|---|---|---|
| `sonos-sdk` | `sonos-sdk` | Public |
| `sonos-api` | `sonos-api` | Public |
| `sonos-discovery` | `sonos-sdk-discovery` | Internal |
| `sonos-stream` | `sonos-sdk-stream` | Internal |
| `sonos-state` | `sonos-sdk-state` | Internal |
| `sonos-event-manager` | `sonos-sdk-event-manager` | Internal |
| `callback-server` | `sonos-sdk-callback-server` | Internal |
| `soap-client` | `sonos-sdk-soap-client` | Internal |

Eight crates, not nine — `state-store` was deleted rather than published, and its
`Property` trait and `PropertyBag` live in `sonos-state`.

**Licensing.** Dual `MIT OR Apache-2.0`, with `LICENSE-MIT` and `LICENSE-APACHE` at the
workspace root and a consistent `license` field on every crate.

**Releases.** Automated by `release-plz` (`release-plz.toml`), driven by conventional
commits. The two public crates get semver checks, changelogs and GitHub releases; the six
internal ones are version-bumped and published as transitive dependencies with no
changelog of their own. `sonos-sdk`'s changelog includes all six so users see the whole
picture. Tags are `<package>-v<version>`. `scripts/publish.sh` covers ordered first
publication. The workspace is at 0.8.0.

**CI.** `.github/workflows/ci.yml`, ten jobs, every gating one pinned to Rust 1.98.1.

**Documentation site.** An Astro Starlight project in `website/`, deployed to GitHub Pages
by `.github/workflows/deploy-docs.yml`. Content lives under `website/src/content/docs/`
and navigation is generated from `website/src/content.config.ts`.

**Still open:** `CODE_OF_CONDUCT.md` does not exist.

## Where To Read About It Now

- [docs/CONTRIBUTING.md](../CONTRIBUTING.md) — toolchain, CI jobs, release process
- `docs/plans/2026-02-28-feat-public-release-v0.1.0-plan.md`
- `docs/plans/2026-03-01-feat-phase4-publish-script-dry-run-plan.md`
- `docs/plans/2026-05-29-feat-astro-starlight-docs-site-plan.md`
