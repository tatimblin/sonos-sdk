# Plan: Sync-First Integration of sonos-state + sonos-event-manager

## Overview

Make both `sonos-event-manager` and `sonos-state` sync-first, hiding async internals in background threads. Users get a fully synchronous API with no tokio exposure.

## Current State

| Crate | API | Internal |
|-------|-----|----------|
| sonos-state | Sync | Sync (no events yet) |
| sonos-event-manager | **Async** | Async (tokio) |
| sonos-stream | Async | Async (tokio) |

## Target State

| Crate | API | Internal |
|-------|-----|----------|
| sonos-state | Sync | Sync + background thread |
| sonos-event-manager | **Sync** | Background thread with tokio runtime |
| sonos-stream | Async (unchanged) | Async (unchanged) |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     User Code (fully sync)                      │
│                                                                 │
│   let system = SonosSystem::new()?;      // Sync                │
│   speaker.volume.watch()?;                // Sync               │
│   for event in system.iter() { ... }      // Blocking           │
└──────────────────────────┬──────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────────┐
│                   sonos-state (sync API)                        │
│                                                                 │
│   StateManager::new()                                           │
│   speaker.volume.watch()                                        │
│   manager.iter()                                                │
│                                                                 │
│   Uses: sonos-event-manager (sync API)                          │
└──────────────────────────┬──────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────────┐
│              sonos-event-manager (sync API)                     │
│                                                                 │
│   SonosEventManager::new()              // Sync                 │
│   manager.add_devices(devices)          // Sync                 │
│   manager.ensure_service_subscribed()   // Sync                 │
│   manager.iter() -> impl Iterator       // Blocking             │
│                                                                 │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │           Background Thread (owns tokio runtime)        │   │
│   │                                                         │   │
│   │   EventBroker (async)                                   │   │
│   │   Subscription management                               │   │
│   │   Event processing                                      │   │
│   └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────────┐
│                sonos-stream (async, unchanged)                  │
│                                                                 │
│   EventBroker, EventIterator, etc.                              │
└─────────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Refactor sonos-event-manager to Sync-First

### 1.1 New Sync API Design

```rust
// sonos-event-manager/src/manager.rs

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{mpsc, Arc, RwLock};
use std::thread::{self, JoinHandle};

use sonos_api::Service;
use sonos_discovery::Device;
use sonos_stream::events::EnrichedEvent;

/// Command sent to background worker
enum Command {
    AddDevices(Vec<Device>),
    Subscribe { ip: IpAddr, service: Service },
    Unsubscribe { ip: IpAddr, service: Service },
    Shutdown,
}

/// Sync-first event manager
pub struct SonosEventManager {
    /// Send commands to background worker
    command_tx: mpsc::Sender<Command>,

    /// Receive events from background worker
    event_rx: Arc<std::sync::Mutex<mpsc::Receiver<EnrichedEvent>>>,

    /// Device info cache (sync access)
    devices: Arc<RwLock<HashMap<IpAddr, Device>>>,

    /// Service subscription ref counts (sync access)
    service_refs: Arc<RwLock<HashMap<(IpAddr, Service), usize>>>,

    /// Background worker handle
    _worker: JoinHandle<()>,
}
```

### 1.2 Sync Constructor

```rust
impl SonosEventManager {
    /// Create new event manager (sync)
    pub fn new() -> Result<Self> {
        Self::with_config(Default::default())
    }

    pub fn with_config(config: BrokerConfig) -> Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let devices = Arc::new(RwLock::new(HashMap::new()));
        let service_refs = Arc::new(RwLock::new(HashMap::new()));

        // Spawn background worker with its own tokio runtime
        let worker = spawn_event_worker(config, command_rx, event_tx)?;

        Ok(Self {
            command_tx,
            event_rx: Arc::new(std::sync::Mutex::new(event_rx)),
            devices,
            service_refs,
            _worker: worker,
        })
    }
}
```

### 1.3 Sync Methods

