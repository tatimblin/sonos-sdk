# Sonos SDK: TUI Data Catalog

This document catalogs all data and capabilities available from the sonos-sdk that would be relevant for building a Terminal User Interface (TUI).

## Overview

The sonos-sdk provides three main public APIs:
1. **sonos-discovery** - Device discovery on local network
2. **sonos-api** - Direct operation execution (stateless commands)
3. **sonos-state** - Reactive state management (recommended for TUI)

---

## 1. Device Discovery (`sonos-discovery`)

### What You Get From Discovery

When you discover devices on the network, each `Device` contains:

```rust
pub struct Device {
    pub id: String,              // Unique device identifier (UDN), e.g., "uuid:RINCON_000E58A0123456"
    pub name: String,            // Friendly name of the device
    pub room_name: String,       // Room name where the device is located
    pub ip_address: String,      // IP address of the device
    pub port: u16,              // Port number (typically 1400)
    pub model_name: String,     // Model name (e.g., "Sonos One", "Sonos Play:1")
}
```

### Usage Patterns

```rust
// Simple one-shot discovery
let devices = sonos_discovery::get();

// Discovery with custom timeout
let devices = sonos_discovery::get_with_timeout(Duration::from_secs(5));

// Streaming discovery (iterator)
for event in sonos_discovery::get_iter() {
    match event {
        DeviceEvent::Found(device) => {
            // Handle device as it's discovered
        }
    }
}
```

---

## 2. Reactive Properties (`sonos-state`)

**Recommended for TUI** - Properties can be watched reactively with automatic change notifications.

### Speaker-Scoped Properties

These properties belong to individual speakers:

#### Audio Properties (from RenderingControl service)

| Property | Type | Range | Description |
|----------|------|-------|-------------|
| `Volume` | `u8` | 0-100 | Master volume level |
| `Mute` | `bool` | - | Master mute state |
| `Bass` | `i8` | -10 to +10 | Bass EQ setting |
| `Treble` | `i8` | -10 to +10 | Treble EQ setting |
| `Loudness` | `bool` | - | Loudness compensation |

#### Playback Properties (from AVTransport service)

| Property | Type | Values | Description |
|----------|------|--------|-------------|
| `PlaybackState` | enum | `Playing`, `Paused`, `Stopped`, `Transitioning` | Current playback state |
| `Position` | struct | - | Playback position and duration (in milliseconds) |
| `CurrentTrack` | struct | - | Currently playing track information |

**Position Details:**
```rust
pub struct Position {
    pub position_ms: u64,    // Current position in milliseconds
    pub duration_ms: u64,    // Total duration in milliseconds
}

// Helper methods:
position.progress()  // Returns 0.0 to 1.0 fraction
```

**CurrentTrack Details:**
```rust
pub struct CurrentTrack {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_art_uri: Option<String>,
    pub uri: Option<String>,
}

// Helper methods:
track.display()      // Returns formatted string like "Artist - Title"
track.is_empty()     // Check if track has meaningful content
```

#### Topology Properties (from ZoneGroupTopology service)

| Property | Type | Description |
|----------|------|-------------|
| `GroupMembership` | struct | Speaker's group membership and coordinator status |

**GroupMembership Details:**
```rust
pub struct GroupMembership {
    pub group_id: GroupId,        // ID of the group this speaker belongs to
    pub is_coordinator: bool,     // Whether this speaker is the group coordinator
}
```

### Group-Scoped Properties

These properties belong to speaker groups:

| Property | Type | Range | Description |
|----------|------|-------|-------------|
| `GroupVolume` | `u16` | 0-100 | Group-wide volume level |

### System-Scoped Properties

| Property | Type | Description |
|----------|------|-------------|
| `Topology` | struct | System-wide view of all speakers and groups |

**Topology Details:**
```rust
pub struct Topology {
    pub speakers: Vec<SpeakerInfo>,
    pub groups: Vec<GroupInfo>,
}

pub struct SpeakerInfo {
    pub id: SpeakerId,
    pub name: String,
    pub room_name: String,
    pub ip_address: IpAddr,
    pub port: u16,
    pub model_name: String,
    pub software_version: String,
    pub satellites: Vec<SpeakerId>,  // For home theater setups
}

pub struct GroupInfo {
    pub id: GroupId,
    pub coordinator_id: SpeakerId,
    pub member_ids: Vec<SpeakerId>,
}
```

