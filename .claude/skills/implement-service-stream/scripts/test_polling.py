#!/usr/bin/env python3
"""
Test polling strategy against a real Sonos speaker.

Usage:
    python test_polling.py <speaker_ip> <service> [--interval SECS] [--count N]

Example:
    python test_polling.py 192.168.1.100 AVTransport --interval 2 --count 5
"""

import argparse
import subprocess
import sys
import time
import json
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]


def run_cargo_example(speaker_ip: str, service: str, operation: str) -> dict:
    """Run a sonos-api operation via cargo example."""
    cmd = [
        "cargo", "run", "-p", "sonos-api", "--example", "cli_example",
        "--", speaker_ip, service, operation
    ]

    try:
        result = subprocess.run(
            cmd,
            cwd=WORKSPACE_ROOT,
            capture_output=True,
            text=True,
            timeout=30
        )

        if result.returncode != 0:
            print(f"Warning: Command failed: {result.stderr}")
            return {}

        # Parse output (simple key: value format)
        data = {}
        for line in result.stdout.strip().split('\n'):
            if ':' in line:
                key, value = line.split(':', 1)
                data[key.strip()] = value.strip()
        return data

    except subprocess.TimeoutExpired:
        print("Warning: Command timed out")
        return {}
    except FileNotFoundError:
        print("Error: cargo not found. Make sure Rust is installed.")
        sys.exit(1)


def get_service_state(speaker_ip: str, service: str) -> dict:
    """Get current state for a service."""
    service_ops = {
        "AVTransport": [
            ("GetTransportInfo", ["CurrentTransportState", "CurrentTransportStatus"]),
            ("GetPositionInfo", ["Track", "TrackDuration", "RelTime"]),
        ],
        "RenderingControl": [
            ("GetVolume", ["CurrentVolume"]),
            ("GetMute", ["CurrentMute"]),
        ],
        "ZoneGroupTopology": [
            ("GetZoneGroupState", []),
        ],
    }

    if service not in service_ops:
        print(f"Warning: Unknown service '{service}'")
        print(f"Known services: {', '.join(service_ops.keys())}")
        return {}

    state = {}
    for op, fields in service_ops[service]:
        result = run_cargo_example(speaker_ip, service, op)
        state.update(result)

    return state


def compare_states(old: dict, new: dict) -> list:
    """Compare two states and return changes."""
    changes = []

    all_keys = set(old.keys()) | set(new.keys())
    for key in sorted(all_keys):
        old_val = old.get(key, "<not present>")
        new_val = new.get(key, "<not present>")
        if old_val != new_val:
            changes.append({
                "field": key,
                "old": old_val,
                "new": new_val,
            })

    return changes


def poll_loop(speaker_ip: str, service: str, interval: float, count: int):
    """Poll speaker and detect changes."""
    print(f"Polling {service} on {speaker_ip}")
    print(f"Interval: {interval}s, Count: {count if count > 0 else 'infinite'}")
    print("=" * 60)

    previous_state = None
    iteration = 0
    total_changes = 0

    try:
        while count <= 0 or iteration < count:
            iteration += 1

            print(f"\n[{iteration}] Polling at {time.strftime('%H:%M:%S')}...")
            current_state = get_service_state(speaker_ip, service)

            if not current_state:
                print("  No data received")
                time.sleep(interval)
                continue

            if previous_state is None:
                print("  Initial state captured:")
                for key, value in sorted(current_state.items()):
                    print(f"    {key}: {value}")
            else:
                changes = compare_states(previous_state, current_state)
                if changes:
                    total_changes += len(changes)
                    print(f"  {len(changes)} change(s) detected:")
                    for change in changes:
                        print(f"    {change['field']}: {change['old']} -> {change['new']}")
                else:
                    print("  No changes")

            previous_state = current_state
            time.sleep(interval)

    except KeyboardInterrupt:
        print("\n\nInterrupted by user")

    print("\n" + "=" * 60)
    print(f"Summary: {iteration} polls, {total_changes} total changes detected")


def main():
    parser = argparse.ArgumentParser(description="Test polling strategy against Sonos speaker")
    parser.add_argument("speaker_ip", help="IP address of Sonos speaker")
    parser.add_argument("service", help="Service to poll (AVTransport, RenderingControl, etc.)")
    parser.add_argument("--interval", type=float, default=5.0, help="Polling interval in seconds (default: 5)")
    parser.add_argument("--count", type=int, default=0, help="Number of polls (0 = infinite, default: 0)")

    args = parser.parse_args()

    # Validate IP format
    parts = args.speaker_ip.split('.')
    if len(parts) != 4:
        print(f"Error: Invalid IP address format: {args.speaker_ip}")
        sys.exit(1)

    poll_loop(args.speaker_ip, args.service, args.interval, args.count)


if __name__ == "__main__":
    main()
