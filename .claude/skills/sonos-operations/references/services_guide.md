# Sonos UPnP Services Guide

This guide provides an overview of the UPnP services supported by Sonos devices and their available operations.

## Service Overview

Sonos devices expose functionality through several UPnP services:

- **AVTransport** - Media playback and transport control
- **RenderingControl** - Volume, mute, bass/treble, loudness
- **DeviceProperties** - Device information and settings
- **ZoneGroupTopology** - Multi-room group management
- **GroupRenderingControl** - Group-wide volume control (if supported)

## AVTransport Service

**Purpose**: Controls media playback, transport state, and media URIs.

### Core Playback Operations
- `Play` - Start playback at specified speed
- `Pause` - Pause current playback
- `Stop` - Stop playback and clear transport state
- `Next` - Skip to next track
- `Previous` - Go to previous track

### Transport State Queries
- `GetTransportInfo` - Current transport state (PLAYING, PAUSED_PLAYBACK, STOPPED)
- `GetPositionInfo` - Current track info, position, duration
- `GetMediaInfo` - Current media URI and metadata
- `GetDeviceCapabilities` - Supported transport actions

### Media Management
- `SetAVTransportURI` - Set the current media URI and metadata
- `SetNextAVTransportURI` - Queue next media URI
- `Seek` - Seek to specific track or time position

### Advanced Operations
- `GetCurrentTransportActions` - Available transport actions in current state
- `BecomeCoordinatorOfStandaloneGroup` - Leave current group
- `DelegateGroupCoordinationTo` - Transfer group coordination

## RenderingControl Service

**Purpose**: Controls audio rendering properties (volume, bass, treble, etc.).

### Volume Operations
- `SetVolume` - Set volume level (0-100)
- `GetVolume` - Get current volume level
- `SetRelativeVolume` - Adjust volume by offset

### Mute Operations
- `SetMute` - Enable/disable mute
- `GetMute` - Get current mute state

### Audio Enhancement
- `SetBass` - Set bass level (-10 to +10)
- `GetBass` - Get current bass level
- `SetTreble` - Set treble level (-10 to +10)
- `GetTreble` - Get current treble level
- `SetLoudness` - Enable/disable loudness compensation
- `GetLoudness` - Get current loudness state

### Channel Support
Most operations support these channels:
- `Master` - Overall/combined audio
- `LF` - Left front channel
- `RF` - Right front channel

## DeviceProperties Service

**Purpose**: Provides device information and configuration.

### Zone Information
- `GetZoneInfo` - Zone name, icon, and configuration
- `GetZoneAttributes` - Current zone name and icon settings
- `SetZoneAttributes` - Update zone name and icon

### Device Information
- `GetHouseholdID` - Unique household identifier
- `GetZoneInfo` - Zone-specific information

### Configuration
Operations for device-specific settings and preferences.

## ZoneGroupTopology Service

**Purpose**: Manages multi-room grouping and topology.

### Group State
- `GetZoneGroupState` - Complete topology of all zones and groups
- `GetZoneGroupAttributes` - Attributes for specific zone groups

### Group Management
Operations to join/leave groups and manage multi-room setups.

## GroupRenderingControl Service

**Purpose**: Controls rendering properties at the group level (when supported).

### Group Volume Operations
Similar to RenderingControl but applied to entire groups:
- `SetGroupVolume` - Set volume for all speakers in group
- `GetGroupVolume` - Get group volume level
- `SetGroupMute` - Mute/unmute entire group

## Service Usage Patterns

### Operation Prerequisites
- **AVTransport**: Most operations work in any state, but Play requires media URI
- **RenderingControl**: All operations work regardless of playback state
- **DeviceProperties**: Read operations always available, write may have restrictions
- **ZoneGroupTopology**: Always available for topology queries

### State Dependencies
- Some AVTransport operations depend on current transport state
- Group operations may behave differently for coordinators vs. members
- Volume operations have different effects on grouped vs. ungrouped speakers

### Error Conditions
- `401 Invalid Action` - Operation not supported by this device/service
- `402 Invalid Args` - Parameters are malformed or out of range
- `501 Action Failed` - Device couldn't execute (busy, invalid state, etc.)

## Discovery and Service Endpoints

Each service is available at a specific endpoint on the device:
- Device IP: Usually port 1400
- Service URLs: `/MediaRenderer/[ServiceName]/Control`
- Event URLs: `/MediaRenderer/[ServiceName]/Event`

## Best Practices

### Service Selection
- Use **AVTransport** for playback control
- Use **RenderingControl** for audio adjustments
- Use **DeviceProperties** for device info
- Use **ZoneGroupTopology** for multi-room management

### Error Handling
- Always handle network timeouts gracefully
- Check device state before state-dependent operations
- Validate parameters before sending requests

### Performance
- Group multiple related operations when possible
- Cache device information to reduce repeated queries
- Use appropriate timeouts for different operation types