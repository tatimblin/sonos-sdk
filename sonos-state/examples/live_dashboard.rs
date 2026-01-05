//! Live Terminal Dashboard
//!
//! This example creates a live-updating terminal dashboard that shows:
//! - Speakers organized by their groups
//! - Real-time state updates (volume, playback, track info) from actual events
//! - Group coordinator information
//!
//! # Usage
//!
//! ```bash
//! cargo run --example live_dashboard
//! ```
//!
//! Press Ctrl+C to exit.
//!
//! Note: This example only processes real events from Sonos devices.
//! If no events are received, the display will remain static.

use sonos_state::{
    EventReceiver, PlaybackState, StateChange, StateManager, StateManagerConfig,
};
use std::io::{self, Write};
use std::net::IpAddr;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

/// Adapter to receive events from a channel (for real events only)
struct ChannelEventReceiver {
    receiver: mpsc::Receiver<sonos_state::StateEvent>,
    connected: bool,
}

impl ChannelEventReceiver {
    fn new(receiver: mpsc::Receiver<sonos_state::StateEvent>) -> Self {
        Self {
            receiver,
            connected: true,
        }
    }
}

impl EventReceiver for ChannelEventReceiver {
    fn recv(&mut self) -> Option<sonos_state::StateEvent> {
        match self.receiver.recv() {
            Ok(event) => Some(event),
            Err(_) => {
                self.connected = false;
                None
            }
        }
    }

