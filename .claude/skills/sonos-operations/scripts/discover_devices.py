#!/usr/bin/env python3
"""
Discover Sonos devices using the sonos-discovery crate.
Provides a Python wrapper around the Rust discovery functionality.
"""

import subprocess
import json
import sys
import re
from typing import List, Dict, Optional

def run_discovery_command() -> Optional[str]:
    """Run the sonos-discovery example to get device list."""
    try:
        # Try to run the discovery example
        result = subprocess.run(
            ['cargo', 'run', '-p', 'sonos-discovery', '--example', 'discover_json'],
            capture_output=True,
            text=True,
            timeout=10
        )

        if result.returncode == 0:
            return result.stdout
        else:
            print(f"Discovery failed: {result.stderr}")
            return None

    except subprocess.TimeoutExpired:
        print("Discovery timed out after 10 seconds")
        return None
    except Exception as e:
        print(f"Error running discovery: {e}")
        return None

def parse_discovery_output(output: str) -> List[Dict]:
    """Parse the JSON output from the discovery command to extract device info."""
    devices = []

    try:
        # Parse JSON output from discover_json example
        json_devices = json.loads(output)

        for device in json_devices:
            devices.append({
                'name': device.get('name', ''),
                'room_name': device.get('room_name', ''),
                'ip_address': device.get('ip_address', ''),
                'id': device.get('id', ''),
                'port': device.get('port', 1400),
                'model_name': device.get('model_name', '')
            })

    except json.JSONDecodeError as e:
        print(f"Error parsing JSON output: {e}")
        print("Raw output:")
        print(output)

    return devices

def discover_devices() -> List[Dict]:
    """Discover Sonos devices and return structured data."""
    print("Discovering Sonos devices...")

    output = run_discovery_command()
    if not output:
        print("No discovery output received")
        return []

    devices = parse_discovery_output(output)

    if not devices:
        print("No devices found or could not parse discovery output")
        print("Raw output:")
        print(output)

    return devices

def format_devices_for_selection(devices: List[Dict]) -> str:
    """Format devices for user selection."""
    if not devices:
        return "No devices found"

    result = "Available Sonos devices:\n"
    for i, device in enumerate(devices, 1):
        room_name = device.get('room_name', '')
        display_name = f"{device['name']}" + (f" ({room_name})" if room_name and room_name != device['name'] else "")
        result += f"{i}. {display_name} - {device['ip_address']}\n"

    return result

def validate_ip_address(ip: str) -> bool:
    """Validate if a string is a valid IP address."""
    parts = ip.split('.')
    if len(parts) != 4:
        return False

    try:
        for part in parts:
            num = int(part)
            if num < 0 or num > 255:
                return False
        return True
    except ValueError:
        return False

def main():
    """Main entry point for device discovery."""
    devices = discover_devices()

    if devices:
        print(format_devices_for_selection(devices))

        # Save to JSON for programmatic use
        with open('discovered_devices.json', 'w') as f:
            json.dump(devices, f, indent=2)

        print(f"\nFound {len(devices)} devices. Details saved to discovered_devices.json")
    else:
        print("No devices discovered. Make sure:")
        print("1. You're on the same network as Sonos devices")
        print("2. The sonos-discovery crate is built")
        print("3. Your firewall allows network discovery")

    return 0 if devices else 1

if __name__ == "__main__":
    exit(main())