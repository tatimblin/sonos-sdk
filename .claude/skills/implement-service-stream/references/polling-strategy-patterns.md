# Polling Strategy Patterns

## Overview

Polling strategies provide fallback event detection when UPnP events are blocked by firewalls. Each service needs a `ServicePoller` implementation in `sonos-stream/src/polling/strategies.rs`.

## The ServicePoller Trait

```rust
#[async_trait]
pub trait ServicePoller: Send + Sync {
    /// Poll device state and return serialized state string
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String>;

    /// Compare states and return detected changes
    async fn parse_for_changes(
        &self,
        old_state: &str,
        new_state: &str,
    ) -> Vec<StateChange>;

    /// Return the service type
    fn service_type(&self) -> Service;
}
```

## Implementation Pattern

### Step 1: Define the Poller Struct

```rust
/// Poller for NewService state
pub struct NewServicePoller;
```

### Step 2: Implement poll_state()

```rust
#[async_trait]
impl ServicePoller for NewServicePoller {
    async fn poll_state(
        &self,
        client: &SonosClient,
        pair: &SpeakerServicePair,
    ) -> PollingResult<String> {
        // 1. Build the operation
        let operation = new_service::get_state_operation()
            .build()
            .map_err(|e| PollingError::Network(format!("Build failed: {}", e)))?;

        // 2. Execute in blocking context (SonosClient is sync)
        let response = tokio::task::spawn_blocking({
            let client = client.clone();
            let ip = pair.ip.to_string();
            move || client.execute_enhanced(&ip, operation)
        })
        .await
        .map_err(|e| PollingError::Network(format!("Task join failed: {}", e)))?
        .map_err(|e| PollingError::Network(format!("API call failed: {}", e)))?;

        // 3. Serialize state for comparison
        let state = serde_json::json!({
            "field1": response.field1,
            "field2": response.field2,
            // Include all state fields
        });

        Ok(serde_json::to_string(&state).unwrap_or_default())
    }
```

### Step 3: Implement parse_for_changes()

```rust
    async fn parse_for_changes(
        &self,
        old_state: &str,
        new_state: &str,
    ) -> Vec<StateChange> {
        let mut changes = vec![];

        // Parse both states
        let old: serde_json::Value = serde_json::from_str(old_state).unwrap_or_default();
        let new: serde_json::Value = serde_json::from_str(new_state).unwrap_or_default();

        // Compare each relevant field
        if old["field1"] != new["field1"] {
            changes.push(StateChange {
                field: "field1".to_string(),
                old_value: old["field1"].to_string(),
                new_value: new["field1"].to_string(),
            });
        }

        if old["field2"] != new["field2"] {
            changes.push(StateChange {
                field: "field2".to_string(),
                old_value: old["field2"].to_string(),
                new_value: new["field2"].to_string(),
            });
        }

        changes
    }
```

### Step 4: Implement service_type()

```rust
    fn service_type(&self) -> Service {
        Service::NewService
    }
}
```

### Step 5: Register the Poller

In `DeviceStatePoller::new()`:

```rust
impl DeviceStatePoller {
    pub fn new() -> Self {
        let mut service_pollers: HashMap<Service, Box<dyn ServicePoller>> = HashMap::new();

        service_pollers.insert(Service::AVTransport, Box::new(AVTransportPoller));
        service_pollers.insert(Service::RenderingControl, Box::new(RenderingControlPoller));
        service_pollers.insert(Service::NewService, Box::new(NewServicePoller));

        Self {
            client: SonosClient::new(),
            service_pollers,
        }
    }
}
```

## Existing Poller Examples

### AVTransportPoller

Polls transport state (playback, position, track info):

