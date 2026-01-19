---
name: sonos-operations
description: Execute any Sonos API operation dynamically with automatic discovery, parameter parsing, and direct SOAP execution. Use when users want to test operations on speakers, debug device behavior, or execute any UPnP operation without requiring code changes. Operations are discovered automatically from the sonos-api crate.
---

# Sonos Operations

Execute any operation from the sonos-api crate dynamically with automatic discovery, parameter handling, and direct execution. **No code changes required when new operations are added** - operations are discovered automatically from the Rust source.

## Quick Start

**Execute Any Operation:**
```bash
# Basic execution
python .claude/skills/sonos-operations/scripts/execute_operation.py <operation> <ip> [-p param=value]

# Examples
python .claude/skills/sonos-operations/scripts/execute_operation.py Play 192.168.1.100
python .claude/skills/sonos-operations/scripts/execute_operation.py GetVolume 192.168.1.100 -p channel=Master
python .claude/skills/sonos-operations/scripts/execute_operation.py SetVolume 192.168.1.100 -p desired_volume=50
```

**List Available Operations:**
```bash
python .claude/skills/sonos-operations/scripts/execute_operation.py --list
python .claude/skills/sonos-operations/scripts/discover_operations.py --service AVTransport
```

**Get Operation Details:**
```bash
python .claude/skills/sonos-operations/scripts/execute_operation.py --info SetVolume
python .claude/skills/sonos-operations/scripts/discover_operations.py --operation Play
```

## How It Works

This skill uses **dynamic discovery** to find all operations defined in the sonos-api crate and executes them via **direct SOAP calls**:

1. **Discovery**: Parses Rust macro definitions (`define_upnp_operation!`, `define_operation_with_response!`) to extract operation metadata
2. **Execution**: Constructs SOAP envelopes directly in Python and sends HTTP requests to Sonos devices
3. **Zero maintenance**: New operations added to sonos-api are automatically available - no skill updates needed

## Usage Patterns

### By Natural Language Request

When a user asks to run an operation, use this workflow:

1. **Discover the operation** (if needed):
   ```bash
   python .claude/skills/sonos-operations/scripts/execute_operation.py --info <operation_name>
   ```

2. **Find devices** (if IP not provided):
   ```bash
   python .claude/skills/sonos-operations/scripts/discover_devices.py
   ```

3. **Execute the operation**:
   ```bash
   python .claude/skills/sonos-operations/scripts/execute_operation.py <operation> <ip> [-p key=value ...]
   ```

### Common Execution Examples

```bash
# Playback control
python .claude/skills/sonos-operations/scripts/execute_operation.py Play 192.168.1.100
python .claude/skills/sonos-operations/scripts/execute_operation.py Pause 192.168.1.100
python .claude/skills/sonos-operations/scripts/execute_operation.py Stop 192.168.1.100
python .claude/skills/sonos-operations/scripts/execute_operation.py Next 192.168.1.100
python .claude/skills/sonos-operations/scripts/execute_operation.py Previous 192.168.1.100

# Volume control
python .claude/skills/sonos-operations/scripts/execute_operation.py GetVolume 192.168.1.100 -p channel=Master
python .claude/skills/sonos-operations/scripts/execute_operation.py SetVolume 192.168.1.100 -p desired_volume=50 -p channel=Master

# Transport info
python .claude/skills/sonos-operations/scripts/execute_operation.py GetTransportInfo 192.168.1.100
python .claude/skills/sonos-operations/scripts/execute_operation.py GetPositionInfo 192.168.1.100
python .claude/skills/sonos-operations/scripts/execute_operation.py GetMediaInfo 192.168.1.100

# Seek
python .claude/skills/sonos-operations/scripts/execute_operation.py Seek 192.168.1.100 -p unit=REL_TIME -p target=00:02:30
python .claude/skills/sonos-operations/scripts/execute_operation.py Seek 192.168.1.100 -p unit=TRACK_NR -p target=5
```

### JSON Output for Parsing

Add `--json` for machine-readable output:
```bash
python .claude/skills/sonos-operations/scripts/execute_operation.py GetVolume 192.168.1.100 --json
```

## Parameter Handling

### Common Defaults

