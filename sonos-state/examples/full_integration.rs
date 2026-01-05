//! Full Integration Example
//!
//! This example demonstrates how to use sonos-state together with sonos-discovery
//! and sonos-stream to build a complete state management system for Sonos devices.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example full_integration
//! ```
//!
//! # What this example does
//!
//! 1. Discovers Sonos devices on the network using sonos-discovery
//! 2. Initializes state from a single speaker IP using GetZoneGroupTopology
//! 3. Sets up event streaming for all discovered speakers
//! 4. Consumes and displays state changes in real-time

use sonos_state::{
    EventReceiver, StateChange, StateEvent, StateEventPayload, StateManager, StateManagerConfig,
};
use std::net::IpAddr;
use std::sync::mpsc;
use std::time::Duration;

/// Adapter to convert sonos-stream events to sonos-state events
///
/// This demonstrates how to implement the EventReceiver trait
/// for integration with sonos-stream.
struct StreamEventAdapter {
    receiver: mpsc::Receiver<StateEvent>,
}

impl StreamEventAdapter {
    fn new(receiver: mpsc::Receiver<StateEvent>) -> Self {
        Self { receiver }
    }
}

impl EventReceiver for StreamEventAdapter {
    fn recv(&mut self) -> Option<StateEvent> {
        self.receiver.recv().ok()
    }

    fn try_recv(&mut self) -> Option<StateEvent> {
        self.receiver.try_recv().ok()
    }