### Usage Pattern (Recommended for TUI)

```rust
// Create state manager
let manager = StateManager::new()?;

// Add discovered devices
let devices = sonos_discovery::get();
manager.add_devices(devices)?;

// Get current value (non-reactive)
let speaker_id = SpeakerId::new("RINCON_123");
if let Some(volume) = manager.get_property::<Volume>(&speaker_id) {
    println!("Current volume: {}%", volume.0);
}

// Watch for changes (reactive)
manager.register_watch(&speaker_id, "volume");

// Blocking iteration over ALL changes
for event in manager.iter() {
    println!("{} changed on {}", event.property_key, event.speaker_id);
    // Fetch the new value
    if let Some(vol) = manager.get_property::<Volume>(&event.speaker_id) {
        println!("New volume: {}%", vol.0);
    }
}

// Non-blocking check for changes
for event in manager.iter().try_iter() {
    // Handle events without blocking
}

// Wait with timeout
if let Some(event) = manager.iter().recv_timeout(Duration::from_secs(1)) {
    // Handle event
}
```

---

## 3. Direct Operations (`sonos-api`)

**For sending commands** - All operations are stateless and execute immediately.

### AVTransport Operations (30 operations)

#### Playback Control
- `PlayOperation` - Start playback
- `PauseOperation` - Pause playback
- `StopOperation` - Stop playback
- `NextOperation` - Skip to next track
- `PreviousOperation` - Go to previous track

#### Seek and Position
- `SeekOperation` - Seek to position (by track number, time, or delta)
  - Units: `TRACK_NR`, `REL_TIME`, `TIME_DELTA`
- `GetPositionInfoOperation` → Returns track, duration, position, metadata, URI

#### Transport Info
- `GetTransportInfoOperation` → Returns state, status, speed
- `GetTransportSettingsOperation` → Returns play mode, recording quality
- `GetCurrentTransportActionsOperation` → Returns available actions
- `GetDeviceCapabilitiesOperation` → Returns supported media types

#### Media and URI Management
- `GetMediaInfoOperation` → Returns tracks, duration, URIs, metadata
- `SetAVTransportURIOperation` - Set what to play (URI + metadata)
- `SetNextAVTransportURIOperation` - Set next track

#### Play Settings
- `SetPlayModeOperation` - Set play mode
  - Modes: `NORMAL`, `REPEAT_ALL`, `REPEAT_ONE`, `SHUFFLE_NOREPEAT`, `SHUFFLE`, `SHUFFLE_REPEAT_ONE`
- `GetCrossfadeModeOperation` / `SetCrossfadeModeOperation` - Crossfade on/off

#### Sleep Timer
- `ConfigureSleepTimerOperation` - Set sleep timer duration
- `GetRemainingSleepTimerDurationOperation` → Returns remaining time

#### Queue Management
- `AddURIToQueueOperation` → Returns first track number, tracks added, queue length
- `RemoveTrackFromQueueOperation` - Remove single track
- `RemoveTrackRangeFromQueueOperation` → Returns new update ID
- `RemoveAllTracksFromQueueOperation` - Clear queue
- `SaveQueueOperation` → Returns assigned object ID
- `CreateSavedQueueOperation` → Returns tracks, length, object ID, update ID
- `BackupQueueOperation` - Backup current queue

#### Group Coordination
- `BecomeCoordinatorOfStandaloneGroupOperation` → Returns delegated coordinator ID, group ID
- `DelegateGroupCoordinationToOperation` - Transfer coordinator role

#### Alarms
- `SnoozeAlarmOperation` - Snooze active alarm
- `GetRunningAlarmPropertiesOperation` → Returns alarm ID, group ID, start time

### RenderingControl Operations (11 operations)