Most operations automatically use sensible defaults:
- `instance_id`: 0 (always added automatically)
- `channel`: "Master" (for volume operations - must be specified)
- `speed`: "1" (for Play operation - must be specified)

### Parameter Syntax

```bash
-p key=value           # String or auto-detected type
-p volume=50           # Auto-detected as integer
-p crossfade=true      # Auto-detected as boolean
-p channel=Master      # String value
```

### Finding Required Parameters

```bash
# Show all parameters for an operation (* = required)
python .claude/skills/sonos-operations/scripts/execute_operation.py --info SetVolume
```

Output:
```
Operation: SetVolumeOperation
  Action: SetVolume
  Service: RenderingControl
  Parameters:
    * instance_id: u32 (default: 0)
    * channel: String
    * desired_volume: u8
```

## Operation Discovery

### List All Operations

```bash
python .claude/skills/sonos-operations/scripts/execute_operation.py --list
```

### Filter by Service

```bash
python .claude/skills/sonos-operations/scripts/discover_operations.py --service AVTransport
python .claude/skills/sonos-operations/scripts/discover_operations.py --service RenderingControl
python .claude/skills/sonos-operations/scripts/discover_operations.py --service ZoneGroupTopology
```

### Search Operations

```bash
python .claude/skills/sonos-operations/scripts/discover_operations.py --grep volume
python .claude/skills/sonos-operations/scripts/discover_operations.py --grep transport
python .claude/skills/sonos-operations/scripts/discover_operations.py --grep queue
```

### Get Full Operation Details

```bash
# Human-readable
python .claude/skills/sonos-operations/scripts/discover_operations.py --operation SetVolume

# JSON format
python .claude/skills/sonos-operations/scripts/discover_operations.py --operation SetVolume --json
```

## Device Discovery

### Find Devices on Network

```bash
python .claude/skills/sonos-operations/scripts/discover_devices.py
```

This discovers Sonos devices and saves them to `discovered_devices.json`.

### Manual IP Entry

If device discovery fails, the user can provide an IP address directly:
```bash
python .claude/skills/sonos-operations/scripts/execute_operation.py GetVolume 192.168.1.100 -p channel=Master
```

## Error Handling

The script provides detailed error information:

```bash
# Missing parameter
$ python execute_operation.py SetVolume 192.168.1.100
Operation failed: Required parameter 'channel' not provided

# Unknown operation
$ python execute_operation.py UnknownOp 192.168.1.100
Operation failed: Operation 'UnknownOp' not found. Available: ...

# Device unreachable
$ python execute_operation.py Play 192.168.1.1
Operation failed: Network error: [Errno 60] Operation timed out

# SOAP fault (UPnP error)
$ python execute_operation.py Play 192.168.1.100
Operation failed: Transition not available
Error code: 701
```

## Testing New Operations

When implementing new operations in sonos-api:

1. **Implement the operation** using `define_upnp_operation!` or `define_operation_with_response!` macro
2. **Run discovery** to verify it's detected:
   ```bash
   python .claude/skills/sonos-operations/scripts/discover_operations.py --operation YourNewOperation
   ```
3. **Test execution**:
   ```bash
   python .claude/skills/sonos-operations/scripts/execute_operation.py YourNewOperation 192.168.1.100 -p param=value
   ```

No changes to this skill are needed - new operations are automatically available!

## Reference Files

- **discover_operations.py**: Parses Rust source to extract all operation metadata
- **execute_operation.py**: Executes operations via direct SOAP calls
- **discover_devices.py**: Discovers Sonos devices on the network
- **references/operation_patterns.md**: Common parameter patterns and UPnP values
- **references/services_guide.md**: Service-specific documentation

## Architecture

```
User Request
    |
    v
discover_operations.py   <-- Parses Rust macros to find operations
    |
    v
execute_operation.py     <-- Constructs SOAP, sends HTTP request
    |
    v
Sonos Device (HTTP/SOAP) <-- Direct communication, no Rust compilation
    |
    v
Response Parsing         <-- Extract values from XML response
```

This design ensures:
- **No Rust compilation** at runtime
- **Automatic support** for new operations
- **Fast execution** with minimal dependencies
- **Easy debugging** with verbose/JSON output modes
