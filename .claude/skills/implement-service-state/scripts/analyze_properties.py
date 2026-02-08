#!/usr/bin/env python3
"""
Analyze sonos-state properties.

Usage:
    python analyze_properties.py --list
    python analyze_properties.py --service RenderingControl
    python analyze_properties.py --coverage
"""

import argparse
import re
import sys
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]  # Up from .claude/skills/implement-service-state/scripts

PROPERTY_FILE = WORKSPACE_ROOT / "sonos-state" / "src" / "property.rs"
DECODER_FILE = WORKSPACE_ROOT / "sonos-state" / "src" / "decoder.rs"
STREAM_TYPES_FILE = WORKSPACE_ROOT / "sonos-stream" / "src" / "events" / "types.rs"


def parse_property_structs(content: str) -> dict:
    """Parse property struct definitions from property.rs"""
    properties = {}

    # Match property definitions with their traits
    # Look for impl Property for X { const KEY ... }
    property_pattern = r'impl Property for (\w+)\s*\{[^}]*const KEY:\s*&\'static str\s*=\s*"([^"]+)"'
    for match in re.finditer(property_pattern, content, re.DOTALL):
        name = match.group(1)
        key = match.group(2)
        properties[name] = {"key": key, "scope": None, "service": None}

    # Now find SonosProperty impls to get scope and service
    sonos_pattern = r'impl SonosProperty for (\w+)\s*\{([^}]+)\}'
    for match in re.finditer(sonos_pattern, content, re.DOTALL):
        name = match.group(1)
        body = match.group(2)

        if name in properties:
            # Extract SCOPE
            scope_match = re.search(r'const SCOPE:\s*Scope\s*=\s*Scope::(\w+)', body)
            if scope_match:
                properties[name]["scope"] = scope_match.group(1)

            # Extract SERVICE
            service_match = re.search(r'const SERVICE:\s*Service\s*=\s*Service::(\w+)', body)
            if service_match:
                properties[name]["service"] = service_match.group(1)

    return properties


def parse_property_change_enum(content: str) -> list:
    """Parse PropertyChange enum variants from decoder.rs"""
    variants = []

    # Find PropertyChange enum
    enum_match = re.search(r'pub enum PropertyChange\s*\{([^}]+)\}', content, re.DOTALL)
    if enum_match:
        enum_body = enum_match.group(1)
        # Match variants like Volume(Volume), Mute(Mute)
        variant_pattern = r'(\w+)\((\w+)\)'
        for match in re.finditer(variant_pattern, enum_body):
            variants.append((match.group(1), match.group(2)))

    return variants


def parse_decode_event_arms(content: str) -> dict:
    """Parse decode_event match arms from decoder.rs"""
    mapping = {}

    # Find decode_event function and extract match arms
    match_pattern = r'EventData::(\w+)\([^)]*\)\s*=>\s*decode_(\w+)\('
    for match in re.finditer(match_pattern, content):
        event_variant = match.group(1)
        decoder_name = match.group(2)
        mapping[event_variant] = decoder_name

    # Also check for direct vec![] returns
    direct_pattern = r'EventData::(\w+)\([^)]*\)\s*=>\s*vec!\[\]'
    for match in re.finditer(direct_pattern, content):
        event_variant = match.group(1)
        mapping[event_variant] = "empty"

    return mapping


def parse_stream_event_data(content: str) -> list:
    """Parse EventData variants from sonos-stream types.rs"""
    variants = []

    enum_match = re.search(r'pub enum EventData\s*\{([^}]+)\}', content, re.DOTALL)
    if enum_match:
        enum_body = enum_match.group(1)
        variant_pattern = r'(\w+)\((\w+)\)'
        for match in re.finditer(variant_pattern, enum_body):
            variants.append((match.group(1), match.group(2)))

    return variants


