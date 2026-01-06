# Property Connector Implementation Guide

This guide explains how to implement property connectors for the sonos-state modular property system. Property connectors handle initialization and real-time updates for all Sonos device properties from UPnP services.

## 📋 Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Property Connector Lifecycle](#property-connector-lifecycle)
3. [Implementation Steps](#implementation-steps)
4. [Service-Specific Guides](#service-specific-guides)
5. [Data Structure Extensions](#data-structure-extensions)
6. [Testing and Validation](#testing-and-validation)
7. [Integration Examples](#integration-examples)

## 🏗️ Architecture Overview

### Core Components

```
PropertyManager
├── PropertyConnectorRegistry    (Manages all connectors)
├── SpeakerProperties           (Per-speaker property container)
├── GroupProperties             (Per-group property container)
└── SystemProperties            (System-wide property container)

Each PropertyConnector implements:
├── PropertyInitializer         (Queries initial values)
├── PropertyUpdater            (Processes real-time events)
└── PropertyConnector          (Converts to StateChange events)
```

### Property Scopes

Properties are categorized by scope:

- **`PerSpeaker`**: Individual speaker properties (volume, current track)
- **`PerGroup`**: Group/zone properties (group playback state)
- **`SystemWide`**: Household properties (alarms, music services)
- **`Hybrid`**: Can exist at multiple scopes (AVTransport)

### Initialization Types

Different services require different initialization approaches:

- **`IndividualQuery`**: Query each speaker separately
- **`CoordinatorQuery`**: Query only the group coordinator
- **`AnyDeviceQuery`**: Query any device (same result from all)
- **`SystemQuery`**: Query system-wide information
- **`TopologyDerived`**: Derived from topology discovery

## 🔄 Property Connector Lifecycle

### 1. Registration Phase
```rust
let registry = PropertyConnectorRegistry::new()
    .with_rendering_control(Box::new(RenderingControlConnector::new()))
    .with_av_transport(Box::new(AVTransportConnector::new()));

let property_manager = PropertyManager::new(registry);
```

### 2. Initialization Phase
```rust
// For each discovered speaker/group
property_manager.initialize_all_properties(&speakers, &groups)?;

// Calls appropriate initialization method based on scope:
// - PropertyInitializer::initialize_for_speaker()
// - PropertyInitializer::initialize_for_group()
// - PropertyInitializer::initialize_system_wide()
```

### 3. Real-time Update Phase
```rust
// For each incoming event
let changes = property_manager.handle_event(enriched_event)?;

// Routes to appropriate updater method:
// - PropertyUpdater::update_speaker_property()
// - PropertyUpdater::update_group_property()
// - PropertyUpdater::update_system_property()
```

## 📝 Implementation Steps

### Step 1: Define Property Structure

Create a struct to hold all properties for your service:

```rust
// src/properties/your_service.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct YourServiceProperties {
    // Core properties
    pub primary_property: Option<String>,
    pub numeric_property: Option<u32>,
    pub boolean_property: Option<bool>,

    // Collections
    pub property_list: Vec<String>,
    pub property_map: HashMap<String, String>,

    // Capabilities (set during initialization)
    pub supports_feature_x: bool,
    pub supports_feature_y: bool,
}

impl YourServiceProperties {
    pub fn new_basic() -> Self {
        Self {
            primary_property: Some("default".to_string()),
            ..Default::default()
        }
    }

    /// Update a property, returning whether it changed
    pub fn update_primary(&mut self, new_value: String) -> bool {
        let changed = self.primary_property.as_ref() != Some(&new_value);
        self.primary_property = Some(new_value);
        changed
    }
}
```

### Step 2: Create the Connector

Implement the connector with all three traits:

```rust
pub struct YourServiceConnector {
    client: SonosClient,
}

impl YourServiceConnector {
    pub fn new() -> Self {
        Self {
            client: SonosClient::new(),
        }
    }

    /// Query all properties from a device
    fn query_full_properties(&self, device_ip: &str) -> Result<YourServiceProperties> {
        let mut properties = YourServiceProperties::default();

        // Query individual properties using sonos-api operations
        if let Ok(response) = self.query_specific_operation(device_ip) {
            properties.primary_property = Some(response.value);
            properties.supports_feature_x = true;
        }

        Ok(properties)
    }

    fn query_specific_operation(&self, device_ip: &str) -> Result<OperationResponse> {
        let operation = your_service_operation()
            .build()
            .map_err(|e| StateError::Init(format!("Build failed: {}", e)))?;

        self.client
            .execute_enhanced(device_ip, operation)
            .map_err(|e| StateError::Init(format!("Query failed: {}", e)))
    }
}
```

### Step 3: Implement PropertyInitializer

```rust
impl PropertyInitializer for YourServiceConnector {
    type Property = YourServiceProperties;

    fn service() -> Service { Service::YourService }
    fn scope() -> PropertyScope { PropertyScope::PerSpeaker }
    fn initialization_type() -> InitializationType { InitializationType::IndividualQuery }

    fn initialize_for_speaker(&self, speaker: &Speaker) -> Result<Option<Self::Property>> {
        println!("🔧 Querying {} properties for {}", Self::name(), speaker.name);

        match self.query_full_properties(&speaker.ip_address.to_string()) {
            Ok(properties) => {
                println!("   ✅ Success: {:?}", properties.primary_property);
                Ok(Some(properties))
            }
            Err(e) => {
                println!("   ⚠️  Failed: {}", e);
                // Return basic properties rather than failing completely
                Ok(Some(YourServiceProperties::new_basic()))
            }
        }
    }

    fn initialize_for_group(&self, group: &Group, speakers: &[Speaker]) -> Result<Option<Self::Property>> {
        // Only implement if your service has group-level properties
        Ok(None)
    }

    fn initialize_system_wide(&self, any_speaker: &Speaker) -> Result<Option<Self::Property>> {
        // Only implement if your service has system-wide properties
        Ok(None)
    }
}
```

### Step 4: Implement PropertyUpdater

```rust
impl PropertyUpdater for YourServiceConnector {
    type Property = YourServiceProperties;

    fn can_handle_event(&self, event: &EnrichedEvent) -> bool {
        event.service == Service::YourService
    }

    fn update_speaker_property(
        &self,
        speaker_id: &SpeakerId,
        current: Option<&Self::Property>,
        event: &EnrichedEvent
    ) -> Result<Option<PropertyChange<Self::Property>>> {
        // Extract event data specific to your service
        let EventData::YourServiceEvent(ref your_event) = event.event_data else {
            return Ok(None);
        };

        let mut new_properties = current.cloned().unwrap_or_default();
        let mut changed = false;

        // Update properties from event data
        if let Some(ref new_value) = your_event.some_field {
            if new_properties.update_primary(new_value.clone()) {
                changed = true;
            }
        }

        if let Some(ref numeric_value) = your_event.numeric_field {
            if let Ok(parsed) = numeric_value.parse::<u32>() {
                if new_properties.numeric_property != Some(parsed) {
                    new_properties.numeric_property = Some(parsed);
                    changed = true;
                }
            }
        }

        if changed {
            Ok(Some(PropertyChange::new(current.cloned(), new_properties)))
        } else {
            Ok(None)
        }
    }

    fn update_group_property(&self, group_id: &GroupId, current: Option<&Self::Property>, event: &EnrichedEvent) -> Result<Option<PropertyChange<Self::Property>>> {
        // Only implement if your service has group-level properties
        Ok(None)
    }

    fn update_system_property(&self, current: Option<&Self::Property>, event: &EnrichedEvent) -> Result<Option<PropertyChange<Self::Property>>> {
        // Only implement if your service has system-wide properties
        Ok(None)
    }
}
```

### Step 5: Implement PropertyConnector

```rust
impl PropertyConnector for YourServiceConnector {
    fn name() -> &'static str { "YourService" }

    fn to_state_change(
        &self,
        scope: &PropertyScope,
        id: &str,
        change: PropertyChange<Self::Property>
    ) -> Vec<StateChange> {
        let mut changes = Vec::new();

        if let Some(ref old_props) = change.old_value {
            let new_props = &change.new_value;

            // Generate specific StateChange events for important property changes
            if old_props.primary_property != new_props.primary_property {
                changes.push(StateChange::PropertiesChanged {
                    target_id: id.to_string(),
                    property_type: "your_service_primary".to_string(),
                    details: new_props.primary_property
                        .as_deref()
                        .unwrap_or("None")
                        .to_string(),
                });
            }

            // Add more specific state changes as needed
            if old_props.numeric_property != new_props.numeric_property {
                changes.push(StateChange::PropertiesChanged {
                    target_id: id.to_string(),
                    property_type: "your_service_numeric".to_string(),
                    details: new_props.numeric_property
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "None".to_string()),
                });
            }
        }

        changes
    }
}
```

### Step 6: Register the Connector

Add registration method to `PropertyConnectorRegistry`:

```rust
impl PropertyConnectorRegistry {
    pub fn with_your_service(mut self, connector: Box<dyn PropertyConnector<Property = YourServiceProperties>>) -> Self {
        self.your_service = Some(connector);
        self
    }
}
```

## 🎯 Service-Specific Guides

### AVTransport Service

**Properties to track:**
- Transport state (PLAYING, PAUSED, STOPPED)
- Current track information (URI, metadata, duration, position)
- Queue information (length, play mode)
- Next track information

**Key operations:**
- `GetTransportInfo` - Current playback state
- `GetPositionInfo` - Track position and metadata
- `GetMediaInfo` - Queue and playlist information

**Event handling:**
- Transport state changes
- Track changes (new song starts)
- Position updates (during playback)
- Queue modifications

**Implementation notes:**
- AVTransport can be per-speaker or per-group depending on grouping
- Grouped speakers share transport state through coordinator
- Position updates are frequent - consider throttling

### RenderingControl Service ✅ (Already Implemented)

**Properties tracked:**
- Master volume, L/R channel volumes
- Mute states for all channels
- EQ settings (bass, treble, balance, loudness)
- Volume capabilities and limits

### DeviceProperties Service

**Properties to track:**
- Zone name, icon, configuration
- Hardware info (model, serial, software version)
- Network status (wireless mode, signal strength)
- Device capabilities and features

**Key operations:**
- `GetZoneAttributes` - Name and configuration
- `GetZoneInfo` - Device information
- `GetNetworkInfo` - Connection status
- `GetBatteryInfo` - For battery devices

### ZoneGroupTopology Service

**Properties to track:**
- Zone group structure and membership
- Coordinator assignments
- Vanished/disappeared devices
- Topology version and change tracking

**Key operations:**
- `GetZoneGroupState` - Complete topology

**Event handling:**
- Group creation/dissolution
- Membership changes
- Coordinator changes
- Device appearance/disappearance

### AlarmClock Service

**Properties to track:**
- Active alarms (schedule, target room, content)
- Sleep timers (remaining time, fade settings)
- Daily index refresh settings

**Key operations:**
- `ListAlarms` - All configured alarms
- `GetAlarmClock` - Alarm schedule details
- `GetSleepTimers` - Active sleep timers

### AudioIn Service

**Properties to track:**
- Line-in connection status
- Input level/gain settings
- Auto-play configuration
- Source name/description

**Key operations:**
- `GetAudioInputAttributes` - Input configuration
- `GetLineInLevel` - Current input level

**Device support:**
- Only available on speakers with line-in (PLAY:5, Connect, Port)

### MusicServices Service

**Properties to track:**
- Available streaming services
- Configured user accounts
- Service capabilities and features
- Authentication status

**Key operations:**
- `GetSessionId` - Service session management
- `ListAvailableServices` - Available streaming options

## 🔧 Data Structure Extensions

### Adding New Fields to Speaker

To add new fields to the `Speaker` struct:

```rust
// In src/model.rs, add to Speaker struct:
pub struct Speaker {
    // ... existing fields ...

    // Your new fields
    pub your_new_field: Option<String>,
    pub your_capability: bool,
}

// In speaker initialization code:
let speaker = Speaker {
    // ... existing initialization ...
    your_new_field: connector.query_your_field(&device)?,
    your_capability: connector.supports_your_feature(&device),
};
```

### Adding New Fields to Group

```rust
// In src/model.rs, add to Group struct:
pub struct Group {
    // ... existing fields ...

    // Your new group-specific fields
    pub group_specific_property: Option<String>,
}
```

### Adding New StateChange Variants

```rust
// In src/model.rs, add to StateChange enum:
pub enum StateChange {
    // ... existing variants ...

    // Your new state change types
    YourPropertyChanged {
        speaker_id: SpeakerId,
        old_value: String,
        new_value: String,
    },
    YourFeatureEnabled {
        target_id: String,
        enabled: bool,
    },
}
```

### Extending Property Containers

```rust
// In src/properties/mod.rs, add to SpeakerProperties:
pub struct SpeakerProperties {
    // ... existing properties ...
    pub your_service: Option<your_service::YourServiceProperties>,
}

// Add corresponding fields to GroupProperties and SystemProperties as needed
```

## 🧪 Testing and Validation

### Unit Tests

Create comprehensive tests for your connector:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_creation() {
        let props = YourServiceProperties::new_basic();
        assert_eq!(props.primary_property, Some("default".to_string()));
    }

    #[test]
    fn test_property_updates() {
        let mut props = YourServiceProperties::default();
        assert!(props.update_primary("new_value".to_string()));
        assert!(!props.update_primary("new_value".to_string())); // Same value
    }

    #[test]
    fn test_event_conversion() {
        // Test converting sonos-stream events to property changes
        let connector = YourServiceConnector::new();
        let event = create_test_event();

        let change = connector.update_speaker_property(
            &SpeakerId::new("test"),
            None,
            &event
        ).unwrap();

        assert!(change.is_some());
    }

    #[test]
    fn test_state_change_generation() {
        let connector = YourServiceConnector::new();
        let change = PropertyChange::new(
            Some(YourServiceProperties::default()),
            YourServiceProperties::new_basic()
        );

        let state_changes = connector.to_state_change(
            &PropertyScope::PerSpeaker,
            "test_id",
            change
        );

        assert!(!state_changes.is_empty());
    }
}
```

### Integration Tests

Create example programs to validate end-to-end functionality:

```rust
// examples/your_service_example.rs
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize with your connector
    let registry = PropertyConnectorRegistry::new()
        .with_your_service(Box::new(YourServiceConnector::new()));

    let mut property_manager = PropertyManager::new(registry);

    // Test initialization
    let speakers = discover_speakers().await?;
    let report = property_manager.initialize_all_properties(&speakers, &[])?;

    println!("Initialization: {}", report.summary());

    // Test real-time updates
    let mut event_stream = setup_event_stream().await?;

    while let Some(event) = event_stream.next().await {
        let changes = property_manager.handle_event(event)?;

        for change in changes {
            println!("Property change: {}", change.description());
        }
    }

    Ok(())
}
```

## 🔌 Integration Examples

### Complete Connector Registration

```rust
use sonos_state::properties::*;

// Create registry with all connectors
let registry = PropertyConnectorRegistry::new()
    // Core audio properties (implemented)
    .with_rendering_control(Box::new(rendering_control::RenderingControlConnector::new()))

    // Playback and transport (implement next)
    .with_av_transport(Box::new(av_transport::AVTransportConnector::new()))

    // Device information
    .with_device_properties(Box::new(device_properties::DevicePropertiesConnector::new()))

    // Network topology
    .with_zone_group_topology(Box::new(zone_group_topology::ZoneGroupTopologyConnector::new()))

    // System features
    .with_alarm_clock(Box::new(alarm_clock::AlarmClockConnector::new()))
    .with_audio_in(Box::new(audio_in::AudioInConnector::new()))
    .with_music_services(Box::new(music_services::MusicServicesConnector::new()));

// Create property manager
let property_manager = PropertyManager::new(registry);
```

### Using Properties in Applications

```rust
// Get comprehensive speaker information
let speaker_props = property_manager.get_speaker_properties(&speaker_id)?;

// Access different service properties
if let Some(ref rendering) = speaker_props.rendering {
    println!("Volume: {}%, Bass: {}",
             rendering.master_volume.unwrap_or(0),
             rendering.bass.unwrap_or(0));
}

if let Some(ref av_transport) = speaker_props.av_transport {
    println!("Playing: {}", av_transport.is_playing());
    if let Some(track_info) = av_transport.parse_track_info() {
        println!("Track: {} - {}", track_info.artist.unwrap_or_default(),
                                  track_info.title.unwrap_or_default());
    }
}

if let Some(ref device) = speaker_props.device_properties {
    println!("Model: {}, Wireless: {}",
             device.model_name.as_deref().unwrap_or("Unknown"),
             device.is_wireless());
}
```

### Custom StateChange Handling

```rust
// Process all types of state changes
for change in change_receiver {
    match change {
        // Built-in changes
        StateChange::VolumeChanged { speaker_id, new_volume, .. } => {
            update_volume_slider(&speaker_id, new_volume);
        }
        StateChange::TrackChanged { speaker_id, new_track } => {
            update_now_playing(&speaker_id, new_track);
        }

        // Generic property changes
        StateChange::PropertiesChanged { target_id, property_type, details } => {
            match property_type.as_str() {
                "rendering_eq" => update_eq_display(&target_id, &details),
                "your_service_primary" => handle_your_property_change(&target_id, &details),
                _ => log_unknown_property_change(&property_type, &details),
            }
        }

        _ => {}
    }
}
```

## 📚 Best Practices

### Error Handling

1. **Graceful Degradation**: Return basic properties instead of failing completely
2. **Specific Error Messages**: Include context about which operation failed
3. **Capability Detection**: Check device capabilities before querying unsupported features
4. **Timeout Handling**: Set reasonable timeouts for network operations

### Performance Optimization

1. **Batch Operations**: Query multiple properties in single calls when possible
2. **Cache Results**: Cache infrequently changing properties (device info, capabilities)
3. **Event Throttling**: Throttle high-frequency events (position updates)
4. **Selective Updates**: Only update properties that actually changed

### Logging and Debugging

1. **Structured Logging**: Use consistent log levels and formatting
2. **Property Tracing**: Log property changes for debugging
3. **Error Context**: Include speaker ID and operation context in error messages
4. **Performance Metrics**: Track initialization times and event processing rates

### Future Extensibility

1. **Version Compatibility**: Handle different Sonos software versions gracefully
2. **Optional Properties**: Use `Option<T>` for properties that may not be available
3. **Capability Flags**: Track what each device supports
4. **Backward Compatibility**: Maintain compatibility when adding new properties

This guide provides the foundation for implementing comprehensive property tracking across all Sonos services. Each connector follows the same patterns while handling service-specific details appropriately.