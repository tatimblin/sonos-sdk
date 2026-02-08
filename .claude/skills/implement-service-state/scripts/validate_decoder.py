#!/usr/bin/env python3
"""
Validate decoder with sample event JSON.

Usage:
    python validate_decoder.py sample_event.json
    python validate_decoder.py --generate RenderingControl
    python validate_decoder.py --test

Examples:
    # Generate sample event file
    python validate_decoder.py --generate RenderingControl > rc_event.json

    # Validate decoder output
    python validate_decoder.py rc_event.json
"""

import argparse
import json
import re
import sys
from pathlib import Path

# Find workspace root
SCRIPT_DIR = Path(__file__).parent
WORKSPACE_ROOT = SCRIPT_DIR.parents[3]

DECODER_FILE = WORKSPACE_ROOT / "sonos-state" / "src" / "decoder.rs"
STREAM_TYPES_FILE = WORKSPACE_ROOT / "sonos-stream" / "src" / "events" / "types.rs"


def parse_event_struct_fields(content: str, struct_name: str) -> list:
    """Parse fields from an event struct in types.rs"""
    fields = []

    # Find struct definition
    struct_pattern = rf'pub struct {struct_name}\s*\{{([^}}]+)\}}'
    match = re.search(struct_pattern, content, re.DOTALL)
    if not match:
        return fields

    body = match.group(1)

    # Parse fields
    field_pattern = r'pub (\w+):\s*([^,\n]+)'
    for field_match in re.finditer(field_pattern, body):
        field_name = field_match.group(1)
        field_type = field_match.group(2).strip().rstrip(',')
        fields.append((field_name, field_type))

    return fields


def get_event_struct_name(service_name: str) -> str:
    """Map service name to event struct name"""
    mapping = {
        "RenderingControl": "RenderingControlEvent",
        "AVTransport": "AVTransportEvent",
        "ZoneGroupTopology": "ZoneGroupTopologyEvent",
        "DeviceProperties": "DevicePropertiesEvent",
    }
    return mapping.get(service_name, f"{service_name}Event")


def generate_sample_event(service_name: str) -> dict:
    """Generate a sample event JSON for testing"""
    if not STREAM_TYPES_FILE.exists():
        print(f"Error: {STREAM_TYPES_FILE} not found", file=sys.stderr)
        sys.exit(1)

    content = STREAM_TYPES_FILE.read_text()
    struct_name = get_event_struct_name(service_name)
    fields = parse_event_struct_fields(content, struct_name)

    if not fields:
        print(f"Error: Could not find struct {struct_name}", file=sys.stderr)
        sys.exit(1)

    # Generate sample values based on field names
    event_data = {}
    for field_name, field_type in fields:
        if "Option<String>" in field_type:
            # Generate reasonable sample values based on field name
            if "volume" in field_name.lower():
                event_data[field_name] = "50"
            elif "mute" in field_name.lower():
                event_data[field_name] = "0"
            elif "bass" in field_name.lower():
                event_data[field_name] = "5"
            elif "treble" in field_name.lower():
                event_data[field_name] = "-3"
            elif "loudness" in field_name.lower():
                event_data[field_name] = "1"
            elif "state" in field_name.lower():
                event_data[field_name] = "PLAYING"
            elif "time" in field_name.lower():
                event_data[field_name] = "0:03:45"
            elif "duration" in field_name.lower():
                event_data[field_name] = "0:04:30"
            elif "uri" in field_name.lower():
                event_data[field_name] = "x-sonos-spotify:track123"
            elif "metadata" in field_name.lower():
                event_data[field_name] = '<DIDL-Lite><item><dc:title>Sample Song</dc:title><dc:creator>Sample Artist</dc:creator></item></DIDL-Lite>'
            else:
                event_data[field_name] = f"sample_{field_name}"
        elif "HashMap" in field_type:
            event_data[field_name] = {}

    return {
        "service": service_name,
        "event_type": struct_name,
        "event_data": event_data,
        "speaker_id": "RINCON_000000000001400"
    }


def parse_decoder_for_service(content: str, service_name: str) -> dict:
    """Parse decoder function to understand expected transformations"""
    decoder_name = service_name.lower().replace("_", "")

    # Find decoder function
    func_pattern = rf'fn decode_{decoder_name}\([^)]*\)[^{{]*\{{(.*?)\n\}}'
    # Use a simpler approach - find function start and match braces
    func_start = content.find(f"fn decode_{service_name.lower()}")
    if func_start == -1:
        # Try alternative naming
        for alt in ["rendering_control", "av_transport", "topology"]:
            func_start = content.find(f"fn decode_{alt}")
            if func_start != -1:
                break

    if func_start == -1:
        return {"fields": [], "error": f"Could not find decoder for {service_name}"}

    # Extract field parsing patterns
    field_patterns = []

    # Look for patterns like: if let Some(field_str) = &event.field_name
    field_pattern = r'if let Some\(\w+\) = &event\.(\w+)'
    for match in re.finditer(field_pattern, content[func_start:func_start+2000]):
        field_patterns.append(match.group(1))

    return {"fields": field_patterns}


