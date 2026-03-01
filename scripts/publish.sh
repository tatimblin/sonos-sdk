#!/bin/bash
set -euo pipefail

# Publish all workspace crates to crates.io in dependency order.
# Usage: ./scripts/publish.sh [--dry-run]
#
# --dry-run  Validate packaging for all crates without uploading.
#            Leaf crates get a full `cargo publish --dry-run` (they have no
#            unpublished workspace deps). Non-leaf crates use `cargo package
#            --list` to verify file contents, since their workspace deps aren't
#            on crates.io yet for a first publish.
#
# Without --dry-run, publishes for real with a 25 s pause between crates
# for crates.io index propagation.

# Leaf crates have no workspace dependencies — safe for full dry-run.
LEAF_CRATES=(
  sonos-sdk-soap-client
  sonos-sdk-discovery
  sonos-sdk-state-store
  sonos-sdk-callback-server
)

# Non-leaf crates depend on other workspace crates.
NON_LEAF_CRATES=(
  sonos-api
  sonos-sdk-stream
  sonos-sdk-event-manager
  sonos-sdk-state
  sonos-sdk
)

# Combined publish order (leaves first).
ALL_CRATES=("${LEAF_CRATES[@]}" "${NON_LEAF_CRATES[@]}")

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=true
  shift
fi

is_leaf() {
  local crate="$1"
  for leaf in "${LEAF_CRATES[@]}"; do
    [[ "$leaf" == "$crate" ]] && return 0
  done
  return 1
}

for crate in "${ALL_CRATES[@]}"; do
  if [[ "$DRY_RUN" == true ]]; then
    if is_leaf "$crate"; then
      echo "==> $crate (full dry-run)..."
      cargo publish -p "$crate" --dry-run "$@"
    else
      echo "==> $crate (package --list)..."
      cargo package -p "$crate" --list "$@"
    fi
  else
    echo "==> Publishing $crate..."
    cargo publish -p "$crate" "$@"
    echo "    Waiting 25s for crates.io index propagation..."
    sleep 25
  fi
done

if [[ "$DRY_RUN" == true ]]; then
  echo ""
  echo "Dry run complete — all ${#ALL_CRATES[@]} crates validated!"
  echo "  Leaf crates:     full cargo publish --dry-run"
  echo "  Non-leaf crates: cargo package --list (deps not yet on crates.io)"
else
  echo "All crates published!"
fi
