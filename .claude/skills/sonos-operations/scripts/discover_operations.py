#!/usr/bin/env python3
"""
Discovers all SonosOperation implementations in the sonos-api codebase.
Parses Rust source code to extract operation names, services, and required parameters.

This script provides complete operation metadata for dynamic execution:
- Operation name, action, and service
- Request field names and types
- Service endpoint and URI for direct SOAP calls
"""

import os
import re
import json
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# Service metadata for direct SOAP execution
# Maps service names to their UPnP endpoint and service URI
SERVICE_INFO = {
    'AVTransport': {
        'endpoint': 'MediaRenderer/AVTransport/Control',
        'service_uri': 'urn:schemas-upnp-org:service:AVTransport:1',
        'event_endpoint': 'MediaRenderer/AVTransport/Event',
    },
    'RenderingControl': {
        'endpoint': 'MediaRenderer/RenderingControl/Control',
        'service_uri': 'urn:schemas-upnp-org:service:RenderingControl:1',
        'event_endpoint': 'MediaRenderer/RenderingControl/Event',
    },
    'GroupRenderingControl': {
        'endpoint': 'MediaRenderer/GroupRenderingControl/Control',
        'service_uri': 'urn:schemas-upnp-org:service:GroupRenderingControl:1',
        'event_endpoint': 'MediaRenderer/GroupRenderingControl/Event',
    },
    'ZoneGroupTopology': {
        'endpoint': 'ZoneGroupTopology/Control',
        'service_uri': 'urn:schemas-upnp-org:service:ZoneGroupTopology:1',
        'event_endpoint': 'ZoneGroupTopology/Event',
    },
}


def find_operations_in_file(file_path: str) -> List[Dict]:
    """Extract operation information from a Rust source file."""
    operations = []

    with open(file_path, 'r') as f:
        content = f.read()

    # Find both define_upnp_operation! and define_operation_with_response! macro usages
    def extract_macro_content(pattern, content):
        matches = []
        for match in re.finditer(pattern, content):
            start = match.end()  # Position after the opening brace
            brace_count = 1
            pos = start

            while pos < len(content) and brace_count > 0:
                if content[pos] == '{':
                    brace_count += 1
                elif content[pos] == '}':
                    brace_count -= 1
                pos += 1

            if brace_count == 0:
                macro_content = content[start:pos-1]  # Exclude the final closing brace
                matches.append(macro_content)

        return matches

    macro_matches = []
    macro_matches.extend(extract_macro_content(r'define_upnp_operation!\s*\{', content))
    macro_matches.extend(extract_macro_content(r'define_operation_with_response!\s*\{', content))

    for macro_content in macro_matches:
        operation_info = {
            'name': None,
            'file': file_path,
            'service': None,
            'action': None,
            'request_type': None,
            'response_type': None,
            'request_fields': [],
            'service_info': None,  # Will be populated with endpoint/uri
        }

        # Extract operation name
        op_match = re.search(r'operation:\s*(\w+)', macro_content)
        if op_match:
            operation_info['name'] = op_match.group(1)
            operation_info['request_type'] = f"{op_match.group(1)}Request"

        # Extract action
        action_match = re.search(r'action:\s*"([^"]*)"', macro_content)
        if action_match:
            operation_info['action'] = action_match.group(1)

        # Extract service
        service_match = re.search(r'service:\s*(\w+)', macro_content)
        if service_match:
            service_name = service_match.group(1)
            operation_info['service'] = service_name
            # Add service info for direct SOAP calls
            if service_name in SERVICE_INFO:
                operation_info['service_info'] = SERVICE_INFO[service_name]

        # Extract request fields from the request: {...} block
        request_match = re.search(r'request:\s*\{(.*?)\}', macro_content, re.DOTALL)
        if request_match:
            request_content = request_match.group(1).strip()
            if request_content:  # Only parse if not empty
                # Parse field definitions: field_name: Type,
                field_pattern = r'(\w+):\s*([^,\n}]+)'
                field_matches = re.findall(field_pattern, request_content)

                for field_name, field_type in field_matches:
                    field_type = field_type.strip().rstrip(',')
                    operation_info['request_fields'].append({
                        'name': field_name,
                        'type': field_type,
                        'required': not field_type.startswith('Option<'),
                        'xml_name': to_pascal_case(field_name),  # XML tag name
                    })

        # Always add instance_id as it's implicitly added by the macro
        if operation_info['name']:
            operation_info['request_fields'].insert(0, {
                'name': 'instance_id',
                'type': 'u32',
                'required': True,
                'xml_name': 'InstanceID',
            })

        if operation_info['name']:
            operations.append(operation_info)

    return operations


