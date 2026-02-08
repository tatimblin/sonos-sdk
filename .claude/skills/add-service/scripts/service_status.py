#!/usr/bin/env python3
"""
Check implementation status of a service across all SDK layers.

Usage:
    python service_status.py <service_name>
    python service_status.py --all

Examples:
    python service_status.py RenderingControl
    python service_status.py AVTransport
    python service_status.py --all
"""

import argparse
import re
import sys
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]


def camel_to_snake(name: str) -> str:
    """Convert CamelCase to snake_case"""
    # Insert underscore before uppercase letters (except at start)
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', s1).lower()

# File paths for each layer
FILES = {
    "api": {
        "services_mod": WORKSPACE_ROOT / "sonos-api" / "src" / "services" / "mod.rs",
        "service_enum": WORKSPACE_ROOT / "sonos-api" / "src" / "lib.rs",
    },
    "stream": {
        "types": WORKSPACE_ROOT / "sonos-stream" / "src" / "events" / "types.rs",
        "processor": WORKSPACE_ROOT / "sonos-stream" / "src" / "events" / "processor.rs",
        "strategies": WORKSPACE_ROOT / "sonos-stream" / "src" / "polling" / "strategies.rs",
    },
    "state": {
        "property": WORKSPACE_ROOT / "sonos-state" / "src" / "property.rs",
        "decoder": WORKSPACE_ROOT / "sonos-state" / "src" / "decoder.rs",
    },
    "sdk": {
        "handles": WORKSPACE_ROOT / "sonos-sdk" / "src" / "property" / "handles.rs",
        "speaker": WORKSPACE_ROOT / "sonos-sdk" / "src" / "speaker.rs",
    },
}


def check_api_layer(service_name: str) -> dict:
    """Check API layer implementation status"""
    status = {
        "service_module": False,
        "service_enum": False,
        "operations": [],
    }

    # Check service module exists (convert CamelCase to snake_case)
    service_dir = WORKSPACE_ROOT / "sonos-api" / "src" / "services" / camel_to_snake(service_name)
    if service_dir.exists():
        status["service_module"] = True

        # List operation files
        ops_file = service_dir / "operations.rs"
        if ops_file.exists():
            content = ops_file.read_text()
            # Find operation structs (from both macro-generated and manually defined)
            # Look for pub struct XxxOperation patterns
            pattern = r'pub struct (\w+Operation)\b'
            ops = re.findall(pattern, content)
            # Also look for operation definitions in macros
            # define_upnp_operation! { operation: XxxOperation, ...
            macro_pattern = r'operation:\s*(\w+Operation)'
            ops.extend(re.findall(macro_pattern, content))
            # Deduplicate while preserving order
            seen = set()
            status["operations"] = [x for x in ops if not (x in seen or seen.add(x))]

    # Check Service enum
    if FILES["api"]["service_enum"].exists():
        content = FILES["api"]["service_enum"].read_text()
        # Look for service in Service enum
        if f"Service::{service_name}" in content or service_name in content:
            status["service_enum"] = True

    return status


def check_stream_layer(service_name: str) -> dict:
    """Check stream layer implementation status"""
    status = {
        "event_struct": False,
        "event_data_variant": False,
        "processor_case": False,
        "poller": False,
    }

    # Check event types
    if FILES["stream"]["types"].exists():
        content = FILES["stream"]["types"].read_text()

        # Check for event struct
        event_struct_name = f"{service_name}Event"
        if f"pub struct {event_struct_name}" in content:
            status["event_struct"] = True

        # Check for EventData variant
        if f"EventData::{event_struct_name}" in content or f"{event_struct_name}(" in content:
            status["event_data_variant"] = True

    # Check processor
    if FILES["stream"]["processor"].exists():
        content = FILES["stream"]["processor"].read_text()
        if f"Service::{service_name}" in content:
            status["processor_case"] = True

    # Check polling strategies
    if FILES["stream"]["strategies"].exists():
        content = FILES["stream"]["strategies"].read_text()
        poller_name = f"{service_name}Poller"
        if f"struct {poller_name}" in content or f"pub struct {poller_name}" in content:
            status["poller"] = True

    return status