```rust
impl SonosEventManager {
    /// Add discovered devices (sync)
    pub fn add_devices(&self, devices: Vec<Device>) -> Result<()> {
        // Update local cache
        {
            let mut device_map = self.devices.write().map_err(|_| Error::LockPoisoned)?;
            for device in &devices {
                let ip: IpAddr = device.ip_address.parse()?;
                device_map.insert(ip, device.clone());
            }
        }

        // Send to worker for EventBroker registration
        self.command_tx.send(Command::AddDevices(devices))?;
        Ok(())
    }

    /// Ensure service is subscribed (sync, ref counted)
    pub fn ensure_service_subscribed(&self, ip: IpAddr, service: Service) -> Result<()> {
        let should_subscribe = {
            let mut refs = self.service_refs.write().map_err(|_| Error::LockPoisoned)?;
            let count = refs.entry((ip, service)).or_insert(0);
            let was_zero = *count == 0;
            *count += 1;
            was_zero
        };

        if should_subscribe {
            self.command_tx.send(Command::Subscribe { ip, service })?;
        }
        Ok(())
    }

    /// Release service subscription (sync, ref counted)
    pub fn release_service_subscription(&self, ip: IpAddr, service: Service) -> Result<()> {
        let should_unsubscribe = {
            let mut refs = self.service_refs.write().map_err(|_| Error::LockPoisoned)?;
            if let Some(count) = refs.get_mut(&(ip, service)) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    refs.remove(&(ip, service));
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_unsubscribe {
            self.command_tx.send(Command::Unsubscribe { ip, service })?;
        }
        Ok(())
    }

    /// Get devices (sync)
    pub fn devices(&self) -> Vec<Device> {
        self.devices.read()
            .map(|d| d.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Blocking iterator over events
    pub fn iter(&self) -> EventManagerIterator {
        EventManagerIterator {
            rx: Arc::clone(&self.event_rx),
        }
    }
}
```

### 1.4 Background Worker

```rust
// sonos-event-manager/src/worker.rs

fn spawn_event_worker(
    config: BrokerConfig,
    command_rx: mpsc::Receiver<Command>,
    event_tx: mpsc::Sender<EnrichedEvent>,
) -> Result<JoinHandle<()>> {
    // Spawn thread with its own tokio runtime
    let handle = thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        rt.block_on(async {
            // Create EventBroker (async)
            let mut broker = match EventBroker::new(config).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!("Failed to create EventBroker: {}", e);
                    return;
                }
            };

            // Get event iterator
            let mut events = match broker.event_iterator() {
                Ok(iter) => iter,
                Err(e) => {
                    tracing::error!("Failed to get event iterator: {}", e);
                    return;
                }
            };

            loop {
                tokio::select! {
                    // Forward events to sync channel
                    event = events.next_async() => {
                        match event {
                            Some(e) => {
                                if event_tx.send(e).is_err() {
                                    break; // Receiver dropped
                                }
                            }
                            None => break, // Event stream ended
                        }
                    }

                    // Process commands (poll periodically)
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {
                        while let Ok(cmd) = command_rx.try_recv() {
                            match cmd {
                                Command::Subscribe { ip, service } => {
                                    if let Err(e) = broker.register_speaker_service(ip, service).await {
                                        tracing::warn!("Subscribe failed for {}:{:?}: {}", ip, service, e);
                                    }
                                }
                                Command::Unsubscribe { ip, service } => {
                                    if let Err(e) = broker.unregister_speaker_service(ip, service).await {
                                        tracing::warn!("Unsubscribe failed for {}:{:?}: {}", ip, service, e);
                                    }
                                }
                                Command::AddDevices(_) => {
                                    // Devices already cached in main struct
                                    // EventBroker doesn't need device list
                                }
                                Command::Shutdown => return,
                            }
                        }
                    }
                }
            }
        });
    });

    Ok(handle)
}
```

### 1.5 Sync Iterator

```rust
// sonos-event-manager/src/iter.rs

pub struct EventManagerIterator {
    rx: Arc<std::sync::Mutex<mpsc::Receiver<EnrichedEvent>>>,
}

impl Iterator for EventManagerIterator {
    type Item = EnrichedEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.lock().ok()?.recv().ok()
    }
}

impl EventManagerIterator {
    pub fn try_recv(&self) -> Option<EnrichedEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<EnrichedEvent> {
        self.rx.lock().ok()?.recv_timeout(timeout).ok()
    }
}
```

### Files to Modify (sonos-event-manager)

| File | Action |
|------|--------|
| `src/manager.rs` | Rewrite with sync API |
| `src/worker.rs` | New - background thread |
| `src/iter.rs` | New - sync iterator |
| `src/lib.rs` | Update exports |
| `Cargo.toml` | Keep tokio (internal only) |

