#!/usr/bin/env python3
"""
Analyze sonos-stream event types.

Usage:
    python analyze_stream_events.py --list
    python analyze_stream_events.py --service AVTransport
    python analyze_stream_events.py --validate
"""

import argparse
import re
import sys
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]  # Up from .claude/skills/implement-service-stream/scripts

TYPES_FILE = WORKSPACE_ROOT / "sonos-stream" / "src" / "events" / "types.rs"
PROCESSOR_FILE = WORKSPACE_ROOT / "sonos-stream" / "src" / "events" / "processor.rs"


def parse_event_structs(content: str) -> dict:
    """Parse event struct definitions from types.rs"""
    structs = {}

    # Match struct definitions
    struct_pattern = r'pub struct (\w+Event)\s*\{([^}]+)\}'
    matches = re.findall(struct_pattern, content, re.DOTALL)

    for name, body in matches:
        fields = []
        field_pattern = r'pub (\w+):\s*([\w<>,\s:]+)'
        for field_match in re.finditer(field_pattern, body):
            field_name = field_match.group(1)
            field_type = field_match.group(2).strip().rstrip(',')
            fields.append((field_name, field_type))
        structs[name] = fields

    return structs


def parse_event_data_enum(content: str) -> list:
    """Parse EventData enum variants"""
    variants = []

    # Find EventData enum
    enum_match = re.search(r'pub enum EventData\s*\{([^}]+)\}', content, re.DOTALL)
    if enum_match:
        enum_body = enum_match.group(1)
        variant_pattern = r'(\w+)\((\w+)\)'
        for match in re.finditer(variant_pattern, enum_body):
            variants.append((match.group(1), match.group(2)))

    return variants


def parse_service_type_mapping(content: str) -> dict:
    """Parse service_type() match arms"""
    mapping = {}

    # Find service_type implementation
    impl_match = re.search(
        r'fn service_type\(&self\)[^{]*\{([^}]+match self[^}]+\})',
        content, re.DOTALL
    )
    if impl_match:
        impl_body = impl_match.group(1)
        arm_pattern = r'EventData::(\w+)\(_\)\s*=>\s*\{?\s*sonos_api::Service::(\w+)'
        for match in re.finditer(arm_pattern, impl_body):
            mapping[match.group(1)] = match.group(2)

    return mapping


def list_events():
    """List all event types"""
    if not TYPES_FILE.exists():
        print(f"Error: {TYPES_FILE} not found")
        sys.exit(1)

    content = TYPES_FILE.read_text()
    structs = parse_event_structs(content)
    variants = parse_event_data_enum(content)
    mapping = parse_service_type_mapping(content)

    print("=" * 60)
    print("EventData Variants")
    print("=" * 60)

    for variant, struct_type in variants:
        service = mapping.get(variant, "UNKNOWN")
        print(f"\n{variant}")
        print(f"  Struct: {struct_type}")
        print(f"  Service: {service}")

        if struct_type in structs:
            print(f"  Fields ({len(structs[struct_type])}):")
            for field_name, field_type in structs[struct_type]:
                print(f"    - {field_name}: {field_type}")

    print("\n" + "=" * 60)
    print(f"Total: {len(variants)} event types")


def show_service(service_name: str):
    """Show details for a specific service"""
    if not TYPES_FILE.exists():
        print(f"Error: {TYPES_FILE} not found")
        sys.exit(1)

    content = TYPES_FILE.read_text()
    structs = parse_event_structs(content)
    mapping = parse_service_type_mapping(content)

    # Find matching event
    target_variant = None
    for variant, service in mapping.items():
        if service.lower() == service_name.lower() or service_name.lower() in variant.lower():
            target_variant = variant
            break

    if not target_variant:
        print(f"Error: No event found for service '{service_name}'")
        print(f"Available services: {', '.join(set(mapping.values()))}")
        sys.exit(1)

    struct_name = target_variant.replace("Event", "") + "Event"
    if struct_name not in structs:
        # Try direct match
        for s in structs:
            if target_variant in s or s in target_variant:
                struct_name = s
                break

    print(f"Service: {mapping.get(target_variant, 'UNKNOWN')}")
    print(f"Event Variant: EventData::{target_variant}")
    print(f"Event Struct: {struct_name}")
    print()

    if struct_name in structs:
        print("Fields:")
        for field_name, field_type in structs[struct_name]:
            print(f"  pub {field_name}: {field_type}")
    else:
        print(f"Warning: Struct {struct_name} not found in types.rs")


def validate_coverage():
    """Validate that all EventData variants have processor support"""
    if not TYPES_FILE.exists() or not PROCESSOR_FILE.exists():
        print("Error: Required files not found")
        sys.exit(1)

    types_content = TYPES_FILE.read_text()
    processor_content = PROCESSOR_FILE.read_text()

    variants = parse_event_data_enum(types_content)
    mapping = parse_service_type_mapping(types_content)

    print("Event Type Coverage")
    print("=" * 60)

    issues = []
    for variant, struct_type in variants:
        service = mapping.get(variant, "UNKNOWN")

        # Check if processor handles this service
        has_processor = f"Service::{service}" in processor_content or \
                       f"sonos_api::Service::{service}" in processor_content

        status = "OK" if has_processor else "MISSING"
        if not has_processor:
            issues.append(variant)

        print(f"{variant:40} {service:25} [{status}]")

    print()
    if issues:
        print(f"WARNING: {len(issues)} event types missing processor support:")
        for issue in issues:
            print(f"  - {issue}")
    else:
        print("All event types have processor support!")


def main():
    parser = argparse.ArgumentParser(description="Analyze sonos-stream event types")
    parser.add_argument("--list", action="store_true", help="List all event types")
    parser.add_argument("--service", type=str, help="Show details for a specific service")
    parser.add_argument("--validate", action="store_true", help="Validate event coverage")

    args = parser.parse_args()

    if args.list:
        list_events()
    elif args.service:
        show_service(args.service)
    elif args.validate:
        validate_coverage()
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