def list_properties():
    """List all property types"""
    if not PROPERTY_FILE.exists():
        print(f"Error: {PROPERTY_FILE} not found")
        sys.exit(1)

    content = PROPERTY_FILE.read_text()
    properties = parse_property_structs(content)

    print("=" * 70)
    print("Sonos State Properties")
    print("=" * 70)

    # Group by service
    by_service = {}
    for name, info in properties.items():
        service = info.get("service", "Unknown")
        if service not in by_service:
            by_service[service] = []
        by_service[service].append((name, info))

    for service in sorted(by_service.keys()):
        print(f"\n{service}")
        print("-" * 40)
        for name, info in sorted(by_service[service]):
            scope = info.get("scope", "?")
            key = info.get("key", "?")
            print(f"  {name:25} key={key:20} scope={scope}")

    print("\n" + "=" * 70)
    print(f"Total: {len(properties)} properties")


def show_service(service_name: str):
    """Show properties for a specific service"""
    if not PROPERTY_FILE.exists():
        print(f"Error: {PROPERTY_FILE} not found")
        sys.exit(1)

    content = PROPERTY_FILE.read_text()
    properties = parse_property_structs(content)

    # Filter by service
    matching = {
        name: info for name, info in properties.items()
        if info.get("service", "").lower() == service_name.lower()
    }

    if not matching:
        print(f"No properties found for service '{service_name}'")
        print(f"Available services: {', '.join(set(p.get('service', 'Unknown') for p in properties.values()))}")
        sys.exit(1)

    print(f"Properties for {service_name}")
    print("=" * 60)

    for name, info in sorted(matching.items()):
        print(f"\n{name}")
        print(f"  KEY: \"{info['key']}\"")
        print(f"  SCOPE: Scope::{info['scope']}")
        print(f"  SERVICE: Service::{info['service']}")


def check_coverage():
    """Check decoder coverage for stream events"""
    if not all(f.exists() for f in [PROPERTY_FILE, DECODER_FILE, STREAM_TYPES_FILE]):
        print("Error: Required files not found")
        sys.exit(1)

    property_content = PROPERTY_FILE.read_text()
    decoder_content = DECODER_FILE.read_text()
    stream_content = STREAM_TYPES_FILE.read_text()

    properties = parse_property_structs(property_content)
    property_changes = parse_property_change_enum(decoder_content)
    decode_arms = parse_decode_event_arms(decoder_content)
    stream_events = parse_stream_event_data(stream_content)

    print("Property Coverage Analysis")
    print("=" * 70)

    # Check which properties have PropertyChange variants
    print("\n1. Properties → PropertyChange Variants")
    print("-" * 50)
    property_names = set(properties.keys())
    change_types = set(t for _, t in property_changes)

    covered = property_names & change_types
    missing = property_names - change_types

    for prop in sorted(covered):
        print(f"  [OK] {prop}")
    for prop in sorted(missing):
        print(f"  [MISSING] {prop} - no PropertyChange variant")

    # Check which stream events have decoders
    print("\n2. Stream Events → Decoders")
    print("-" * 50)

    for event_variant, _struct_type in stream_events:
        if event_variant in decode_arms:
            decoder = decode_arms[event_variant]
            status = "OK" if decoder != "empty" else "EMPTY"
            print(f"  [{status}] {event_variant} → decode_{decoder}()")
        else:
            print(f"  [MISSING] {event_variant} - no decoder arm")

    # Summary
    print("\n" + "=" * 70)
    print("Summary:")
    print(f"  Properties defined: {len(properties)}")
    print(f"  PropertyChange variants: {len(property_changes)}")
    print(f"  Stream event types: {len(stream_events)}")
    print(f"  Decode arms: {len(decode_arms)}")

    if missing:
        print(f"\nWarning: {len(missing)} properties without PropertyChange variants:")
        for prop in sorted(missing):
            print(f"  - {prop}")


def main():
    parser = argparse.ArgumentParser(description="Analyze sonos-state properties")
    parser.add_argument("--list", action="store_true", help="List all properties")
    parser.add_argument("--service", type=str, help="Show properties for a specific service")
    parser.add_argument("--coverage", action="store_true", help="Check decoder coverage")

    args = parser.parse_args()

    if args.list:
        list_properties()
    elif args.service:
        show_service(args.service)
    elif args.coverage:
        check_coverage()
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
