# Volume Tracking: How RenderingControl Events Update Speaker State

This document explains how RenderingControl events from `sonos-stream` flow through `sonos-state` to update volume levels for each individual speaker in the Sonos network.

## 🏗️ Architecture Overview

```
Sonos Device 📱    →    sonos-stream 📡    →    sonos-state 📊    →    Your App 🎵
   Volume: 75            EnrichedEvent           StateChange            Volume UI
   IP: 192.168.1.100     speaker_ip: 100         speaker_id: RINCON_   Update Display
```

## 🔄 Event Flow Pipeline

### 1. **Event Generation** (Sonos Device)
- User changes volume on Sonos device or app
- Device sends UPnP NOTIFY message to callback server
- Contains RenderingControl service data with volume levels

### 2. **Event Capture** (sonos-stream)
- `EventBroker` receives UPnP notification via `callback-server`
- Parses XML payload into `RenderingControlEvent`
- Creates `EnrichedEvent` with speaker IP and timestamp

### 3. **Event Conversion** (SonosStreamEventReceiver)
```rust
// From sonos-stream format
RenderingControlEvent {
    master_volume: Some("75"),  // String format
    master_mute: Some("false"),
    lf_volume: Some("70"),      // Left Front
    rf_volume: Some("80"),      // Right Front
    // ... other channels
}

// To sonos-state format
StateEventPayload::Rendering {
    master_volume: Some(75),    // Parsed to u8
    master_mute: Some(false),   // Parsed to bool
}
```

### 4. **Speaker Resolution** (StateProcessor)
```rust
// processor.rs:108
if let Some(speaker_id) = self.ip_to_speaker.get(&event.speaker_ip).cloned() {
    if let Some(volume) = master_volume {
        if let Some(change) = self.cache.update_volume(&speaker_id, volume) {
            changes.push(change);
        }
    }
}
```

**Key Points:**
- `ip_to_speaker` HashMap maps `IpAddr → SpeakerId`
- Built during initialization from device topology
- Rebuilt when topology changes (speakers added/removed)

### 5. **State Update** (StateCache)
```rust
// state_cache.rs:88
pub fn update_volume(&self, id: &SpeakerId, volume: u8) -> Option<StateChange> {
    let mut speakers = self.speakers.write().ok()?;
    let state = speakers.get_mut(id)?;
    let old_volume = state.volume;

    if old_volume != volume {
        state.volume = volume;
        Some(StateChange::VolumeChanged {
            speaker_id: id.clone(),
            old_volume,
            new_volume: volume,
        })
    } else {
        None  // No change detected
    }
}
```

**Features:**
- ✅ **Thread-safe**: Uses `Arc<RwLock<HashMap<SpeakerId, SpeakerState>>>`
- ✅ **Change detection**: Only emits events when volume actually changes
- ✅ **Per-speaker state**: Each speaker maintains independent volume
- ✅ **Atomic updates**: Volume changes are isolated per speaker

## 🎯 Multi-Speaker Scenarios

### Independent Speaker Volumes
```rust
// Each speaker maintains its own volume state
SpeakerState {
    speaker: Speaker {
        id: SpeakerId("RINCON_123"),
        ip_address: "192.168.1.100"
    },
    volume: 75,        // Independent volume
    muted: false,
    // ...
}

SpeakerState {
    speaker: Speaker {
        id: SpeakerId("RINCON_456"),
        ip_address: "192.168.1.101"
    },
    volume: 50,        // Different volume
    muted: true,
    // ...
}
```

### Group Volume Coordination
When speakers are grouped, Sonos typically:
1. **Coordinator** sends volume changes to all group members
2. Each **member** receives its own RenderingControl event
3. **sonos-state** processes each event independently
4. Results in multiple `StateChange::VolumeChanged` events

```
Group Volume Change (via coordinator):
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Coordinator   │    │    Member 1     │    │    Member 2     │
│ RINCON_123 @100 │    │ RINCON_456 @101 │    │ RINCON_789 @102 │
│   Vol: 60 → 80  │    │   Vol: 45 → 60  │    │   Vol: 70 → 93  │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         ↓                      ↓                      ↓
    StateChange::           StateChange::           StateChange::
    VolumeChanged          VolumeChanged          VolumeChanged
```

## 🔍 Advanced Features

### Stereo Pair Support
The RenderingControl event includes separate channels:
```rust
RenderingControlEvent {
    master_volume: Some("75"),   // Overall volume
    lf_volume: Some("70"),       // Left Front speaker
    rf_volume: Some("80"),       // Right Front speaker
    lf_mute: Some("false"),
    rf_mute: Some("false"),
    // ...
}
```

**Current Implementation:**
- ✅ Captures all channel data from events
- ⚠️  Only processes `master_volume` and `master_mute`
- 🔮 **Future Enhancement**: Could support per-channel volume tracking

### Bass, Treble, Balance
RenderingControl events also include:
```rust
bass: Some("5"),           // -10 to +10
treble: Some("-2"),        // -10 to +10
balance: Some("0"),        // -100 to +100 (L/R balance)
loudness: Some("true"),    // Loudness compensation
```

**Current Implementation:**
- ✅ Events contain this data
- ❌ Not currently tracked in sonos-state
- 🔮 **Future Enhancement**: Could extend `SpeakerState` to include EQ settings

