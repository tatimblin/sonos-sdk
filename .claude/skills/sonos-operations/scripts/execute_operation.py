#!/usr/bin/env python3
"""
Execute Sonos operations using direct SOAP calls.

This script dynamically executes any Sonos UPnP operation by:
1. Discovering available operations from the sonos-api crate
2. Constructing SOAP payloads from operation metadata
3. Making direct HTTP requests to Sonos devices

No Rust compilation required - operations are executed directly via HTTP/SOAP.
"""

import json
import sys
import urllib.request
import urllib.error
import xml.etree.ElementTree as ET
from typing import Dict, Any, Optional, List
from pathlib import Path

# Import from discover_operations for operation metadata
try:
    from discover_operations import discover_all_operations, get_operation_by_name, SERVICE_INFO
except ImportError:
    # Fallback if run from a different directory
    import os
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from discover_operations import discover_all_operations, get_operation_by_name, SERVICE_INFO


def build_soap_envelope(service_uri: str, action: str, payload: str) -> str:
    """Build a SOAP envelope for the given action and payload."""
    return f'''<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
    <s:Body>
        <u:{action} xmlns:u="{service_uri}">
            {payload}
        </u:{action}>
    </s:Body>
</s:Envelope>'''


def build_payload_from_params(operation: Dict, params: Dict[str, Any]) -> str:
    """Build the XML payload body from operation metadata and parameters."""
    payload_parts = []

    for field in operation['request_fields']:
        field_name = field['name']
        xml_name = field['xml_name']
        field_type = field['type']

        # Get value from params or use defaults
        if field_name in params:
            value = params[field_name]
        elif field_name == 'instance_id':
            value = 0  # Default instance_id
        elif not field['required']:
            continue  # Skip optional fields not provided
        else:
            raise ValueError(f"Required parameter '{field_name}' not provided")

        # Convert value to appropriate XML representation
        xml_value = convert_value_to_xml(value, field_type)
        payload_parts.append(f"<{xml_name}>{xml_value}</{xml_name}>")

    return ''.join(payload_parts)


def convert_value_to_xml(value: Any, field_type: str) -> str:
    """Convert a Python value to its XML string representation."""
    if field_type == 'bool':
        if isinstance(value, bool):
            return '1' if value else '0'
        elif isinstance(value, str):
            return '1' if value.lower() in ('true', '1', 'yes') else '0'
        else:
            return '1' if value else '0'
    elif field_type in ('u32', 'u16', 'u8', 'i32', 'i16', 'i8'):
        return str(int(value))
    else:
        # String type - escape XML entities
        return xml_escape(str(value))


def xml_escape(text: str) -> str:
    """Escape special XML characters."""
    return (text
            .replace('&', '&amp;')
            .replace('<', '&lt;')
            .replace('>', '&gt;')
            .replace('"', '&quot;')
            .replace("'", '&apos;'))


def execute_soap_request(
    ip: str,
    port: int,
    endpoint: str,
    service_uri: str,
    action: str,
    payload: str,
    timeout: int = 10
) -> Dict:
    """Execute a SOAP request and return the parsed response."""
    url = f"http://{ip}:{port}/{endpoint}"
    soap_action = f'"{service_uri}#{action}"'
    envelope = build_soap_envelope(service_uri, action, payload)

    # Create request
    request = urllib.request.Request(
        url,
        data=envelope.encode('utf-8'),
        headers={
            'Content-Type': 'text/xml; charset="utf-8"',
            'SOAPACTION': soap_action,
        },
        method='POST'
    )

    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            response_data = response.read().decode('utf-8')
            return parse_soap_response(response_data, action)

    except urllib.error.HTTPError as e:
        error_body = e.read().decode('utf-8') if e.fp else ''
        return parse_soap_fault(error_body, e.code)
    except urllib.error.URLError as e:
        return {
            'success': False,
            'error': f'Network error: {e.reason}',
            'error_code': None,
        }
    except Exception as e:
        return {
            'success': False,
            'error': str(e),
            'error_code': None,
        }