---

## Phase 2: Add Event Decoder to sonos-state

Create decoder to convert `EnrichedEvent` → typed properties.

```rust
// sonos-state/src/decoder.rs

use sonos_stream::events::{EnrichedEvent, EventData,
    AVTransportEvent, RenderingControlEvent};
use crate::property::*;
use crate::model::SpeakerId;

pub struct DecodedChanges {
    pub speaker_id: SpeakerId,
    pub changes: Vec<PropertyChange>,
}

pub enum PropertyChange {
    Volume(Volume),
    Mute(Mute),
    Bass(Bass),
    Treble(Treble),
    Loudness(Loudness),
    PlaybackState(PlaybackState),
    Position(Position),
    CurrentTrack(CurrentTrack),
    GroupMembership(GroupMembership),
}

pub fn decode_event(event: &EnrichedEvent, speaker_id: SpeakerId) -> DecodedChanges {
    let changes = match &event.event_data {
        EventData::RenderingControl(rc) => decode_rendering_control(rc),
        EventData::AVTransport(avt) => decode_av_transport(avt),
        EventData::ZoneGroupTopology(zgt) => decode_topology(zgt),
        EventData::DeviceProperties(_) => vec![],
    };
    DecodedChanges { speaker_id, changes }
}

fn decode_rendering_control(event: &RenderingControlEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    if let Some(vol) = event.master_volume {
        changes.push(PropertyChange::Volume(Volume(vol as u8)));
    }
    if let Some(mute) = event.master_mute {
        changes.push(PropertyChange::Mute(Mute(mute)));
    }
    if let Some(bass) = event.bass {
        changes.push(PropertyChange::Bass(Bass(bass as i8)));
    }
    if let Some(treble) = event.treble {
        changes.push(PropertyChange::Treble(Treble(treble as i8)));
    }
    if let Some(loudness) = event.loudness {
        changes.push(PropertyChange::Loudness(Loudness(loudness)));
    }

    changes
}

fn decode_av_transport(event: &AVTransportEvent) -> Vec<PropertyChange> {
    let mut changes = vec![];

    if let Some(state) = &event.transport_state {
        let ps = match state.as_str() {
            "PLAYING" => PlaybackState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => PlaybackState::Paused,
            "STOPPED" => PlaybackState::Stopped,
            _ => PlaybackState::Transitioning,
        };
        changes.push(PropertyChange::PlaybackState(ps));
    }

    // Position
    if event.rel_time.is_some() || event.track_duration.is_some() {
        let position = Position {
            position_ms: parse_duration_ms(event.rel_time.as_deref()),
            duration_ms: parse_duration_ms(event.track_duration.as_deref()),
        };
        changes.push(PropertyChange::Position(position));
    }

    // CurrentTrack
    if event.current_track_uri.is_some() || event.track_metadata.is_some() {
        let track = CurrentTrack {
            title: event.track_metadata.as_ref()
                .and_then(|m| m.title.clone()),
            artist: event.track_metadata.as_ref()
                .and_then(|m| m.creator.clone()),
            album: event.track_metadata.as_ref()
                .and_then(|m| m.album.clone()),
            album_art_uri: event.track_metadata.as_ref()
                .and_then(|m| m.album_art_uri.clone()),
            uri: event.current_track_uri.clone(),
        };
        changes.push(PropertyChange::CurrentTrack(track));
    }

    changes
}

fn parse_duration_ms(duration: Option<&str>) -> Option<u64> {
    // Parse "HH:MM:SS" to milliseconds
    let d = duration?;
    let parts: Vec<&str> = d.split(':').collect();
    if parts.len() != 3 { return None; }

    let hours: u64 = parts[0].parse().ok()?;
    let minutes: u64 = parts[1].parse().ok()?;
    let seconds: u64 = parts[2].parse().ok()?;

    Some((hours * 3600 + minutes * 60 + seconds) * 1000)
}
```

### Files to Create (sonos-state)

| File | Purpose |
|------|---------|
| `src/decoder.rs` | Convert EnrichedEvent → typed properties |

---

## Phase 3: Wire StateManager to SonosEventManager

### 3.1 Add Event Manager to StateManager

