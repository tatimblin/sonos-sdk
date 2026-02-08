#!/usr/bin/env python3
"""
Test SDK property access against a real Sonos speaker.

Usage:
    python test_sdk_property.py <speaker_ip> <property> [--get|--fetch|--watch]

Examples:
    python test_sdk_property.py 192.168.1.100 volume --get
    python test_sdk_property.py 192.168.1.100 playback_state --fetch
    python test_sdk_property.py 192.168.1.100 volume --watch --duration 30
"""

import argparse
import subprocess
import sys
import time
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]

# Known property mappings (property name → API operation for fetch testing)
FETCHABLE_PROPERTIES = {
    "volume": {
        "service": "RenderingControl",
        "operation": "GetVolume",
        "response_field": "CurrentVolume",
    },
    "playback_state": {
        "service": "AVTransport",
        "operation": "GetTransportInfo",
        "response_field": "CurrentTransportState",
    },
    "position": {
        "service": "AVTransport",
        "operation": "GetPositionInfo",
        "response_field": "RelTime",
    },
}

# Properties that can only be read from cache (event-only)
EVENT_ONLY_PROPERTIES = [
    "mute",
    "bass",
    "treble",
    "loudness",
    "current_track",
    "group_membership",
]


def run_cargo_command(speaker_ip: str, service: str, operation: str) -> dict:
    """Run a sonos-api operation via cargo."""
    # This is a simulation - in practice you'd use the actual SDK
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
            return {"error": result.stderr.strip()}

        # Parse output
        data = {}
        for line in result.stdout.strip().split('\n'):
            if ':' in line:
                key, value = line.split(':', 1)
                data[key.strip()] = value.strip()
        return data

    except subprocess.TimeoutExpired:
        return {"error": "Command timed out"}
    except FileNotFoundError:
        return {"error": "cargo not found"}


def test_get(speaker_ip: str, property_name: str):
    """Test get() - reading from cache"""
    print(f"Testing get() for '{property_name}'")
    print("=" * 50)
    print()
    print("Note: get() returns cached values (no network call)")
    print("The cache is populated by:")
    print("  1. Previous fetch() calls")
    print("  2. UPnP event updates (when watching)")
    print()

    if property_name in FETCHABLE_PROPERTIES:
        print(f"'{property_name}' IS fetchable - cache can be populated via fetch()")
    elif property_name in EVENT_ONLY_PROPERTIES:
        print(f"'{property_name}' is EVENT-ONLY - cache only populated via events")
    else:
        print(f"'{property_name}' - unknown property")

    print()
    print("Example SDK usage:")
    print(f"  let value = speaker.{property_name}.get();")
    print(f"  // Returns Option<{property_name.title().replace('_', '')}> from cache")


def test_fetch(speaker_ip: str, property_name: str):
    """Test fetch() - fetching from device"""
    print(f"Testing fetch() for '{property_name}'")
    print("=" * 50)
    print()

    if property_name not in FETCHABLE_PROPERTIES:
        if property_name in EVENT_ONLY_PROPERTIES:
            print(f"ERROR: '{property_name}' is EVENT-ONLY")
            print("This property doesn't have a dedicated UPnP Get operation.")
            print("It can only be read from cache after being populated by events.")
            print()
            print("Use watch() to receive event updates, then get() to read.")
        else:
            print(f"ERROR: Unknown property '{property_name}'")
        return

    prop_info = FETCHABLE_PROPERTIES[property_name]
    print(f"Fetching '{property_name}' from {speaker_ip}...")
    print(f"  Service: {prop_info['service']}")
    print(f"  Operation: {prop_info['operation']}")
    print()

    result = run_cargo_command(speaker_ip, prop_info['service'], prop_info['operation'])

    if "error" in result:
        print(f"Error: {result['error']}")
        return

    print("Response:")
    for key, value in result.items():
        marker = " <--" if key == prop_info['response_field'] else ""
        print(f"  {key}: {value}{marker}")

    print()
    print("Example SDK usage:")
    print(f"  let value = speaker.{property_name}.fetch()?;")
    print(f"  // Returns Result<{property_name.title().replace('_', '')}, SdkError>")


def test_watch(speaker_ip: str, property_name: str, duration: int):
    """Test watch() - subscribing to changes"""
    print(f"Testing watch() for '{property_name}'")
    print("=" * 50)
    print()
    print(f"Note: watch() registers for UPnP event notifications")
    print(f"Duration: {duration} seconds")
    print()

    print("Example SDK usage:")
    print(f"  let status = speaker.{property_name}.watch()?;")
    print(f"  println!(\"Mode: {{}}\", status.mode);")
    print(f"  println!(\"Current: {{:?}}\", status.current);")
    print()
    print(f"  // Then iterate on system events:")
    print(f"  for event in system.iter() {{")
    print(f"      if event.property_key == \"{property_name}\" {{")
    print(f"          let new_value = speaker.{property_name}.get();")
    print(f"          println!(\"Changed to: {{:?}}\", new_value);")
    print(f"      }}")
    print(f"  }}")
    print()

    if property_name in FETCHABLE_PROPERTIES:
        prop_info = FETCHABLE_PROPERTIES[property_name]
        print(f"Polling {prop_info['service']}/{prop_info['operation']} to simulate event watching...")
        print("-" * 40)

        previous_value = None
        start_time = time.time()

        try:
            while time.time() - start_time < duration:
                result = run_cargo_command(speaker_ip, prop_info['service'], prop_info['operation'])

                if "error" in result:
                    print(f"Error: {result['error']}")
                    time.sleep(2)
                    continue

                current_value = result.get(prop_info['response_field'])

                if previous_value is None:
                    print(f"Initial value: {current_value}")
                elif current_value != previous_value:
                    print(f"Changed: {previous_value} → {current_value}")

                previous_value = current_value
                time.sleep(2)

        except KeyboardInterrupt:
            print("\nStopped by user")

    else:
        print(f"'{property_name}' is event-only - simulated polling not available")


def main():
    parser = argparse.ArgumentParser(
        description="Test SDK property access against Sonos speaker",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python test_sdk_property.py 192.168.1.100 volume --get
  python test_sdk_property.py 192.168.1.100 playback_state --fetch
  python test_sdk_property.py 192.168.1.100 volume --watch --duration 30

Available properties:
  Fetchable: volume, playback_state, position
  Event-only: mute, bass, treble, loudness, current_track, group_membership
        """
    )
    parser.add_argument("speaker_ip", help="IP address of Sonos speaker")
    parser.add_argument("property", help="Property to test")
    parser.add_argument("--get", action="store_true", help="Test get() method")
    parser.add_argument("--fetch", action="store_true", help="Test fetch() method")
    parser.add_argument("--watch", action="store_true", help="Test watch() method")
    parser.add_argument("--duration", type=int, default=30,
                       help="Watch duration in seconds (default: 30)")

    args = parser.parse_args()

    # Validate IP format
    parts = args.speaker_ip.split('.')
    if len(parts) != 4:
        print(f"Error: Invalid IP address format: {args.speaker_ip}")
        sys.exit(1)

    # Default to --get if no action specified
    if not (args.get or args.fetch or args.watch):
        args.get = True

    if args.get:
        test_get(args.speaker_ip, args.property)
    elif args.fetch:
        test_fetch(args.speaker_ip, args.property)
    elif args.watch:
        test_watch(args.speaker_ip, args.property, args.duration)


if __name__ == "__main__":
    main()