def check_state_layer(service_name: str) -> dict:
    """Check state layer implementation status"""
    status = {
        "properties": [],
        "property_changes": [],
        "decoder": False,
    }

    # Check properties
    if FILES["state"]["property"].exists():
        content = FILES["state"]["property"].read_text()

        # Find properties for this service
        pattern = rf'impl SonosProperty for (\w+)\s*\{{[^}}]*Service::{service_name}'
        status["properties"] = re.findall(pattern, content, re.DOTALL)

    # Check decoder
    if FILES["state"]["decoder"].exists():
        content = FILES["state"]["decoder"].read_text()

        # Check for PropertyChange variants from this service
        for prop in status["properties"]:
            if f"PropertyChange::{prop}" in content:
                status["property_changes"].append(prop)

        # Check for decoder function (convert CamelCase to snake_case)
        decoder_name = f"decode_{camel_to_snake(service_name)}"
        if f"fn {decoder_name}" in content:
            status["decoder"] = True

    return status


def check_sdk_layer(service_name: str) -> dict:
    """Check SDK layer implementation status"""
    status = {
        "handles": [],
        "fetchable": [],
        "speaker_fields": [],
    }

    # Check handles
    if FILES["sdk"]["handles"].exists():
        content = FILES["sdk"]["handles"].read_text()

        # Find type aliases
        pattern = r'pub type (\w+Handle)\s*=\s*PropertyHandle<(\w+)>'
        for alias, prop in re.findall(pattern, content):
            status["handles"].append((alias, prop))

        # Find Fetchable implementations
        pattern = r'impl Fetchable for (\w+)'
        status["fetchable"] = re.findall(pattern, content)

    # Check speaker fields
    if FILES["sdk"]["speaker"].exists():
        content = FILES["sdk"]["speaker"].read_text()

        pattern = r'pub (\w+):\s*(\w+Handle)'
        for field, handle in re.findall(pattern, content):
            status["speaker_fields"].append((field, handle))

    return status


def print_status(service_name: str):
    """Print comprehensive status for a service"""
    print(f"Service Implementation Status: {service_name}")
    print("=" * 70)

    # Layer 1: API
    print("\n[Layer 1] sonos-api")
    print("-" * 40)
    api = check_api_layer(service_name)
    print(f"  Service module: {'✓' if api['service_module'] else '✗'}")
    print(f"  Service enum:   {'✓' if api['service_enum'] else '✗'}")
    if api["operations"]:
        print(f"  Operations ({len(api['operations'])}):")
        for op in api["operations"]:
            print(f"    - {op}")
    else:
        print("  Operations: (none found)")

    # Layer 2: Stream
    print("\n[Layer 2] sonos-stream")
    print("-" * 40)
    stream = check_stream_layer(service_name)
    print(f"  Event struct:      {'✓' if stream['event_struct'] else '✗'}")
    print(f"  EventData variant: {'✓' if stream['event_data_variant'] else '✗'}")
    print(f"  Processor case:    {'✓' if stream['processor_case'] else '✗'}")
    print(f"  Poller impl:       {'✓' if stream['poller'] else '✗'}")

    # Layer 3: State
    print("\n[Layer 3] sonos-state")
    print("-" * 40)
    state = check_state_layer(service_name)
    if state["properties"]:
        print(f"  Properties ({len(state['properties'])}):")
        for prop in state["properties"]:
            has_change = prop in state["property_changes"]
            marker = "✓" if has_change else "✗"
            print(f"    [{marker}] {prop}")
    else:
        print("  Properties: (none found)")
    print(f"  Decoder function: {'✓' if state['decoder'] else '✗'}")

    # Layer 4: SDK
    print("\n[Layer 4] sonos-sdk")
    print("-" * 40)
    sdk = check_sdk_layer(service_name)

    # Filter to properties from this service
    service_properties = set(state["properties"])
    relevant_handles = [(a, p) for a, p in sdk["handles"] if p in service_properties]

    if relevant_handles:
        print(f"  Handles ({len(relevant_handles)}):")
        for alias, prop in relevant_handles:
            is_fetchable = prop in sdk["fetchable"]
            speaker_field = any(h == alias for _, h in sdk["speaker_fields"])
            fetch_marker = "[fetch]" if is_fetchable else "[no fetch]"
            field_marker = "✓" if speaker_field else "✗"
            print(f"    [{field_marker}] {alias} → {prop} {fetch_marker}")
    else:
        print("  Handles: (none found for this service)")

    # Summary
    print("\n" + "=" * 70)
    print("Summary:")

    all_good = (
        api["service_module"] and
        stream["event_struct"] and
        stream["event_data_variant"] and
        len(state["properties"]) > 0 and
        state["decoder"] and
        len(relevant_handles) > 0
    )

    if all_good:
        print("  ✓ Service appears fully implemented across all layers")
    else:
        print("  ✗ Service implementation is incomplete")
        if not api["service_module"]:
            print("    → Missing: API service module")
        if not stream["event_struct"]:
            print("    → Missing: Stream event struct")
        if not stream["event_data_variant"]:
            print("    → Missing: EventData variant")
        if not state["properties"]:
            print("    → Missing: State properties")
        if not state["decoder"]:
            print("    → Missing: State decoder")
        if not relevant_handles:
            print("    → Missing: SDK handles")


