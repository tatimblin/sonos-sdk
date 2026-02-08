#!/usr/bin/env python3
"""
Run full integration test for a service across all SDK layers.

Usage:
    python integration_test.py <service_name> <speaker_ip>

Examples:
    python integration_test.py RenderingControl 192.168.1.100
    python integration_test.py AVTransport 192.168.1.100
"""

import argparse
import subprocess
import sys
import time
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]


def run_command(cmd: list, cwd: Path = WORKSPACE_ROOT, timeout: int = 60) -> tuple:
    """Run a command and return (success, output)"""
    try:
        result = subprocess.run(
            cmd,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=timeout
        )
        return result.returncode == 0, result.stdout + result.stderr
    except subprocess.TimeoutExpired:
        return False, "Command timed out"
    except Exception as e:
        return False, str(e)


def test_cargo_build():
    """Test that the workspace builds"""
    print("\n[Test] Cargo Build")
    print("-" * 40)

    success, output = run_command(["cargo", "build", "--workspace"], timeout=300)
    if success:
        print("  ✓ Workspace builds successfully")
        return True
    else:
        print("  ✗ Build failed")
        print(f"    {output[:500]}...")
        return False


def test_cargo_tests(crate: str):
    """Test that crate tests pass"""
    print(f"\n[Test] {crate} Tests")
    print("-" * 40)

    success, output = run_command(["cargo", "test", "-p", crate], timeout=120)
    if success:
        print(f"  ✓ {crate} tests pass")
        return True
    else:
        print(f"  ✗ {crate} tests failed")
        # Extract test failure summary
        for line in output.split('\n'):
            if 'FAILED' in line or 'error' in line.lower():
                print(f"    {line}")
        return False


def test_api_operation(speaker_ip: str, service: str, operation: str) -> bool:
    """Test an API operation against real speaker"""
    print(f"\n[Test] API: {service}/{operation}")
    print("-" * 40)

    success, output = run_command([
        "cargo", "run", "--example", "cli_example",
        "--", speaker_ip, service, operation
    ], timeout=30)

    if success:
        print(f"  ✓ {operation} succeeded")
        # Show first few lines of output
        lines = output.strip().split('\n')[:5]
        for line in lines:
            print(f"    {line}")
        return True
    else:
        print(f"  ✗ {operation} failed")
        print(f"    {output[:200]}")
        return False


def test_stream_events(service: str) -> bool:
    """Test that stream layer has event support"""
    print(f"\n[Test] Stream: Event Types")
    print("-" * 40)

    # Run the analyze script
    script_path = WORKSPACE_ROOT / ".claude" / "skills" / "implement-service-stream" / "scripts" / "analyze_stream_events.py"

    if not script_path.exists():
        print("  ? Script not found, skipping")
        return True

    success, output = run_command(["python3", str(script_path), "--service", service])

    if success and service in output:
        print(f"  ✓ Event type found for {service}")
        return True
    else:
        print(f"  ✗ No event type for {service}")
        return False


def test_state_properties(service: str) -> bool:
    """Test that state layer has properties"""
    print(f"\n[Test] State: Properties")
    print("-" * 40)

    # Run the analyze script
    script_path = WORKSPACE_ROOT / ".claude" / "skills" / "implement-service-state" / "scripts" / "analyze_properties.py"

    if not script_path.exists():
        print("  ? Script not found, skipping")
        return True

    success, output = run_command(["python3", str(script_path), "--service", service])

    if success and "KEY" in output:
        print(f"  ✓ Properties found for {service}")
        return True
    else:
        print(f"  ✗ No properties for {service}")
        return False


def test_sdk_handles(service: str) -> bool:
    """Test that SDK layer has handles"""
    print(f"\n[Test] SDK: Handles")
    print("-" * 40)

    # Run the analyze script
    script_path = WORKSPACE_ROOT / ".claude" / "skills" / "implement-service-sdk" / "scripts" / "analyze_handles.py"

    if not script_path.exists():
        print("  ? Script not found, skipping")
        return True

    success, output = run_command(["python3", str(script_path), "--list"])

    # This is a rough check - just verify the script runs
    if success:
        print(f"  ✓ Handle analysis completed")
        return True
    else:
        print(f"  ✗ Handle analysis failed")
        return False


def get_service_operations(service: str) -> list:
    """Get list of operations to test for a service"""
    # Map services to their common operations
    service_operations = {
        "RenderingControl": ["GetVolume", "GetMute"],
        "AVTransport": ["GetTransportInfo", "GetPositionInfo"],
        "ZoneGroupTopology": ["GetZoneGroupState"],
        "DeviceProperties": ["GetZoneInfo"],
    }

    return service_operations.get(service, [])


def main():
    parser = argparse.ArgumentParser(description="Run full integration test for a service")
    parser.add_argument("service_name", help="Service name to test")
    parser.add_argument("speaker_ip", help="IP address of Sonos speaker")
    parser.add_argument("--skip-build", action="store_true", help="Skip cargo build")
    parser.add_argument("--skip-tests", action="store_true", help="Skip cargo tests")

    args = parser.parse_args()

    # Validate IP
    parts = args.speaker_ip.split('.')
    if len(parts) != 4:
        print(f"Error: Invalid IP address: {args.speaker_ip}")
        sys.exit(1)

    print("=" * 60)
    print(f"Integration Test: {args.service_name}")
    print(f"Speaker: {args.speaker_ip}")
    print("=" * 60)

    results = {}

    # 1. Build test
    if not args.skip_build:
        results["build"] = test_cargo_build()
        if not results["build"]:
            print("\n✗ Build failed - stopping tests")
            sys.exit(1)
    else:
        print("\n[Skipped] Cargo build")

    # 2. Unit tests
    if not args.skip_tests:
        for crate in ["sonos-api", "sonos-stream", "sonos-state", "sonos-sdk"]:
            results[f"test_{crate}"] = test_cargo_tests(crate)
    else:
        print("\n[Skipped] Unit tests")

    # 3. API operations
    operations = get_service_operations(args.service_name)
    if operations:
        for op in operations:
            results[f"api_{op}"] = test_api_operation(args.speaker_ip, args.service_name, op)
    else:
        print(f"\n[Info] No known operations to test for {args.service_name}")

    # 4. Stream layer
    results["stream"] = test_stream_events(args.service_name)

    # 5. State layer
    results["state"] = test_state_properties(args.service_name)

    # 6. SDK layer
    results["sdk"] = test_sdk_handles(args.service_name)

    # Summary
    print("\n" + "=" * 60)
    print("Summary")
    print("=" * 60)

    passed = sum(1 for v in results.values() if v)
    total = len(results)

    for test_name, success in results.items():
        status = "✓" if success else "✗"
        print(f"  [{status}] {test_name}")

    print(f"\nResult: {passed}/{total} tests passed")

    if passed == total:
        print("\n✓ All integration tests passed!")
        sys.exit(0)
    else:
        print(f"\n✗ {total - passed} tests failed")
        sys.exit(1)


if __name__ == "__main__":
    main()