    fn is_connected(&self) -> bool {
        // Channel is connected if we can try_recv without getting Disconnected
        match self.receiver.try_recv() {
            Err(mpsc::TryRecvError::Disconnected) => false,
            _ => true,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Sonos State Management - Full Integration Example");
    println!("{}", "=".repeat(50));

    // Step 1: Discover devices
    println!("\n1. Discovering Sonos devices...");
    let devices = sonos_discovery::get_with_timeout(Duration::from_secs(5));

    if devices.is_empty() {
        println!("   No Sonos devices found on the network!");
        println!("   Make sure you're on the same network as your Sonos speakers.");
        return Ok(());
    }

    println!("   Found {} device(s):", devices.len());
    for device in &devices {
        println!("   - {} ({}) at {}", device.name, device.model_name, device.ip_address);
    }

    // Use the first device for initialization
    let device = &devices[0];
    let device_ip: IpAddr = device
        .ip_address
        .parse()
        .expect("Invalid IP address from discovery");

    println!("\n   Using '{}' at {} for initialization", device.name, device_ip);

    // Step 2: Initialize state from single IP
    println!("\n2. Initializing state from speaker...");
    let mut state_manager = StateManager::new(StateManagerConfig::default());

    match state_manager.initialize_from_ip(device_ip) {
        Ok(()) => {
            let snapshot = state_manager.snapshot();
            println!("   Discovered {} speaker(s) in {} group(s)",
                     snapshot.speaker_count(),
                     snapshot.group_count());

            println!("\n   Speakers:");
            for speaker_state in snapshot.speakers() {
                let coord_marker = if speaker_state.is_coordinator { " (coordinator)" } else { "" };
                println!("   - {} [{}]{}",
                         speaker_state.speaker.name,
                         speaker_state.speaker.ip_address,
                         coord_marker);
            }

            println!("\n   Groups:");
            for group in snapshot.groups() {
                let member_count = group.member_count();
                let type_label = if group.is_standalone() { "standalone" } else { "grouped" };
                println!("   - {} ({} member(s), {})",
                         group.get_id(),
                         member_count,
                         type_label);
            }
        }
        Err(e) => {
            println!("   Failed to initialize state: {}", e);
            println!("   Continuing with manual state population for demo...");

            // For demo purposes, manually initialize with discovered devices
            let speakers: Vec<sonos_state::Speaker> = devices
                .iter()
                .map(|d| sonos_state::Speaker {
                    id: sonos_state::SpeakerId::new(&d.id),
                    name: d.name.clone(),
                    room_name: d.room_name.clone(),
                    ip_address: d.ip_address.parse().unwrap(),
                    port: d.port,
                    model_name: d.model_name.clone(),
                    software_version: String::new(),
                    satellites: vec![],
                })
                .collect();

            state_manager.cache().initialize(speakers, vec![]);
        }
    }

    // Step 3: Demonstrate event processing
    println!("\n3. Demonstrating event processing...");

    // Create a channel for simulating events
    let (event_sender, event_receiver) = mpsc::channel();

    // Simulate some events for demonstration
    println!("   Simulating volume change event...");
    let volume_event = StateEvent::new(
        device_ip,
        StateEventPayload::Rendering {
            master_volume: Some(42),
            master_mute: Some(false),
        },
    );
    event_sender.send(volume_event)?;

    println!("   Simulating playback state change event...");
    let playback_event = StateEvent::new(
        device_ip,
        StateEventPayload::Transport {
            transport_state: Some("PLAYING".to_string()),
            current_track_uri: Some("x-sonos-spotify:spotify:track:abc123".to_string()),
            track_duration: Some("0:03:45".to_string()),
            rel_time: Some("0:01:30".to_string()),
            track_metadata: Some(r#"<DIDL-Lite><item><dc:title>Test Song</dc:title><dc:creator>Test Artist</dc:creator></item></DIDL-Lite>"#.to_string()),
        },
    );
    event_sender.send(playback_event)?;

    // Process events manually (for demo)
    drop(event_sender); // Close sender to signal end

    let adapter = StreamEventAdapter::new(event_receiver);
    println!("\n   Processing events...");

    // Process events synchronously for demo
    let mut receiver_for_processing = adapter;
    while let Some(event) = receiver_for_processing.try_recv() {
        let changes = state_manager.process_event(event);
        for change in changes {
            print_state_change(&change);
        }
    }

    // Step 4: Show final state
    println!("\n4. Final state after events:");
    let final_snapshot = state_manager.snapshot();
    for speaker_state in final_snapshot.speakers() {
        println!("   {} ({}):", speaker_state.speaker.name, speaker_state.speaker.ip_address);
        println!("      Playback: {:?}", speaker_state.playback_state);
        println!("      Volume: {}", speaker_state.volume);
        println!("      Muted: {}", speaker_state.muted);
        if let Some(track) = &speaker_state.current_track {
            println!("      Track: {} - {}",
                     track.title.as_deref().unwrap_or("Unknown"),
                     track.artist.as_deref().unwrap_or("Unknown"));
        }
    }

    // Instructions for real usage
    println!("\n{}", "=".repeat(50));
    println!("For real-time event processing, you would:");
    println!("1. Set up sonos-stream EventBroker");
    println!("2. Register all speakers for AVTransport and RenderingControl events");
    println!("3. Create an adapter from sonos-stream events to StateEvent");
    println!("4. Call state_manager.start_processing() with the adapter");
    println!("5. Consume StateChange events from the change receiver");

    Ok(())
}

fn print_state_change(change: &StateChange) {
    match change {
        StateChange::VolumeChanged {
            speaker_id,
            old_volume,
            new_volume,
        } => {
            println!("      [{}] Volume: {} -> {}", speaker_id, old_volume, new_volume);
        }
        StateChange::MuteChanged { speaker_id, muted } => {
            println!("      [{}] Mute: {}", speaker_id, muted);
        }
        StateChange::PlaybackStateChanged {
            speaker_id,
            old_state,
            new_state,
        } => {
            println!("      [{}] Playback: {:?} -> {:?}", speaker_id, old_state, new_state);
        }
        StateChange::PositionChanged {
            speaker_id,
            position_ms,
            duration_ms,
        } => {
            let progress = if *duration_ms > 0 {
                (*position_ms as f64 / *duration_ms as f64) * 100.0
            } else {
                0.0
            };
            println!("      [{}] Position: {:.1}%", speaker_id, progress);
        }
        StateChange::TrackChanged {
            speaker_id,
            new_track,
            ..
        } => {
            if let Some(track) = new_track {
                println!(
                    "      [{}] Track: {} - {}",
                    speaker_id,
                    track.title.as_deref().unwrap_or("Unknown"),
                    track.artist.as_deref().unwrap_or("Unknown")
                );
            } else {
                println!("      [{}] Track: None", speaker_id);
            }
        }
        StateChange::GroupsChanged { new_groups, .. } => {
            println!("      Groups changed: {} groups", new_groups.len());
        }
        StateChange::StateInitialized {
            speaker_count,
            group_count,
        } => {
            println!(
                "      State initialized: {} speakers, {} groups",
                speaker_count, group_count
            );
        }
        StateChange::SpeakerAdded { speaker_id } => {
            println!("      Speaker added: {}", speaker_id);
        }
        StateChange::SpeakerRemoved { speaker_id } => {
            println!("      Speaker removed: {}", speaker_id);
        }
        StateChange::ProcessingError { error, .. } => {
            println!("      Error: {}", error);
        }
    }
}