    fn try_recv(&mut self) -> Option<sonos_state::StateEvent> {
        match self.receiver.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.connected = false;
                None
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Sonos Live Dashboard - Discovering devices...");

    // Step 1: Discover devices
    let devices = sonos_discovery::get_with_timeout(Duration::from_secs(5));

    if devices.is_empty() {
        println!("❌ No Sonos devices found on the network.");
        println!("   Make sure you're on the same network as your Sonos speakers.");
        println!("   The dashboard will show an empty state.");
        println!("\nStarting dashboard anyway...");

        // Create empty state manager
        let mut state_manager = StateManager::new(StateManagerConfig::default());
        let (_sender, receiver) = mpsc::channel::<sonos_state::StateEvent>();
        let event_receiver = ChannelEventReceiver::new(receiver);
        state_manager.start_processing(event_receiver)?;
        let change_receiver = state_manager.take_change_receiver().unwrap();
        run_dashboard(state_manager, change_receiver)?;
        return Ok(());
    }

    // Step 2: Initialize state manager
    let mut state_manager = StateManager::new(StateManagerConfig::default());
    let device_ip: IpAddr = devices[0].ip_address.parse()?;

    match state_manager.initialize_from_ip(device_ip) {
        Ok(()) => {
            println!("✅ Successfully discovered {} speaker(s) in {} group(s)",
                     state_manager.snapshot().speaker_count(),
                     state_manager.snapshot().group_count());
        }
        Err(e) => {
            println!("❌ Failed to get topology from device {}: {}", device_ip, e);

            // Try each discovered device as fallback
            let mut topology_success = false;
            for (i, device) in devices.iter().enumerate() {
                if let Ok(fallback_ip) = device.ip_address.parse::<IpAddr>() {
                    if fallback_ip == device_ip {
                        continue; // Skip the one we already tried
                    }

                    println!("   Trying fallback device {} ({})...", device.name, fallback_ip);
                    match state_manager.initialize_from_ip(fallback_ip) {
                        Ok(()) => {
                            println!("✅ Success with fallback! {} speaker(s) in {} group(s)",
                                     state_manager.snapshot().speaker_count(),
                                     state_manager.snapshot().group_count());
                            topology_success = true;
                            break;
                        }
                        Err(fallback_err) => {
                            println!("   Fallback failed: {}", fallback_err);
                            if i >= 2 { // Try max 3 devices
                                break;
                            }
                        }
                    }
                }
            }

            if !topology_success {
                println!("   All topology attempts failed. Using basic device info.");
                println!("   Speakers will show as individual groups.");

                // Initialize with basic device info, each as its own group
                let speakers: Vec<sonos_state::Speaker> = devices
                    .iter()
                    .map(|d| sonos_state::Speaker {
                        id: sonos_state::SpeakerId::new(&d.id),
                        name: d.name.clone(),
                        room_name: d.room_name.clone(),
                        ip_address: d.ip_address.parse().unwrap(),
                        port: d.port,
                        model_name: d.model_name.clone(),
                        software_version: "Unknown".to_string(),
                        satellites: vec![],
                    })
                    .collect();

                // Create individual groups for each speaker (standalone)
                let groups: Vec<sonos_state::Group> = speakers
                    .iter()
                    .map(|speaker| {
                        sonos_state::Group::new(
                            sonos_state::GroupId::new(&format!("{}:0", speaker.id.as_str())),
                            speaker.id.clone(),
                            vec![sonos_state::SpeakerRef::new(speaker.id.clone(), vec![])],
                        )
                    })
                    .collect();

                let group_count = groups.len();
                state_manager.cache().initialize(speakers, groups);
                println!("   Created {} individual groups for discovered speakers", group_count);
            }
        }
    }

    // Step 3: Set up event processing (no events will come in without sonos-stream integration)
    let (_event_sender, event_receiver) = mpsc::channel();
    let receiver = ChannelEventReceiver::new(event_receiver);

    // Start processing (will wait for real events that won't come)
    state_manager.start_processing(receiver)?;
    let change_receiver = state_manager.take_change_receiver().unwrap();

    // Step 4: Run live dashboard
    println!("\n💡 Dashboard ready! Real events from sonos-stream would appear here.");
    println!("   Currently showing static state from topology discovery.");
    run_dashboard(state_manager, change_receiver)?;

    Ok(())
}


fn run_dashboard(
    state_manager: StateManager,
    change_receiver: mpsc::Receiver<StateChange>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nStarting live dashboard... Press Ctrl+C to exit\n");

    // Set up signal handling for clean exit
    ctrlc::set_handler(move || {
        print!("\n\nExiting dashboard...\n");
        std::process::exit(0);
    })?;

    let mut last_update = SystemTime::now();
    let update_interval = Duration::from_millis(500);

    loop {
        // Process any pending changes
        while let Ok(_change) = change_receiver.try_recv() {
            // Changes are automatically applied to the state manager's cache
            // We just need to note that an update occurred
            last_update = SystemTime::now();
        }

        // Refresh display if enough time has passed
        if last_update.elapsed().unwrap_or(Duration::MAX) >= update_interval {
            clear_screen();
            display_state(&state_manager);
            last_update = SystemTime::now();
        }

        thread::sleep(Duration::from_millis(100));
    }
}

fn clear_screen() {
    print!("\x1B[2J\x1B[1;1H");
    io::stdout().flush().unwrap();
}

fn display_state(state_manager: &StateManager) {
    let snapshot = state_manager.snapshot();
    let now = SystemTime::now();

    println!("╔═══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                            🎵 SONOS LIVE DASHBOARD 🎵                         ║");
    println!("╚═══════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("📊 System: {} speakers, {} groups  🕐 Updated: {}",
             snapshot.speaker_count(),
             snapshot.group_count(),
             format_time(now));
    println!();

    if snapshot.group_count() == 0 {
        println!("🔍 No groups found. Speakers may still be initializing...");
        return;
    }

    // Display speakers organized by groups
    for group in snapshot.groups() {
        display_group(group, &snapshot);
        println!();
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💡 Listening for real events. Integrate with sonos-stream for live updates.");
}

fn display_group(group: &sonos_state::Group, snapshot: &sonos_state::StateSnapshot) {
    let group_type = if group.is_standalone() { "📱" } else { "👥" };
    let member_count = group.member_count();

    println!("{} Group: {} ({} member{})",
             group_type,
             group.get_id().as_str().split(':').next().unwrap_or(group.get_id().as_str()),
             member_count,
             if member_count == 1 { "" } else { "s" });

    // Get speakers in this group
    let speakers_in_group: Vec<_> = snapshot.speakers_in_group(group.get_id()).collect();

    for speaker_state in speakers_in_group {
        display_speaker(speaker_state, group);
    }
}

fn display_speaker(speaker_state: &sonos_state::SpeakerState, _group: &sonos_state::Group) {
    let coord_symbol = if speaker_state.is_coordinator { "👑" } else { "🔸" };
    let play_symbol = match speaker_state.playback_state {
        PlaybackState::Playing => "▶️",
        PlaybackState::Paused => "⏸️",
        PlaybackState::Stopped => "⏹️",
        PlaybackState::Transitioning => "⏳",
    };

    let mute_symbol = if speaker_state.muted { "🔇" } else { "" };
    let volume_bar = create_volume_bar(speaker_state.volume);

    println!("  {} {} {} ({})",
             coord_symbol,
             play_symbol,
             speaker_state.speaker.name,
             speaker_state.speaker.model_name);

    println!("     🔊 Vol: {:3}% {} {}",
             speaker_state.volume,
             volume_bar,
             mute_symbol);

    if let Some(track) = &speaker_state.current_track {
        let title = track.title.as_deref().unwrap_or("Unknown Track");
        let artist = track.artist.as_deref().unwrap_or("Unknown Artist");
        println!("     🎵 {}", format_track_info(title, artist));

        if speaker_state.duration_ms > 0 {
            let progress = speaker_state.progress_percent();
            let progress_bar = create_progress_bar(progress);
            let position = format_duration(speaker_state.position_ms);
            println!("     ⏱️  {} {} {:.0}%", position, progress_bar, progress);
        }
    } else if speaker_state.playback_state != PlaybackState::Stopped {
        println!("     🎵 No track information");
    }

    println!("     🌐 {}", speaker_state.speaker.ip_address);
}

fn create_volume_bar(volume: u8) -> String {
    let bar_length = 10;
    let filled = (volume as usize * bar_length) / 100;
    let empty = bar_length - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn create_progress_bar(progress: f64) -> String {
    let bar_length = 15;
    let filled = ((progress * bar_length as f64) / 100.0) as usize;
    let empty = bar_length - filled;
    format!("[{}{}]", "━".repeat(filled), "─".repeat(empty))
}

fn format_track_info(title: &str, artist: &str) -> String {
    let max_length = 50;
    let combined = format!("{} - {}", title, artist);
    if combined.len() > max_length {
        format!("{}...", &combined[..max_length-3])
    } else {
        combined
    }
}

fn format_duration(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{}:{:02}", minutes, seconds)
}

fn format_time(time: SystemTime) -> String {
    let duration = time.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let seconds = duration.as_secs();
    let local_seconds = seconds % 86400; // seconds in a day
    let hours = local_seconds / 3600;
    let minutes = (local_seconds % 3600) / 60;
    let secs = local_seconds % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, secs)
}