#### Volume Control
- `GetVolumeOperation` → Returns current volume (0-100)
- `SetVolumeOperation` - Set volume (0-100)
- `SetRelativeVolumeOperation` → Returns new volume after adjustment
  - Adjustment range: -100 to +100

#### Mute Control
- `GetMuteOperation` → Returns mute state (bool)
- `SetMuteOperation` - Set mute state

#### EQ Control
- `GetBassOperation` → Returns bass level (-10 to +10)
- `SetBassOperation` - Set bass level
- `GetTrebleOperation` → Returns treble level (-10 to +10)
- `SetTrebleOperation` - Set treble level

#### Loudness Control
- `GetLoudnessOperation` → Returns loudness state (bool)
- `SetLoudnessOperation` - Set loudness compensation

**Note:** All volume/mute operations support channel parameter:
- `Master` - Main volume/mute
- `LF` - Left front
- `RF` - Right front

### GroupRenderingControl Operations (6 operations)

**Must be sent to group coordinator only**

- `GetGroupVolumeOperation` → Returns group volume (0-100)
- `SetGroupVolumeOperation` - Set group volume
- `SetRelativeGroupVolumeOperation` → Returns new volume after adjustment
- `GetGroupMuteOperation` → Returns group mute state
- `SetGroupMuteOperation` - Set group mute
- `SnapshotGroupVolumeOperation` - Snapshot volume ratios for proportional changes

### GroupManagement Operations (4 operations)

**Must be sent to group coordinator only**

- `AddMemberOperation` → Returns transport settings, URI, group UUID, reset volume flag
  - Adds a speaker to the group
- `RemoveMemberOperation` - Remove speaker from group
- `ReportTrackBufferingResultOperation` - Report buffering status
- `SetSourceAreaIdsOperation` - Set source area identifiers

### ZoneGroupTopology Operations (1 operation)

- `GetZoneGroupStateOperation` → Returns full XML topology state

### Usage Pattern

```rust
let client = SonosClient::new();
let device_ip = "192.168.1.100";

// Simple command
let play_op = PlayOperation::build().unwrap();
client.execute(device_ip, play_op)?;

// Command with parameters
let volume_op = SetVolumeOperation::build("Master", 75).unwrap();
client.execute(device_ip, volume_op)?;

// Command with response
let position_op = GetPositionInfoOperation::build().unwrap();
let response = client.execute(device_ip, position_op)?;
println!("Track: {}, Position: {}", response.track, response.rel_time);
```

---

## 4. Real-Time Events (`sonos-stream` - Internal)

**Note:** These are processed automatically by `sonos-state`. Not intended for direct use.

### AVTransport Events

Real-time notifications include:
- `transport_state` - Playing/Paused/Stopped/Transitioning
- `transport_status` - OK/Error states
- `speed` - Playback speed
- `current_track_uri` - Current track URI
- `track_duration` - Track length
- `rel_time` - Current position
- `abs_time` - Absolute time
- `rel_count` - Track number
- `play_mode` - Repeat/Shuffle mode
- `track_metadata` - Full DIDL-Lite metadata XML
- `next_track_uri` - Next track URI
- `next_track_metadata` - Next track metadata
- `queue_length` - Number of tracks in queue

### RenderingControl Events

Real-time notifications include:
- `master_volume` - Master volume (0-100)
- `master_mute` - Master mute state
- `lf_volume` / `rf_volume` - Left/Right front volumes
- `lf_mute` / `rf_mute` - Left/Right front mutes
- `bass` - Bass level (-10 to +10)
- `treble` - Treble level (-10 to +10)
- `loudness` - Loudness state
- `balance` - Balance setting
- Additional channel data for home theater setups

### GroupRenderingControl Events

Real-time notifications include:
- `group_volume` - Group volume level
- `group_mute` - Group mute state
- `group_volume_changeable` - Whether group volume can be changed

### ZoneGroupTopology Events

Real-time notifications include:
- Full topology XML with all speakers and group memberships
- Coordinator changes
- Group membership changes

**Event Transparency:** The SDK automatically falls back to polling if UPnP events are blocked by firewall. Your code doesn't need to handle this - events stream consistently either way.

---

## 5. Summary: What to Show in a TUI