```rust
// sonos-state/src/state.rs

use sonos_event_manager::SonosEventManager;

pub struct StateManager {
    // ... existing fields ...

    /// Event manager (optional - enables live events)
    event_manager: Option<Arc<SonosEventManager>>,

    /// Background event processor
    _event_worker: Option<JoinHandle<()>>,
}

impl StateManagerBuilder {
    pub fn with_event_manager(mut self, em: Arc<SonosEventManager>) -> Self {
        self.event_manager = Some(em);
        self
    }

    pub fn build(self) -> Result<StateManager> {
        let (event_tx, event_rx) = mpsc::channel();

        let manager = StateManager {
            store: Arc::new(RwLock::new(StateStore::new())),
            watched: Arc::new(RwLock::new(HashSet::new())),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            event_manager: self.event_manager.clone(),
            _event_worker: None,
            cleanup_timeout: self.cleanup_timeout,
        };

        // Spawn event processor if event_manager provided
        if let Some(em) = &self.event_manager {
            let worker = spawn_state_event_worker(
                Arc::clone(em),
                Arc::clone(&manager.store),
                Arc::clone(&manager.watched),
                manager.event_tx.clone(),
            );
            // Store worker handle...
        }

        Ok(manager)
    }
}
```

### 3.2 State Event Worker

```rust
// sonos-state/src/event_worker.rs

use std::sync::{mpsc, Arc, RwLock};
use std::thread::{self, JoinHandle};

use sonos_event_manager::SonosEventManager;
use crate::decoder::{decode_event, PropertyChange};
use crate::state::{ChangeEvent, StateStore};

pub(crate) fn spawn_state_event_worker(
    event_manager: Arc<SonosEventManager>,
    store: Arc<RwLock<StateStore>>,
    watched: Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: mpsc::Sender<ChangeEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // Consume events from event manager (blocking)
        for event in event_manager.iter() {
            // Look up speaker_id from IP
            let speaker_id = {
                let store = store.read().unwrap();
                match store.speaker_id_for_ip(event.speaker_ip) {
                    Some(id) => id.clone(),
                    None => continue, // Unknown speaker
                }
            };

            // Decode event
            let decoded = decode_event(&event, speaker_id.clone());

            // Apply changes
            for change in decoded.changes {
                apply_property_change(
                    &store, &watched, &event_tx, &speaker_id, change
                );
            }
        }
    })
}

fn apply_property_change(
    store: &Arc<RwLock<StateStore>>,
    watched: &Arc<RwLock<HashSet<(SpeakerId, &'static str)>>>,
    event_tx: &mpsc::Sender<ChangeEvent>,
    speaker_id: &SpeakerId,
    change: PropertyChange,
) {
    let (key, service) = (change.key(), change.service());

    let changed = {
        let mut store = store.write().unwrap();
        match change {
            PropertyChange::Volume(v) => store.set(speaker_id, v),
            PropertyChange::Mute(v) => store.set(speaker_id, v),
            PropertyChange::Bass(v) => store.set(speaker_id, v),
            PropertyChange::Treble(v) => store.set(speaker_id, v),
            PropertyChange::Loudness(v) => store.set(speaker_id, v),
            PropertyChange::PlaybackState(v) => store.set(speaker_id, v),
            PropertyChange::Position(v) => store.set(speaker_id, v),
            PropertyChange::CurrentTrack(v) => store.set(speaker_id, v),
            PropertyChange::GroupMembership(v) => store.set(speaker_id, v),
        }
    };

    if changed {
        let is_watched = watched.read()
            .map(|w| w.contains(&(speaker_id.clone(), key)))
            .unwrap_or(false);

        if is_watched {
            let _ = event_tx.send(ChangeEvent::new(speaker_id.clone(), key, service));
        }
    }
}
```

### 3.3 Wire PropertyHandle to SonosEventManager

```rust
// sonos-state/src/speaker.rs

impl<P: Property> PropertyHandle<P> {
    pub fn watch(&self) -> Result<Option<P>> {
        // 1. Register in local watched set
        self.state_manager.register_watch(&self.speaker_id, P::KEY);

        // 2. Subscribe via event manager (sync call)
        if let Some(em) = &self.state_manager.event_manager {
            if let Err(e) = em.ensure_service_subscribed(self.speaker_ip, P::SERVICE) {
                tracing::warn!("Failed to subscribe: {}", e);
            }
        }

        Ok(self.get())
    }

    pub fn unwatch(&self) {
        // 1. Unregister from local watched set
        self.state_manager.unregister_watch(&self.speaker_id, P::KEY);

        // 2. Release subscription via event manager (sync call)
        if let Some(em) = &self.state_manager.event_manager {
            if let Err(e) = em.release_service_subscription(self.speaker_ip, P::SERVICE) {
                tracing::warn!("Failed to unsubscribe: {}", e);
            }
        }
    }
}
```