def list_all_services():
    """List all known services and their implementation status"""
    print("All Services Implementation Status")
    print("=" * 70)

    # Known services from Sonos API
    known_services = [
        "AVTransport",
        "RenderingControl",
        "DeviceProperties",
        "ZoneGroupTopology",
        "GroupRenderingControl",
        "ContentDirectory",
        "Queue",
        "AlarmClock",
    ]

    # Also check for any service directories that exist
    services_dir = WORKSPACE_ROOT / "sonos-api" / "src" / "services"
    if services_dir.exists():
        for item in services_dir.iterdir():
            if item.is_dir() and item.name not in ["mod.rs", "__pycache__", "events"]:
                # Convert snake_case directory name to CamelCase service name
                # e.g., "av_transport" -> "AVTransport", "zone_group_topology" -> "ZoneGroupTopology"
                parts = item.name.split("_")
                service_name = "".join(part.capitalize() for part in parts)
                # Handle special cases like "AV" which should be uppercase
                service_name = service_name.replace("Av", "AV")
                if service_name not in known_services:
                    known_services.append(service_name)

    print(f"\n{'Service':<25} {'API':^6} {'Stream':^8} {'State':^7} {'SDK':^5}")
    print("-" * 55)

    for service in sorted(known_services):
        api = check_api_layer(service)
        stream = check_stream_layer(service)
        state = check_state_layer(service)
        sdk = check_sdk_layer(service)

        api_status = "✓" if api["service_module"] else "✗"
        stream_status = "✓" if stream["event_struct"] else "✗"
        state_status = "✓" if state["properties"] else "✗"

        # Check SDK has handles for this service's properties
        service_props = set(state["properties"])
        sdk_handles = [p for _, p in sdk["handles"] if p in service_props]
        sdk_status = "✓" if sdk_handles else "✗"

        print(f"{service:<25} {api_status:^6} {stream_status:^8} {state_status:^7} {sdk_status:^5}")


def main():
    parser = argparse.ArgumentParser(description="Check service implementation status")
    parser.add_argument("service_name", nargs="?", help="Service name to check")
    parser.add_argument("--all", action="store_true", help="List all services status")

    args = parser.parse_args()

    if args.all:
        list_all_services()
    elif args.service_name:
        print_status(args.service_name)
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