def parse_soap_response(xml_text: str, action: str) -> Dict:
    """Parse a successful SOAP response."""
    try:
        # Parse the XML
        root = ET.fromstring(xml_text)

        # Find the response element (handle namespaces)
        response_name = f"{action}Response"

        # Try to find the response element with various namespace handling
        response_elem = None
        for elem in root.iter():
            # Strip namespace for comparison
            local_name = elem.tag.split('}')[-1] if '}' in elem.tag else elem.tag
            if local_name == response_name:
                response_elem = elem
                break

        if response_elem is None:
            # Check for SOAP Body and look inside
            for elem in root.iter():
                local_name = elem.tag.split('}')[-1] if '}' in elem.tag else elem.tag
                if local_name == 'Body':
                    for child in elem:
                        child_name = child.tag.split('}')[-1] if '}' in child.tag else child.tag
                        if child_name == response_name:
                            response_elem = child
                            break
                    break

        if response_elem is not None:
            # Extract all response fields
            result = {'success': True, 'response': {}}
            for child in response_elem:
                # Get local name without namespace
                local_name = child.tag.split('}')[-1] if '}' in child.tag else child.tag
                result['response'][local_name] = child.text or ''
            return result
        else:
            # No response element found - operation may have no response
            return {'success': True, 'response': {}}

    except ET.ParseError as e:
        return {
            'success': False,
            'error': f'Failed to parse response XML: {e}',
            'raw_response': xml_text,
        }


def parse_soap_fault(xml_text: str, http_code: int) -> Dict:
    """Parse a SOAP fault response."""
    try:
        root = ET.fromstring(xml_text)

        # Look for Fault element
        fault_code = None
        fault_string = None
        upnp_error_code = None
        upnp_error_desc = None

        for elem in root.iter():
            local_name = elem.tag.split('}')[-1] if '}' in elem.tag else elem.tag

            if local_name == 'faultcode':
                fault_code = elem.text
            elif local_name == 'faultstring':
                fault_string = elem.text
            elif local_name == 'errorCode':
                upnp_error_code = elem.text
            elif local_name == 'errorDescription':
                upnp_error_desc = elem.text

        error_msg = upnp_error_desc or fault_string or f'HTTP {http_code}'

        return {
            'success': False,
            'error': error_msg,
            'error_code': int(upnp_error_code) if upnp_error_code else http_code,
            'fault_code': fault_code,
        }

    except ET.ParseError:
        return {
            'success': False,
            'error': f'HTTP {http_code}: {xml_text[:200]}',
            'error_code': http_code,
        }


def execute_operation(
    operation_name: str,
    ip: str,
    params: Dict[str, Any] = None,
    port: int = 1400,
    operations_cache: Dict = None
) -> Dict:
    """
    Execute a Sonos operation by name.

    Args:
        operation_name: Name of the operation (e.g., 'PlayOperation', 'GetVolume')
        ip: IP address of the Sonos device
        params: Dictionary of parameters for the operation
        port: Sonos device port (default 1400)
        operations_cache: Optional pre-discovered operations dict

    Returns:
        Dict with 'success' bool and either 'response' or 'error'
    """
    params = params or {}

    # Discover operations if not cached
    if operations_cache is None:
        try:
            operations_cache = discover_all_operations()
        except FileNotFoundError:
            return {
                'success': False,
                'error': 'Could not find sonos-api source. Run from repository root.',
            }

    # Find the operation
    operation = get_operation_by_name(operations_cache, operation_name)
    if not operation:
        available = [op['name'] for op in operations_cache['operations']]
        return {
            'success': False,
            'error': f"Operation '{operation_name}' not found. Available: {', '.join(sorted(available)[:10])}...",
        }

    # Get service info
    service_info = operation.get('service_info')
    if not service_info:
        service_name = operation.get('service')
        service_info = SERVICE_INFO.get(service_name)
        if not service_info:
            return {
                'success': False,
                'error': f"Unknown service '{service_name}' for operation '{operation_name}'",
            }

    # Build the payload
    try:
        payload = build_payload_from_params(operation, params)
    except ValueError as e:
        return {
            'success': False,
            'error': str(e),
        }

    # Execute the SOAP request
    return execute_soap_request(
        ip=ip,
        port=port,
        endpoint=service_info['endpoint'],
        service_uri=service_info['service_uri'],
        action=operation['action'],
        payload=payload,
    )


def format_result(result: Dict, verbose: bool = False) -> str:
    """Format operation result for display."""
    lines = []

    if result['success']:
        lines.append("Operation succeeded")
        if result.get('response'):
            lines.append("Response:")
            for key, value in result['response'].items():
                # Truncate long values
                display_value = value if len(str(value)) < 100 else f"{str(value)[:100]}..."
                lines.append(f"  {key}: {display_value}")
        else:
            lines.append("(No response data)")
    else:
        lines.append(f"Operation failed: {result.get('error', 'Unknown error')}")
        if result.get('error_code'):
            lines.append(f"Error code: {result['error_code']}")

    if verbose and result.get('raw_response'):
        lines.append(f"Raw response: {result['raw_response'][:500]}")

    return '\n'.join(lines)