### Device List View
- Device name (`Device.name`)
- Room name (`Device.room_name`)
- IP address (`Device.ip_address`)
- Model name (`Device.model_name`)
- Group membership (from `GroupMembership` property)
- Is coordinator flag (from `GroupMembership.is_coordinator`)

### Now Playing View (per speaker or coordinator)
- Track title (`CurrentTrack.title`)
- Artist (`CurrentTrack.artist`)
- Album (`CurrentTrack.album`)
- Album art URI (`CurrentTrack.album_art_uri`)
- Playback state (`PlaybackState` - Playing/Paused/Stopped)
- Position / Duration (`Position.position_ms` / `Position.duration_ms`)
- Progress bar (use `Position.progress()` for 0.0-1.0)
- Queue length (from AVTransport events)

### Audio Controls (per speaker)
- Volume slider (`Volume` property, 0-100)
- Mute toggle (`Mute` property)
- Bass slider (`Bass` property, -10 to +10)
- Treble slider (`Treble` property, -10 to +10)
- Loudness toggle (`Loudness` property)

### Group Controls (per group coordinator)
- Group volume slider (`GroupVolume` property, 0-100)
- Group mute toggle (via GroupRenderingControl)
- Member list (from `GroupInfo.member_ids`)

### Playback Controls (buttons)
- Play (`PlayOperation`)
- Pause (`PauseOperation`)
- Stop (`StopOperation`)
- Next track (`NextOperation`)
- Previous track (`PreviousOperation`)
- Seek to position (`SeekOperation`)
- Set play mode (`SetPlayModeOperation`)

### System Topology View
- All speakers (from `Topology.speakers`)
- All groups (from `Topology.groups`)
- Group coordinator relationships
- Satellite speaker relationships (for home theater)

### Advanced Features
- Queue management (add, remove, clear, save)
- Sleep timer configuration
- Crossfade settings
- Group management (add/remove members)
- URI playback (podcasts, radio, local files)

---

## 6. Recommended TUI Architecture

```rust
use sonos_state::{StateManager, Volume, Mute, PlaybackState, CurrentTrack, Position};
use sonos_discovery;

// 1. Initialize
let manager = StateManager::new()?;
let devices = sonos_discovery::get();
manager.add_devices(devices)?;

// 2. Get speaker list for UI
let speakers = manager.speaker_infos();

// 3. Register watches for properties you want to display
for speaker_info in &speakers {
    let id = speaker_info.get_id();
    manager.register_watch(id, "volume");
    manager.register_watch(id, "mute");
    manager.register_watch(id, "playback_state");
    manager.register_watch(id, "current_track");
    manager.register_watch(id, "position");
}

// 4. Render loop
loop {
    // Non-blocking check for changes
    for event in manager.iter().try_iter() {
        // Update UI for the changed property
        match event.property_key {
            "volume" => {
                let vol = manager.get_property::<Volume>(&event.speaker_id);
                // Update volume display
            }
            "playback_state" => {
                let state = manager.get_property::<PlaybackState>(&event.speaker_id);
                // Update play/pause button
            }
            "current_track" => {
                let track = manager.get_property::<CurrentTrack>(&event.speaker_id);
                // Update now playing display
            }
            "position" => {
                let pos = manager.get_property::<Position>(&event.speaker_id);
                // Update progress bar
            }
            _ => {}
        }
    }

    // Render TUI frame
    terminal.draw(|f| {
        // Your rendering logic using the latest property values
    })?;

    // Handle input events (key presses, etc.)
    if event::poll(Duration::from_millis(50))? {
        if let Event::Key(key) = event::read()? {
            // Handle user input to execute operations
            // e.g., client.execute(ip, PlayOperation::build()?)?;
        }
    }
}
```

### Key Points:
1. **Use `sonos-state`** for reactive updates - it's designed for UIs
2. **Use `sonos-api`** for sending commands (play, pause, volume changes)
3. **Use `sonos-discovery`** once at startup to find devices
4. **Register watches** only for properties you're actively displaying
5. **Use `try_iter()`** in your render loop to avoid blocking
6. All operations share HTTP resources efficiently - no resource concerns