def validate_event(event_file: Path):
    """Validate event JSON against decoder expectations"""
    if not event_file.exists():
        print(f"Error: {event_file} not found")
        sys.exit(1)

    if not DECODER_FILE.exists():
        print(f"Error: {DECODER_FILE} not found")
        sys.exit(1)

    with open(event_file) as f:
        event = json.load(f)

    decoder_content = DECODER_FILE.read_text()

    service = event.get("service", "Unknown")
    event_data = event.get("event_data", {})

    print(f"Validating {service} event")
    print("=" * 60)

    # Get decoder info
    decoder_info = parse_decoder_for_service(decoder_content, service)

    if "error" in decoder_info:
        print(f"Warning: {decoder_info['error']}")

    # Analyze each field
    print("\nField Analysis:")
    print("-" * 40)

    expected_changes = []

    for field_name, value in event_data.items():
        if value is None or value == {}:
            print(f"  {field_name}: <empty>")
            continue

        # Simulate parsing based on field type
        change = simulate_parse(field_name, value)
        if change:
            expected_changes.append(change)
            print(f"  {field_name}: \"{value}\" → {change}")
        else:
            print(f"  {field_name}: \"{value}\" → (not decoded)")

    print("\nExpected PropertyChanges:")
    print("-" * 40)
    if expected_changes:
        for change in expected_changes:
            print(f"  - {change}")
    else:
        print("  (no changes expected)")

    print("\n" + "=" * 60)
    print(f"Summary: {len(expected_changes)} expected property changes")


def simulate_parse(field_name: str, value: str) -> str:
    """Simulate what property change would be generated"""
    field_lower = field_name.lower()

    # Volume fields
    if "volume" in field_lower and "master" in field_lower:
        try:
            vol = min(int(value), 100)
            return f"PropertyChange::Volume(Volume({vol}))"
        except ValueError:
            return None

    # Mute fields
    if "mute" in field_lower and "master" in field_lower:
        muted = value == "1" or value.lower() == "true"
        return f"PropertyChange::Mute(Mute({str(muted).lower()}))"

    # Bass
    if field_name == "bass":
        try:
            bass = max(-10, min(10, int(value)))
            return f"PropertyChange::Bass(Bass({bass}))"
        except ValueError:
            return None

    # Treble
    if field_name == "treble":
        try:
            treble = max(-10, min(10, int(value)))
            return f"PropertyChange::Treble(Treble({treble}))"
        except ValueError:
            return None

    # Loudness
    if field_name == "loudness":
        enabled = value == "1" or value.lower() == "true"
        return f"PropertyChange::Loudness(Loudness({str(enabled).lower()}))"

    # Transport state
    if field_name == "transport_state":
        state_map = {
            "PLAYING": "Playing",
            "PAUSED_PLAYBACK": "Paused",
            "PAUSED": "Paused",
            "STOPPED": "Stopped",
        }
        state = state_map.get(value.upper(), "Transitioning")
        return f"PropertyChange::PlaybackState(PlaybackState::{state})"

    # Position (rel_time, track_duration)
    if field_name in ["rel_time", "track_duration"]:
        return f"PropertyChange::Position(...) (from {field_name})"

    # Current track URI
    if field_name == "current_track_uri":
        return "PropertyChange::CurrentTrack(...)"

    return None


def run_self_test():
    """Run self-test to verify script works"""
    print("Running self-test...")
    print("=" * 60)

    # Check files exist
    files_to_check = [
        ("Decoder file", DECODER_FILE),
        ("Stream types file", STREAM_TYPES_FILE),
    ]

    all_ok = True
    for name, path in files_to_check:
        if path.exists():
            print(f"  [OK] {name}: {path}")
        else:
            print(f"  [FAIL] {name}: {path} not found")
            all_ok = False

    # Try generating sample events
    print("\nSample event generation:")
    for service in ["RenderingControl", "AVTransport"]:
        try:
            event = generate_sample_event(service)
            print(f"  [OK] {service}: {len(event['event_data'])} fields")
        except Exception as e:
            print(f"  [FAIL] {service}: {e}")
            all_ok = False

    print("\n" + "=" * 60)
    if all_ok:
        print("Self-test passed!")
    else:
        print("Self-test failed - some checks did not pass")
        sys.exit(1)


def main():
    parser = argparse.ArgumentParser(description="Validate decoder with sample events")
    parser.add_argument("event_file", nargs="?", type=Path, help="JSON event file to validate")
    parser.add_argument("--generate", type=str, metavar="SERVICE",
                       help="Generate sample event JSON for SERVICE")
    parser.add_argument("--test", action="store_true", help="Run self-test")

    args = parser.parse_args()

    if args.test:
        run_self_test()
    elif args.generate:
        event = generate_sample_event(args.generate)
        print(json.dumps(event, indent=2))
    elif args.event_file:
        validate_event(args.event_file)
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