def main():
    """Main entry point."""
    import argparse

    parser = argparse.ArgumentParser(
        description="Execute Sonos operations via direct SOAP calls",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Get transport info (playback state)
  python execute_operation.py GetTransportInfo 192.168.1.100

  # Play with default parameters
  python execute_operation.py Play 192.168.1.100

  # Set volume to 50
  python execute_operation.py SetVolume 192.168.1.100 -p desired_volume=50 -p channel=Master

  # Get volume
  python execute_operation.py GetVolume 192.168.1.100 -p channel=Master

  # Pause playback
  python execute_operation.py Pause 192.168.1.100

  # List available operations
  python execute_operation.py --list

  # Get operation details
  python execute_operation.py --info SetVolume
"""
    )
    parser.add_argument('operation', nargs='?', help='Operation name (e.g., Play, GetVolume)')
    parser.add_argument('ip', nargs='?', help='Device IP address')
    parser.add_argument('-p', '--param', action='append', metavar='KEY=VALUE',
                       help='Operation parameter (can be used multiple times)')
    parser.add_argument('--port', type=int, default=1400, help='Device port (default: 1400)')
    parser.add_argument('--json', action='store_true', help='Output in JSON format')
    parser.add_argument('--verbose', '-v', action='store_true', help='Verbose output')
    parser.add_argument('--list', action='store_true', help='List all available operations')
    parser.add_argument('--info', metavar='OPERATION', help='Show details for an operation')

    args = parser.parse_args()

    # Discover operations
    try:
        operations_cache = discover_all_operations()
    except FileNotFoundError as e:
        print(f"Error: {e}")
        print("Make sure to run from the repository root directory.")
        return 1

    # Handle --list
    if args.list:
        if args.json:
            ops = [{'name': op['name'], 'service': op['service'], 'action': op['action']}
                   for op in operations_cache['operations']]
            print(json.dumps({'operations': sorted(ops, key=lambda x: x['name'])}, indent=2))
        else:
            print(f"Available operations ({operations_cache['total_count']}):\n")
            for service, data in sorted(operations_cache['services'].items()):
                print(f"{service}:")
                for op in sorted(data['operations'], key=lambda x: x['name']):
                    params = ', '.join(f['name'] for f in op['request_fields'] if f['name'] != 'instance_id')
                    params_str = f"({params})" if params else "()"
                    print(f"  - {op['name']} {params_str}")
                print()
        return 0

    # Handle --info
    if args.info:
        op = get_operation_by_name(operations_cache, args.info)
        if not op:
            print(f"Operation '{args.info}' not found")
            return 1

        if args.json:
            print(json.dumps(op, indent=2))
        else:
            print(f"Operation: {op['name']}")
            print(f"  Action: {op['action']}")
            print(f"  Service: {op['service']}")
            if op.get('service_info'):
                print(f"  Endpoint: {op['service_info']['endpoint']}")
            print(f"  Parameters:")
            for field in op['request_fields']:
                req = "*" if field['required'] else " "
                default = " (default: 0)" if field['name'] == 'instance_id' else ""
                print(f"    {req} {field['name']}: {field['type']}{default}")
            print(f"\n  * = required")
        return 0

    # Validate required arguments for execution
    if not args.operation:
        parser.print_help()
        return 1

    if not args.ip:
        print("Error: IP address is required for operation execution")
        return 1

    # Parse parameters
    params = {}
    if args.param:
        for p in args.param:
            if '=' not in p:
                print(f"Error: Invalid parameter format '{p}'. Use KEY=VALUE")
                return 1
            key, value = p.split('=', 1)

            # Try to parse as number or boolean
            if value.isdigit():
                params[key] = int(value)
            elif value.lower() in ('true', 'false'):
                params[key] = value.lower() == 'true'
            else:
                params[key] = value

    # Execute the operation
    result = execute_operation(
        operation_name=args.operation,
        ip=args.ip,
        params=params,
        port=args.port,
        operations_cache=operations_cache,
    )

    # Output result
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(format_result(result, verbose=args.verbose))

    return 0 if result['success'] else 1


if __name__ == "__main__":
    sys.exit(main())