### Files to Modify (sonos-state)

| File | Action |
|------|--------|
| `src/decoder.rs` | New - event decoder |
| `src/event_worker.rs` | New - background processor |
| `src/state.rs` | Add event_manager field, builder method |
| `src/speaker.rs` | Wire watch/unwatch to event manager |
| `src/lib.rs` | Update exports |
| `Cargo.toml` | Add sonos-event-manager dep |

---

## Phase 4: Update sonos-sdk

### 4.1 Simplified SonosSystem

```rust
// sonos-sdk/src/system.rs

use std::sync::Arc;
use sonos_state::{StateManager, Speaker, SpeakerId, ChangeIterator};
use sonos_event_manager::SonosEventManager;
use sonos_discovery;

pub struct SonosSystem {
    state_manager: Arc<StateManager>,
    _event_manager: Arc<SonosEventManager>,
}

impl SonosSystem {
    /// Create with auto-discovery (fully sync)
    pub fn new() -> Result<Self, SdkError> {
        // Create event manager (sync)
        let event_manager = Arc::new(SonosEventManager::new()?);

        // Discover devices
        let devices = sonos_discovery::get();

        // Add to event manager
        event_manager.add_devices(devices.clone())?;

        // Create state manager with event manager
        let state_manager = Arc::new(
            StateManager::builder()
                .with_event_manager(Arc::clone(&event_manager))
                .build()?
        );

        // Add devices to state manager
        state_manager.add_devices(devices)?;

        Ok(Self {
            state_manager,
            _event_manager: event_manager,
        })
    }

    pub fn speakers(&self) -> Vec<Speaker> {
        self.state_manager.speakers()
    }

    pub fn get_speaker_by_id(&self, id: &SpeakerId) -> Option<Speaker> {
        self.state_manager.speaker(id)
    }

    pub fn iter(&self) -> ChangeIterator {
        self.state_manager.iter()
    }
}
```

---

## File Changes Summary

### sonos-event-manager (Sync Refactor)

| File | Action |
|------|--------|
| `src/manager.rs` | Rewrite - sync API with command channel |
| `src/worker.rs` | New - background thread with tokio |
| `src/iter.rs` | New - sync iterator |
| `src/lib.rs` | Update exports |
| `src/error.rs` | Minor updates |

### sonos-state (Integration)

| File | Action |
|------|--------|
| `src/decoder.rs` | New - EnrichedEvent → properties |
| `src/event_worker.rs` | New - consumes event manager iter |
| `src/state.rs` | Add event_manager, builder method |
| `src/speaker.rs` | Wire watch/unwatch |
| `src/lib.rs` | Export decoder |
| `Cargo.toml` | Add sonos-event-manager |

### sonos-sdk (Simplify)

| File | Action |
|------|--------|
| `src/system.rs` | Rewrite - fully sync |
| `src/speaker.rs` | Delete - use sonos-state |
| `src/property/` | Delete - use sonos-state |
| `src/lib.rs` | Update exports |

---

## Verification

### Build All
```bash
cargo build -p sonos-event-manager
cargo build -p sonos-state
cargo build -p sonos-sdk
```

### Test Event Flow
```rust
#[test]
fn test_sync_event_flow() {
    let system = SonosSystem::new().unwrap();
    let speaker = &system.speakers()[0];

    // Watch (sync)
    speaker.volume.watch().unwrap();

    // Iterate (blocking)
    let event = system.iter().recv_timeout(Duration::from_secs(10));
    assert!(event.is_some());
}
```

### Run Example
```bash
cargo run -p sonos-state --example live_dashboard
```

---

## Design Decisions

| Decision | Choice |
|----------|--------|
| sonos-event-manager API | Sync (background thread hides async) |
| sonos-stream | Keep async (too deep to refactor) |
| Event delivery | Fire-and-forget, errors logged |
| Subscription management | Sync ref counting in main thread |