```rust
async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String> {
    // Get transport info
    let transport_op = av_transport::get_transport_info_operation()
        .build()
        .map_err(|e| PollingError::Network(e.to_string()))?;

    let transport_info = tokio::task::spawn_blocking({
        let client = client.clone();
        let ip = pair.ip.to_string();
        move || client.execute_enhanced(&ip, transport_op)
    })
    .await??;

    // Get position info
    let position_op = av_transport::get_position_info_operation()
        .build()
        .map_err(|e| PollingError::Network(e.to_string()))?;

    let position_info = tokio::task::spawn_blocking({
        let client = client.clone();
        let ip = pair.ip.to_string();
        move || client.execute_enhanced(&ip, position_op)
    })
    .await??;

    // Combine into state
    let state = serde_json::json!({
        "transport_state": transport_info.current_transport_state,
        "transport_status": transport_info.current_transport_status,
        "current_speed": transport_info.current_speed,
        "track": position_info.track,
        "track_duration": position_info.track_duration,
        "rel_time": position_info.rel_time,
    });

    Ok(serde_json::to_string(&state).unwrap_or_default())
}
```

### RenderingControlPoller

Polls audio settings (volume, mute, EQ):

```rust
async fn poll_state(&self, client: &SonosClient, pair: &SpeakerServicePair) -> PollingResult<String> {
    // Get volume
    let volume_op = rendering_control::get_volume_operation("Master".to_string())
        .build()
        .map_err(|e| PollingError::Network(e.to_string()))?;

    let volume = tokio::task::spawn_blocking({
        let client = client.clone();
        let ip = pair.ip.to_string();
        move || client.execute_enhanced(&ip, volume_op)
    })
    .await??;

    // Get mute
    let mute_op = rendering_control::get_mute_operation("Master".to_string())
        .build()
        .map_err(|e| PollingError::Network(e.to_string()))?;

    let mute = tokio::task::spawn_blocking({
        let client = client.clone();
        let ip = pair.ip.to_string();
        move || client.execute_enhanced(&ip, mute_op)
    })
    .await??;

    let state = serde_json::json!({
        "master_volume": volume.current_volume,
        "master_mute": mute.current_mute,
    });

    Ok(serde_json::to_string(&state).unwrap_or_default())
}
```

## State Serialization Guidelines

### Consistent Ordering

Use `serde_json::json!()` macro to ensure consistent field ordering:

```rust
let state = serde_json::json!({
    "field1": value1,  // Always same order
    "field2": value2,
});
```

### Handle Missing Values

```rust
let state = serde_json::json!({
    "field1": response.field1.unwrap_or_default(),
    "field2": response.field2,  // null is fine
});
```

### Avoid Non-Deterministic Data

Don't include timestamps or sequence numbers that always change:

```rust
// BAD - will always show as "changed"
let state = serde_json::json!({
    "timestamp": SystemTime::now(),
});

// GOOD - only actual state
let state = serde_json::json!({
    "volume": volume,
    "mute": mute,
});
```

## Error Handling

### Network Errors

```rust
.map_err(|e| PollingError::Network(format!("Failed to poll: {}", e)))?
```

### Unsupported Services

If a service doesn't support polling:

```rust
async fn poll_state(&self, _client: &SonosClient, _pair: &SpeakerServicePair) -> PollingResult<String> {
    Err(PollingError::UnsupportedService {
        service: Service::NewService,
    })
}
```

## Testing Polling Strategies

### Unit Test for Change Detection

```rust
#[tokio::test]
async fn test_parse_changes() {
    let poller = NewServicePoller;

    let old = r#"{"field1": "value1", "field2": 42}"#;
    let new = r#"{"field1": "value2", "field2": 42}"#;

    let changes = poller.parse_for_changes(old, new).await;

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].field, "field1");
}
```

### Integration Test Script

Use the provided script:

```bash
python .claude/skills/implement-service-stream/scripts/test_polling.py <speaker_ip> NewService
```

## Checklist

- [ ] Poller struct defined
- [ ] poll_state() implemented with spawn_blocking for sync calls
- [ ] State serialization is deterministic
- [ ] parse_for_changes() compares all relevant fields
- [ ] service_type() returns correct Service variant
- [ ] Registered in DeviceStatePoller::new()
- [ ] Unit tests for change detection
- [ ] Integration tested with real speaker
