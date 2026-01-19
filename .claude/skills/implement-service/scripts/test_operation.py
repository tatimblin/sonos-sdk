#!/usr/bin/env python3
"""
Test UPnP operations against a Sonos speaker.

Usage:
    python3 test_operation.py <ip> <service> <action> [param=value...]

Examples:
    python3 test_operation.py 192.168.1.100 AVTransport GetTransportInfo
    python3 test_operation.py 192.168.1.100 RenderingControl GetVolume Channel=Master
    python3 test_operation.py 192.168.1.100 AVTransport Play Speed=1

This script wraps the Rust test_operation example for easier use.
"""

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


def test_operation(ip: str, service: str, action: str, params: list[str]) -> tuple[int, str, str]:
    """Run the test_operation Rust example.

    Returns: (exit_code, stdout, stderr)
    """
    project_root = get_project_root()

    cmd = [
        "cargo", "run", "-p", "sonos-api", "--example", "test_operation",
        "--", ip, service, action
    ] + params

    result = subprocess.run(
        cmd,
        cwd=project_root,
        capture_output=True,
        text=True
    )

    return result.returncode, result.stdout, result.stderr


def main():
    if len(sys.argv) < 4:
        print("Usage: python3 test_operation.py <ip> <service> <action> [param=value...]")
        print()
        print("Services: AVTransport, RenderingControl, ZoneGroupTopology, GroupRenderingControl")
        print()
        print("Examples:")
        print("  python3 test_operation.py 192.168.1.100 AVTransport GetTransportInfo")
        print("  python3 test_operation.py 192.168.1.100 RenderingControl GetVolume Channel=Master")
        print("  python3 test_operation.py 192.168.1.100 AVTransport Play Speed=1")
        sys.exit(1)

    ip = sys.argv[1]
    service = sys.argv[2]
    action = sys.argv[3]
    params = sys.argv[4:]

    print(f"Testing {service}::{action} on {ip}...")
    print()

    exit_code, stdout, stderr = test_operation(ip, service, action, params)

    if stdout:
        print(stdout)
    if stderr:
        print(stderr, file=sys.stderr)

    sys.exit(exit_code)


if __name__ == "__main__":
    main()
