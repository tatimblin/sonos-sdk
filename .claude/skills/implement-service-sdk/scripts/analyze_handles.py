#!/usr/bin/env python3
"""
Analyze sonos-sdk property handles.

Usage:
    python analyze_handles.py --list
    python analyze_handles.py --fetchable
    python analyze_handles.py --coverage
"""

import argparse
import re
import sys
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]

HANDLES_FILE = WORKSPACE_ROOT / "sonos-sdk" / "src" / "property" / "handles.rs"
SPEAKER_FILE = WORKSPACE_ROOT / "sonos-sdk" / "src" / "speaker.rs"
STATE_PROPERTY_FILE = WORKSPACE_ROOT / "sonos-state" / "src" / "property.rs"


def parse_type_aliases(content: str) -> list:
    """Parse type aliases from handles.rs"""
    aliases = []

    # Match: pub type XxxHandle = PropertyHandle<Xxx>;
    pattern = r'pub type (\w+Handle)\s*=\s*PropertyHandle<(\w+)>'
    for match in re.finditer(pattern, content):
        alias = match.group(1)
        property_type = match.group(2)
        aliases.append((alias, property_type))

    return aliases


def parse_fetchable_impls(content: str) -> list:
    """Parse Fetchable implementations from handles.rs"""
    fetchable = []

    # Match: impl Fetchable for Xxx {
    pattern = r'impl Fetchable for (\w+)\s*\{'
    for match in re.finditer(pattern, content):
        property_type = match.group(1)
        fetchable.append(property_type)

    return fetchable


def parse_speaker_fields(content: str) -> list:
    """Parse property handle fields from Speaker struct"""
    fields = []

    # Find Speaker struct
    struct_match = re.search(r'pub struct Speaker\s*\{([^}]+)\}', content, re.DOTALL)
    if not struct_match:
        return fields

    body = struct_match.group(1)

    # Match: pub field_name: TypeHandle,
    field_pattern = r'pub (\w+):\s*(\w+Handle)'
    for match in re.finditer(field_pattern, body):
        field_name = match.group(1)
        handle_type = match.group(2)
        fields.append((field_name, handle_type))

    return fields


def parse_state_properties(content: str) -> list:
    """Parse property types from sonos-state property.rs"""
    properties = []

    # Match: impl Property for Xxx {
    pattern = r'impl Property for (\w+)\s*\{'
    for match in re.finditer(pattern, content):
        property_type = match.group(1)
        properties.append(property_type)

    return properties


def list_handles():
    """List all property handles"""
    if not HANDLES_FILE.exists():
        print(f"Error: {HANDLES_FILE} not found")
        sys.exit(1)

    content = HANDLES_FILE.read_text()
    aliases = parse_type_aliases(content)
    fetchable = parse_fetchable_impls(content)

    print("=" * 60)
    print("SDK Property Handles")
    print("=" * 60)

    for alias, property_type in aliases:
        is_fetchable = property_type in fetchable
        fetch_status = "[fetch()]" if is_fetchable else "[no fetch]"
        print(f"  {alias:30} → {property_type:20} {fetch_status}")

    print("\n" + "=" * 60)
    print(f"Total: {len(aliases)} handles ({len(fetchable)} fetchable)")


def list_fetchable():
    """List only fetchable properties"""
    if not HANDLES_FILE.exists():
        print(f"Error: {HANDLES_FILE} not found")
        sys.exit(1)

    content = HANDLES_FILE.read_text()
    fetchable = parse_fetchable_impls(content)

    print("Fetchable Properties (have fetch() method)")
    print("=" * 60)

    for prop in fetchable:
        print(f"  - {prop}")

    print("\n" + "=" * 60)
    print(f"Total: {len(fetchable)} fetchable properties")


def check_coverage():
    """Check coverage between state properties and SDK handles"""
    files_exist = all(f.exists() for f in [HANDLES_FILE, SPEAKER_FILE, STATE_PROPERTY_FILE])
    if not files_exist:
        print("Error: Required files not found")
        sys.exit(1)

    handles_content = HANDLES_FILE.read_text()
    speaker_content = SPEAKER_FILE.read_text()
    state_content = STATE_PROPERTY_FILE.read_text()

    aliases = parse_type_aliases(handles_content)
    fetchable = parse_fetchable_impls(handles_content)
    speaker_fields = parse_speaker_fields(speaker_content)
    state_properties = parse_state_properties(state_content)

    # Build lookup sets
    alias_properties = set(prop for _, prop in aliases)
    speaker_field_handles = set(handle for _, handle in speaker_fields)
    alias_names = set(alias for alias, _ in aliases)

    print("Property Handle Coverage Analysis")
    print("=" * 70)

    # 1. State properties → SDK handles
    print("\n1. State Properties → SDK Handles")
    print("-" * 50)

    for prop in sorted(state_properties):
        has_alias = prop in alias_properties
        expected_alias = f"{prop}Handle"
        has_speaker_field = expected_alias in speaker_field_handles
        is_fetchable = prop in fetchable

        if has_alias and has_speaker_field:
            status = "OK"
            extra = " [fetchable]" if is_fetchable else ""
        elif has_alias:
            status = "PARTIAL"
            extra = " (no Speaker field)"
        else:
            status = "MISSING"
            extra = ""

        print(f"  [{status:7}] {prop:25} {extra}")

    # 2. Speaker fields
    print("\n2. Speaker Fields")
    print("-" * 50)

    for field_name, handle_type in speaker_fields:
        if handle_type in alias_names:
            print(f"  [OK] speaker.{field_name}: {handle_type}")
        else:
            print(f"  [MISSING ALIAS] speaker.{field_name}: {handle_type}")

    # Summary
    print("\n" + "=" * 70)
    print("Summary:")
    print(f"  State properties: {len(state_properties)}")
    print(f"  Type aliases: {len(aliases)}")
    print(f"  Fetchable impls: {len(fetchable)}")
    print(f"  Speaker fields: {len(speaker_fields)}")

    # Find missing
    missing_handles = set(state_properties) - alias_properties
    if missing_handles:
        print(f"\nState properties without SDK handles:")
        for prop in sorted(missing_handles):
            print(f"  - {prop}")


def main():
    parser = argparse.ArgumentParser(description="Analyze sonos-sdk property handles")
    parser.add_argument("--list", action="store_true", help="List all property handles")
    parser.add_argument("--fetchable", action="store_true", help="List only fetchable properties")
    parser.add_argument("--coverage", action="store_true", help="Check state → SDK coverage")

    args = parser.parse_args()

    if args.list:
        list_handles()
    elif args.fetchable:
        list_fetchable()
    elif args.coverage:
        check_coverage()
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
