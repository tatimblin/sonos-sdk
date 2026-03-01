---
title: "feat: Phase 4 — Publish script and dry run"
type: feat
status: completed
date: 2026-03-01
origin: docs/plans/2026-02-28-feat-public-release-v0.1.0-plan.md
---

# Phase 4 — Publish Script & Dry Run

## Overview

Create a shell script to publish all 9 workspace crates to crates.io in dependency order, then validate with `--dry-run`. This is Phase 4 of the [public release plan](2026-02-28-feat-public-release-v0.1.0-plan.md).

**Prerequisite:** Phases 0–2 are merged to main. Phase 3 (PR #46) should be merged before starting, since the publish dry-run validates README content that Phase 3 created.

## Acceptance Criteria

- [x] `scripts/publish.sh` exists with `--dry-run` support
- [x] Publishes crates in verified dependency order (leaves first, `sonos-sdk` last)
- [x] `./scripts/publish.sh --dry-run` exits 0 for all 9 crates
- [x] No unexpected files in published packages (no `plans/`, `brainstorms/`, etc.)
- [x] Script uses `set -euo pipefail` for safety

## Context

### Publish Order (verified from Cargo.toml dependency graph)

After Phase 0 phantom dep cleanup, the correct dependency-ordered sequence:

```
Leaves (no workspace deps):
  1. sonos-sdk-soap-client
  2. sonos-sdk-discovery
  3. sonos-sdk-state-store
  4. sonos-sdk-callback-server

Level 1:
  5. sonos-api            → soap-client, discovery

Level 2:
  6. sonos-sdk-stream     → api, callback-server

Level 3:
  7. sonos-sdk-event-manager → api, stream, discovery

Level 4:
  8. sonos-sdk-state      → api, stream, event-manager, discovery, state-store

Level 5:
  9. sonos-sdk            → state, api, discovery, event-manager
```

(See brainstorm: `docs/brainstorms/2026-02-28-public-release-brainstorm.md` — "Publish Order" section. The order above is the corrected version after Phase 0 cleanup.)

### Key Design Decision

**Shell script for first publish, release-plz configured afterward** (see brainstorm). A shell script with `--dry-run` support is safer for the initial v0.1.0 because partial failure recovery is clearer. release-plz is set up in Phase 7 after v0.1.0 is live.

## MVP

### `scripts/publish.sh`

```bash
#!/bin/bash
set -euo pipefail

# Publish all workspace crates to crates.io in dependency order.
# Usage: ./scripts/publish.sh [--dry-run]
#
# Leaf crates (no workspace deps) are published first.

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

### Verification Steps

1. `chmod +x scripts/publish.sh`
2. `./scripts/publish.sh --dry-run` — all 9 crates must pass
3. `cargo package -p sonos-sdk --list` — verify no unexpected files
4. `cargo package -p sonos-api --list` — verify no unexpected files

## Sources

- **Origin plan:** [docs/plans/2026-02-28-feat-public-release-v0.1.0-plan.md](2026-02-28-feat-public-release-v0.1.0-plan.md) — Phase 4 section
- **Brainstorm:** [docs/brainstorms/2026-02-28-public-release-brainstorm.md](../brainstorms/2026-02-28-public-release-brainstorm.md) — publish order and shell-script-first decision
- Verified publish order from all 9 Cargo.toml files
