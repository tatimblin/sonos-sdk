#!/usr/bin/env python3
"""
Discover Sonos speakers on the network and allow selection.

Usage:
    python3 discover_speakers.py [timeout_seconds]

Output:
    Prints discovered speakers and prompts for selection.
    Returns the selected speaker's IP address.
"""

import json
import subprocess
import sys
from pathlib import Path


def get_project_root() -> Path:
    """Find the project root (where Cargo.toml is)."""
    current = Path(__file__).resolve()
    for parent in current.parents:
        if (parent / "Cargo.toml").exists():
            return parent
    raise RuntimeError("Could not find project root (Cargo.toml)")


def discover_speakers(timeout: int = 5) -> list[dict]:
    """Discover Sonos speakers using the Rust discovery example."""
    project_root = get_project_root()

    result = subprocess.run(
        ["cargo", "run", "-p", "sonos-discovery", "--example", "discover_json", "--", str(timeout)],
        cwd=project_root,
        capture_output=True,
        text=True
    )

    if result.returncode != 0:
        print(f"Error running discovery: {result.stderr}", file=sys.stderr)
        sys.exit(1)

    # Parse the JSON output
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as e:
        print(f"Error parsing discovery output: {e}", file=sys.stderr)
        print(f"Raw output: {result.stdout}", file=sys.stderr)
        sys.exit(1)


def select_speaker(speakers: list[dict]) -> dict:
    """Display speakers and get user selection."""
    if not speakers:
        print("No Sonos speakers found on the network.", file=sys.stderr)
        print("\nTroubleshooting tips:", file=sys.stderr)
        print("  1. Ensure Sonos speakers are powered on", file=sys.stderr)
        print("  2. Check you're on the same network as speakers", file=sys.stderr)
        print("  3. Verify firewall allows network discovery", file=sys.stderr)
        sys.exit(1)

    print("\n=== Discovered Sonos Speakers ===\n")

    for i, speaker in enumerate(speakers, 1):
        print(f"{i}. {speaker['name']} ({speaker['room_name']})")
        print(f"   IP: {speaker['ip_address']}:{speaker['port']}")
        print(f"   Model: {speaker['model_name']}")
        print(f"   ID: {speaker['id']}")
        print()

    while True:
        try:
            choice = input(f"Select a speaker (1-{len(speakers)}): ").strip()
            if not choice:
                continue

            idx = int(choice) - 1
            if 0 <= idx < len(speakers):
                return speakers[idx]

            print(f"Invalid choice. Please enter a number between 1 and {len(speakers)}")
        except ValueError:
            print("Invalid input. Please enter a number.")
        except KeyboardInterrupt:
            print("\nCancelled.")
            sys.exit(0)


def main():
    timeout = int(sys.argv[1]) if len(sys.argv) > 1 else 5

    print(f"Discovering Sonos speakers (timeout: {timeout}s)...")
    speakers = discover_speakers(timeout)

    selected = select_speaker(speakers)

    print(f"\n=== Selected Speaker ===")
    print(f"Name: {selected['name']}")
    print(f"Room: {selected['room_name']}")
    print(f"IP: {selected['ip_address']}")
    print(f"Model: {selected['model_name']}")

    # Output just the IP for easy scripting
    print(f"\nSPEAKER_IP={selected['ip_address']}")

    return selected


if __name__ == "__main__":
    main()