def to_pascal_case(snake_case: str) -> str:
    """Convert snake_case to PascalCase for XML tag names."""
    # Handle special cases
    special_cases = {
        'instance_id': 'InstanceID',
        'current_uri': 'CurrentURI',
        'current_uri_meta_data': 'CurrentURIMetaData',
        'next_uri': 'NextURI',
        'next_uri_meta_data': 'NextURIMetaData',
        'enqueued_uri': 'EnqueuedURI',
        'enqueued_uri_meta_data': 'EnqueuedURIMetaData',
        'object_id': 'ObjectID',
        'update_id': 'UpdateID',
        'alarm_id': 'AlarmID',
        'group_id': 'GroupID',
    }

    if snake_case in special_cases:
        return special_cases[snake_case]

    # Default conversion: split by underscore, capitalize each part
    parts = snake_case.split('_')
    return ''.join(part.capitalize() for part in parts)


def find_struct_fields(content: str, struct_name: str) -> List[Dict]:
    """Extract fields from a struct definition."""
    fields = []

    # Find struct definition
    struct_pattern = rf'#\[derive.*?\]\s*pub\s+struct\s+{struct_name}\s*{{(.*?)}}'
    struct_match = re.search(struct_pattern, content, re.DOTALL)

    if struct_match:
        struct_content = struct_match.group(1)

        # Extract fields: pub field_name: Type,
        field_pattern = r'pub\s+(\w+):\s*([^,\n]+)'
        field_matches = re.findall(field_pattern, struct_content)

        for field_name, field_type in field_matches:
            field_type = field_type.strip()
            fields.append({
                'name': field_name,
                'type': field_type,
                'required': not field_type.startswith('Option<'),
                'xml_name': to_pascal_case(field_name),
            })

    return fields


def discover_all_operations(api_path: str = "sonos-api/src") -> Dict:
    """Discover all operations in the sonos-api codebase."""
    all_operations = []
    services = {}
    seen_operations = set()  # Track seen operation names to avoid duplicates

    # Walk through all Rust files in the API
    api_dir = Path(api_path)
    if not api_dir.exists():
        raise FileNotFoundError(f"API directory not found: {api_path}")

    # Files to skip (contain example/test definitions, not actual operations)
    skip_files = {'macros.rs', 'tests.rs'}

    for rust_file in api_dir.rglob("*.rs"):
        # Skip example/test files
        if rust_file.name in skip_files:
            continue

        try:
            operations = find_operations_in_file(str(rust_file))

            # Deduplicate operations by name and group by service
            for op in operations:
                op_name = op.get('name')
                if op_name and op_name not in seen_operations:
                    seen_operations.add(op_name)
                    all_operations.append(op)

                    # Group by service (only for non-duplicate operations)
                    service = op.get('service')
                    if service:
                        if service not in services:
                            services[service] = {
                                'operations': [],
                                'info': SERVICE_INFO.get(service, {}),
                            }
                        services[service]['operations'].append(op)

        except Exception as e:
            print(f"Error processing {rust_file}: {e}")

    return {
        'operations': all_operations,
        'services': services,
        'service_info': SERVICE_INFO,
        'total_count': len(all_operations)
    }


def filter_operations(results: Dict, service_filter: str = None, grep_filter: str = None) -> Dict:
    """Filter operations by service or grep pattern."""
    if not service_filter and not grep_filter:
        return results

    filtered_services = {}
    filtered_operations = []

    for service, service_data in results['services'].items():
        if service_filter and service_filter.lower() != service.lower():
            continue

        filtered_ops = []
        for op in service_data['operations']:
            if grep_filter:
                search_text = f"{op['name']} {op['action']} {' '.join(f['name'] for f in op['request_fields'])}"
                if grep_filter.lower() not in search_text.lower():
                    continue

            filtered_ops.append(op)
            filtered_operations.append(op)

        if filtered_ops:
            filtered_services[service] = {
                'operations': filtered_ops,
                'info': service_data.get('info', {}),
            }

    return {
        'operations': filtered_operations,
        'services': filtered_services,
        'service_info': results.get('service_info', SERVICE_INFO),
        'total_count': len(filtered_operations)
    }