## 🚀 Usage Examples

### Basic Volume Monitoring
```rust
use sonos_state::{StateManager, StateChange};

let change_receiver = state_manager.take_change_receiver().unwrap();

for change in change_receiver {
    match change {
        StateChange::VolumeChanged { speaker_id, old_volume, new_volume } => {
            println!("🔊 {} volume: {} → {}",
                     speaker_id.as_str(), old_volume, new_volume);
        }
        _ => {}
    }
}
```

### Multi-Speaker Volume Display
```rust
let snapshot = state_manager.snapshot();
for speaker_state in snapshot.speakers() {
    println!("{}: Vol {}%{}",
             speaker_state.speaker.name,
             speaker_state.volume,
             if speaker_state.muted { " (Muted)" } else { "" });
}
```

### Group-Aware Volume Control
```rust
// Find all speakers in same group as target speaker
let target_group_id = snapshot.get_speaker(&speaker_id)?.group_id;
let group_speakers = snapshot.speakers()
    .filter(|s| s.group_id == target_group_id)
    .collect::<Vec<_>>();

println!("Group has {} speakers with volumes: {:?}",
         group_speakers.len(),
         group_speakers.iter().map(|s| s.volume).collect::<Vec<_>>());
```

## 🔧 Implementation Details

### Speaker IP Mapping
```rust
// processor.rs:28
pub fn rebuild_ip_mapping(&mut self) {
    self.ip_to_speaker.clear();
    for speaker_state in self.cache.get_all_speakers() {
        self.ip_to_speaker.insert(
            speaker_state.speaker.ip_address,
            speaker_state.speaker.id.clone(),
        );
    }
}
```

**When IP Mapping is Rebuilt:**
- ✅ During StateProcessor initialization
- ✅ When StateManager starts processing
- ⚠️  **Not** automatically when topology changes
- 🔮 **Enhancement**: Could trigger rebuild on topology events

### Volume Change Detection
```rust
// state_cache.rs:93
if old_volume != volume {
    state.volume = volume;
    Some(StateChange::VolumeChanged {
        speaker_id: id.clone(),
        old_volume,
        new_volume: volume,
    })
} else {
    None
}
```

**Benefits:**
- Prevents duplicate events for same volume
- Reduces noise in event stream
- Maintains clean change history

## 🎯 Performance Characteristics

### Memory Usage
- **Per Speaker**: ~200 bytes (SpeakerState + HashMap entry)
- **IP Mapping**: ~50 bytes per speaker (IpAddr + SpeakerId)
- **Concurrent Access**: RwLock allows multiple readers, single writer

### Event Processing Speed
- **Volume Update**: O(1) HashMap lookup + O(1) state update
- **Change Detection**: Simple equality comparison
- **Thread Safety**: RwLock contention minimal for volume updates

### Network Efficiency
- **UPnP Events**: Only sent when volume actually changes
- **Polling Fallback**: 30-second intervals if events fail
- **Firewall Handling**: Automatic detection and fallback

## 🐛 Known Limitations & Future Enhancements

### Current Limitations
1. **Single Volume Channel**: Only tracks `master_volume`, ignores LF/RF
2. **No EQ Tracking**: Bass/treble/balance not stored in state
3. **Manual IP Mapping**: Doesn't auto-rebuild on topology changes

### Potential Enhancements
1. **Multi-Channel Volume**: Support LF/RF independent volumes
2. **EQ State Management**: Track bass, treble, balance, loudness
3. **Volume Ramping**: Detect and smooth volume transitions
4. **History Tracking**: Maintain volume change history per speaker

### Example Enhancement: Multi-Channel Support
```rust
// Potential future SpeakerState structure
#[derive(Debug, Clone)]
pub struct SpeakerState {
    // Current fields...
    pub volume: u8,
    pub muted: bool,

    // Enhanced volume tracking
    pub channel_volumes: HashMap<VolumeChannel, u8>,
    pub channel_mutes: HashMap<VolumeChannel, bool>,
    pub eq_settings: EqSettings,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum VolumeChannel {
    Master,
    LeftFront,
    RightFront,
    LeftRear,
    RightRear,
    Subwoofer,
}
```

## 🎵 Real-World Usage

### Home Theater Setup
```
Living Room Sonos System:
├── Sonos Arc (192.168.1.100) - Master Vol: 75%
├── Sonos Sub (192.168.1.101) - Linked to Arc
└── Sonos One SL × 2 (192.168.1.102, .103) - Surrounds

Volume Changes:
• Arc master volume affects entire setup
• Each device receives separate RenderingControl event
• sonos-state tracks all 4 devices independently
• UI can show unified or per-device volumes
```

### Multi-Room Audio
```
Whole House Audio:
├── Kitchen: Sonos One (Vol: 60%)
├── Bedroom: Sonos One (Vol: 40%)
├── Living Room: Sonos Arc + Sub (Vol: 75%)
└── Office: Sonos Play:5 (Vol: 30%)

Group Scenarios:
• All rooms grouped → volume changes propagate to all
• Individual control → only target room volume changes
• sonos-state correctly handles both scenarios
```

This architecture provides robust, scalable volume tracking that works seamlessly across all Sonos device types and grouping configurations.