def get_operation_by_name(results: Dict, operation_name: str) -> Optional[Dict]:
    """Find an operation by its name (case-insensitive, partial match)."""
    # Normalize search - remove 'Operation' suffix if present
    search_name = operation_name.lower()
    if search_name.endswith('operation'):
        search_name = search_name[:-9]

    for op in results['operations']:
        op_name = op['name'].lower()
        # Remove 'Operation' suffix for comparison
        if op_name.endswith('operation'):
            op_name = op_name[:-9]

        if op_name == search_name or op['name'].lower() == operation_name.lower():
            return op

    return None


def main():
    """Main entry point."""
    import argparse

    parser = argparse.ArgumentParser(description="Discover Sonos API operations")
    parser.add_argument('--service', help='Filter by service name (e.g., AVTransport, RenderingControl)')
    parser.add_argument('--grep', help='Filter by keyword (searches names, actions, and parameters)')
    parser.add_argument('--list-services', action='store_true', help='Just list service names')
    parser.add_argument('--operation', help='Get details for a specific operation by name')
    parser.add_argument('--json', action='store_true', help='Output in JSON format')
    parser.add_argument('--output', help='Output file path (default: discovered_operations.json)')

    args = parser.parse_args()

    try:
        results = discover_all_operations()

        if args.list_services:
            if args.json:
                print(json.dumps({
                    'services': list(results['services'].keys()),
                    'service_info': results['service_info']
                }, indent=2))
            else:
                print("Available services:")
                for service in results['services'].keys():
                    info = SERVICE_INFO.get(service, {})
                    print(f"  - {service}")
                    if info:
                        print(f"      endpoint: {info.get('endpoint', 'N/A')}")
            return 0

        if args.operation:
            op = get_operation_by_name(results, args.operation)
            if op:
                if args.json:
                    print(json.dumps(op, indent=2))
                else:
                    print(f"Operation: {op['name']}")
                    print(f"  Action: {op['action']}")
                    print(f"  Service: {op['service']}")
                    if op.get('service_info'):
                        print(f"  Endpoint: {op['service_info']['endpoint']}")
                        print(f"  Service URI: {op['service_info']['service_uri']}")
                    print(f"  Parameters:")
                    for field in op['request_fields']:
                        req_marker = "*" if field['required'] else " "
                        print(f"    {req_marker} {field['name']}: {field['type']} (XML: {field['xml_name']})")
                return 0
            else:
                print(f"Operation '{args.operation}' not found")
                return 1

        # Apply filters
        filtered_results = filter_operations(results, args.service, args.grep)

        if args.json:
            print(json.dumps(filtered_results, indent=2))
        else:
            print(f"Found {filtered_results['total_count']} operations across {len(filtered_results['services'])} services:")
            if args.service:
                print(f"(filtered by service: {args.service})")
            if args.grep:
                print(f"(filtered by keyword: {args.grep})")
            print()

            for service, service_data in filtered_results['services'].items():
                info = service_data.get('info', {})
                print(f"Service: {service}")
                if info:
                    print(f"  Endpoint: {info.get('endpoint', 'N/A')}")
                for op in service_data['operations']:
                    print(f"  - {op['name']} ({op['action']})")
                    if op['request_fields']:
                        params = [f"{f['name']}:{f['type']}" for f in op['request_fields']]
                        print(f"    Parameters: {', '.join(params)}")
                print()

        # Save detailed results to JSON
        output_path = args.output or 'discovered_operations.json'
        with open(output_path, 'w') as f:
            json.dump(results, f, indent=2)

        if not args.json:
            print(f"Detailed results saved to {output_path}")

    except Exception as e:
        print(f"Error: {e}")
        return 1

    return 0


if __name__ == "__main__":
    exit(main())
