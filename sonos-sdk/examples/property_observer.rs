//! Property Observer Dashboard — live visibility into all watch() properties
//!
//! Discovers all speakers and watches every property, displaying real-time values,
//! update counts, timestamps, and WatchMode for each. Use this to verify that
//! property changes (e.g., volume adjustments on the Sonos app) propagate through
//! the event pipeline to the SDK.
//!
//! Run with: cargo run -p sonos-sdk --example property_observer
//!
//! For debug output: RUST_LOG=debug cargo run -p sonos-sdk --example property_observer

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sonos_sdk::prelude::*;
use sonos_state::Position;

/// Tracks per-property observation metadata
struct PropertyObservation {
    value: String,
    last_updated: Instant,
    update_count: u64,
    mode: String,
}

impl PropertyObservation {
    fn new(value: String, mode: String) -> Self {
        Self {
            value,
            last_updated: Instant::now(),
            update_count: 0,
            mode,
        }
    }

    fn update(&mut self, value: String) {
        self.value = value;
        self.last_updated = Instant::now();
        self.update_count += 1;
    }

    fn age_str(&self) -> String {
        let elapsed = self.last_updated.elapsed();
        if elapsed.as_secs() < 1 {
            format!("{}ms ago", elapsed.as_millis())
        } else if elapsed.as_secs() < 60 {
            format!("{}s ago", elapsed.as_secs())
        } else {
            format!("{}m ago", elapsed.as_secs() / 60)
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("sonos_stream=info".parse().unwrap())
                .add_directive("sonos_event_manager=info".parse().unwrap())
                .add_directive("sonos_state=info".parse().unwrap()),
        )
        .init();

    println!("=== Property Observer Dashboard ===\n");

    // Step 1: Discover speakers
    println!("Discovering Sonos devices...");
    let system = SonosSystem::new()?;
    let speaker_names = system.speaker_names();

    if speaker_names.is_empty() {
        println!("No Sonos speakers found on the network.");
        return Ok(());
    }

    println!(
        "Found {} speakers: {}\n",
        speaker_names.len(),
        speaker_names.join(", ")
    );

    // Step 2: Find reachable speakers and set up observations
    let mut observations: HashMap<String, PropertyObservation> = HashMap::new();

    // Hold all watch handles to keep subscriptions alive
    let mut _watch_handles: Vec<Box<dyn std::any::Any>> = Vec::new();

    // Track speaker names by ID for display
    let mut speaker_display: HashMap<String, String> = HashMap::new();

    for name in &speaker_names {
        let speaker = match system.speaker(name) {
            Some(s) => s,
            None => continue,
        };

        let sid = speaker.id.as_str().to_string();

        // Volume watch serves as reachability probe — skip speaker if it fails
        let volume_handle = match speaker.volume.watch() {
            Ok(handle) => handle,
            Err(_) => {
                eprintln!("  Skipping {name} (unreachable)");
                continue;
            }
        };

        speaker_display.insert(sid.clone(), name.clone());

        println!("Setting up watches for {name}...");

        // ================================================================
        // RenderingControl properties
        // ================================================================

        // Volume
        {
            let mode = volume_handle.mode().to_string();
            let val = speaker
                .volume
                .get()
                .map(|v| format!("{}", v.0))
                .unwrap_or_else(|| "-".into());
            observations.insert(format!("{sid}|volume"), PropertyObservation::new(val, mode));
            _watch_handles.push(Box::new(volume_handle));
        }

        // Mute
        if let Ok(handle) = speaker.mute.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .mute
                .get()
                .map(|m| format!("{}", m.0))
                .unwrap_or_else(|| "-".into());
            observations.insert(format!("{sid}|mute"), PropertyObservation::new(val, mode));
            _watch_handles.push(Box::new(handle));
        }

        // Bass
        if let Ok(handle) = speaker.bass.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .bass
                .get()
                .map(|b| format!("{}", b.0))
                .unwrap_or_else(|| "-".into());
            observations.insert(format!("{sid}|bass"), PropertyObservation::new(val, mode));
            _watch_handles.push(Box::new(handle));
        }

        // Treble
        if let Ok(handle) = speaker.treble.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .treble
                .get()
                .map(|t| format!("{}", t.0))
                .unwrap_or_else(|| "-".into());
            observations.insert(format!("{sid}|treble"), PropertyObservation::new(val, mode));
            _watch_handles.push(Box::new(handle));
        }

        // Loudness
        if let Ok(handle) = speaker.loudness.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .loudness
                .get()
                .map(|l| format!("{}", l.0))
                .unwrap_or_else(|| "-".into());
            observations.insert(
                format!("{sid}|loudness"),
                PropertyObservation::new(val, mode),
            );
            _watch_handles.push(Box::new(handle));
        }

        // ================================================================
        // AVTransport properties
        // ================================================================

        // PlaybackState
        if let Ok(handle) = speaker.playback_state.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .playback_state
                .get()
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|| "-".into());
            observations.insert(
                format!("{sid}|playback_state"),
                PropertyObservation::new(val, mode),
            );
            _watch_handles.push(Box::new(handle));
        }

        // Position
        if let Ok(handle) = speaker.position.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .position
                .get()
                .map(|p| format_position(&p))
                .unwrap_or_else(|| "-".into());
            observations.insert(
                format!("{sid}|position"),
                PropertyObservation::new(val, mode),
            );
            _watch_handles.push(Box::new(handle));
        }

        // CurrentTrack
        if let Ok(handle) = speaker.current_track.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .current_track
                .get()
                .map(|t| t.display())
                .unwrap_or_else(|| "-".into());
            observations.insert(
                format!("{sid}|current_track"),
                PropertyObservation::new(val, mode),
            );
            _watch_handles.push(Box::new(handle));
        }

        // GroupMembership
        if let Ok(handle) = speaker.group_membership.watch() {
            let mode = handle.mode().to_string();
            let val = speaker
                .group_membership
                .get()
                .map(|gm| format!("{} (coord={})", gm.group_id.as_str(), gm.is_coordinator))
                .unwrap_or_else(|| "-".into());
            observations.insert(
                format!("{sid}|group_membership"),
                PropertyObservation::new(val, mode),
            );
            _watch_handles.push(Box::new(handle));
        }
    }

    // Step 3: Wait for topology to populate, then set up group watches
    println!("\nWaiting for topology...");
    let mut topology_ready = false;
    for _ in 0..10 {
        let groups = system.groups();
        if !groups.is_empty() {
            topology_ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    if topology_ready {
        let groups = system.groups();
        println!("Topology loaded: {} groups\n", groups.len());

        for group in &groups {
            if group.member_count() < 2 {
                continue; // Skip standalone groups for group property display
            }

            let group_label = group
                .coordinator()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| group.id.as_str().to_string());

            // GroupVolume
            if let Ok(handle) = group.volume.watch() {
                let mode = handle.mode().to_string();
                let val = group
                    .volume
                    .get()
                    .map(|v| format!("{}", v.0))
                    .unwrap_or_else(|| "-".into());
                observations.insert(
                    format!("group:{}|group_volume", group.id.as_str()),
                    PropertyObservation::new(val, mode),
                );
                speaker_display.insert(format!("group:{}", group.id.as_str()), group_label.clone());
                _watch_handles.push(Box::new(handle));
            }

            // GroupMute
            if let Ok(handle) = group.mute.watch() {
                let mode = handle.mode().to_string();
                let val = group
                    .mute
                    .get()
                    .map(|m| format!("{}", m.0))
                    .unwrap_or_else(|| "-".into());
                observations.insert(
                    format!("group:{}|group_mute", group.id.as_str()),
                    PropertyObservation::new(val, mode),
                );
                _watch_handles.push(Box::new(handle));
            }

            // GroupVolumeChangeable
            if let Ok(handle) = group.volume_changeable.watch() {
                let mode = handle.mode().to_string();
                let val = group
                    .volume_changeable
                    .get()
                    .map(|v| format!("{}", v.0))
                    .unwrap_or_else(|| "-".into());
                observations.insert(
                    format!("group:{}|group_volume_changeable", group.id.as_str()),
                    PropertyObservation::new(val, mode),
                );
                _watch_handles.push(Box::new(handle));
            }
        }
    } else {
        println!("Topology not available (timeout). Group properties will not be monitored.\n");
    }

    // Step 4: Give subscriptions a moment to establish
    thread::sleep(Duration::from_millis(500));

    // Seed initial values by fetching all speaker properties
    for name in &speaker_names {
        if let Some(speaker) = system.speaker(name) {
            let sid = speaker.id.as_str().to_string();
            if !speaker_display.contains_key(&sid) {
                continue;
            }
            // Fetch to seed cache
            let _ = speaker.volume.fetch();
            let _ = speaker.mute.fetch();
            let _ = speaker.bass.fetch();
            let _ = speaker.treble.fetch();
            let _ = speaker.loudness.fetch();
            let _ = speaker.playback_state.fetch();
            let _ = speaker.position.fetch();
            let _ = speaker.current_track.fetch();

            // Update observations with fetched values
            if let Some(v) = speaker.volume.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|volume")) {
                    obs.value = format!("{}", v.0);
                }
            }
            if let Some(v) = speaker.mute.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|mute")) {
                    obs.value = format!("{}", v.0);
                }
            }
            if let Some(v) = speaker.bass.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|bass")) {
                    obs.value = format!("{}", v.0);
                }
            }
            if let Some(v) = speaker.treble.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|treble")) {
                    obs.value = format!("{}", v.0);
                }
            }
            if let Some(v) = speaker.loudness.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|loudness")) {
                    obs.value = format!("{}", v.0);
                }
            }
            if let Some(v) = speaker.playback_state.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|playback_state")) {
                    obs.value = format!("{v:?}");
                }
            }
            if let Some(v) = speaker.position.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|position")) {
                    obs.value = format_position(&v);
                }
            }
            if let Some(v) = speaker.current_track.get() {
                if let Some(obs) = observations.get_mut(&format!("{sid}|current_track")) {
                    obs.value = v.display();
                }
            }
        }
    }

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    // Display initial state
    display_dashboard(&observations, &speaker_display);

    // Step 5: Event loop — consume system.iter() and update display
    println!("\nListening for events (Ctrl+C to quit)...\n");
    let iter = system.iter();

    while running.load(Ordering::SeqCst) {
        if let Some(event) = iter.recv_timeout(Duration::from_secs(1)) {
            let sid = event.speaker_id.as_str().to_string();
            let key = format!("{}|{}", sid, event.property_key);

            // Update the observation with the new cached value
            if let Some(speaker_name) = speaker_display.get(&sid) {
                if let Some(speaker) = system.speaker(speaker_name) {
                    let new_value = read_property_value(&speaker, event.property_key);
                    if let Some(obs) = observations.get_mut(&key) {
                        obs.update(new_value);
                    }
                }
            }

            // Also check group property updates
            // Group events arrive on the coordinator speaker_id
            for group in system.groups() {
                if group.coordinator_id.as_str() == sid {
                    let group_key = format!("group:{}|{}", group.id.as_str(), event.property_key);
                    if let Some(obs) = observations.get_mut(&group_key) {
                        let new_value = read_group_property_value(&group, event.property_key);
                        obs.update(new_value);
                    }
                }
            }

            println!(
                "[{:>8}] {} | {} = {}",
                format_elapsed(event.timestamp.elapsed()),
                speaker_display.get(&sid).unwrap_or(&sid),
                event.property_key,
                observations
                    .get(&key)
                    .map(|o| o.value.as_str())
                    .unwrap_or("?"),
            );

            // Redisplay dashboard periodically (every 10 events or so)
            let total_events: u64 = observations.values().map(|o| o.update_count).sum();
            if total_events % 5 == 0 {
                display_dashboard(&observations, &speaker_display);
            }
        }
    }

    println!("\nShutting down gracefully...");
    // Watch handles will be dropped, triggering cleanup
    drop(_watch_handles);
    println!("Done.");

    Ok(())
}

/// Read the current cached value of a speaker property by key
fn read_property_value(speaker: &Speaker, key: &str) -> String {
    match key {
        "volume" => speaker
            .volume
            .get()
            .map(|v| format!("{}", v.0))
            .unwrap_or_else(|| "-".into()),
        "mute" => speaker
            .mute
            .get()
            .map(|m| format!("{}", m.0))
            .unwrap_or_else(|| "-".into()),
        "bass" => speaker
            .bass
            .get()
            .map(|b| format!("{}", b.0))
            .unwrap_or_else(|| "-".into()),
        "treble" => speaker
            .treble
            .get()
            .map(|t| format!("{}", t.0))
            .unwrap_or_else(|| "-".into()),
        "loudness" => speaker
            .loudness
            .get()
            .map(|l| format!("{}", l.0))
            .unwrap_or_else(|| "-".into()),
        "playback_state" => speaker
            .playback_state
            .get()
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|| "-".into()),
        "position" => speaker
            .position
            .get()
            .map(|p| format_position(&p))
            .unwrap_or_else(|| "-".into()),
        "current_track" => speaker
            .current_track
            .get()
            .map(|t| t.display())
            .unwrap_or_else(|| "-".into()),
        "group_membership" => speaker
            .group_membership
            .get()
            .map(|gm| format!("{} (coord={})", gm.group_id.as_str(), gm.is_coordinator))
            .unwrap_or_else(|| "-".into()),
        _ => format!("(unknown: {key})"),
    }
}

/// Read the current cached value of a group property by key
fn read_group_property_value(group: &Group, key: &str) -> String {
    match key {
        "group_volume" => group
            .volume
            .get()
            .map(|v| format!("{}", v.0))
            .unwrap_or_else(|| "-".into()),
        "group_mute" => group
            .mute
            .get()
            .map(|m| format!("{}", m.0))
            .unwrap_or_else(|| "-".into()),
        "group_volume_changeable" => group
            .volume_changeable
            .get()
            .map(|v| format!("{}", v.0))
            .unwrap_or_else(|| "-".into()),
        _ => "-".into(),
    }
}

/// Format a Position value as MM:SS / MM:SS
fn format_position(pos: &Position) -> String {
    if pos.duration_ms == 0 && pos.position_ms == 0 {
        return "0:00 / 0:00".to_string();
    }
    format!(
        "{} / {} ({:.0}%)",
        format_time(pos.position_ms),
        format_time(pos.duration_ms),
        pos.progress() * 100.0
    )
}

/// Format milliseconds as M:SS
fn format_time(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}

/// Format a Duration as a short string
fn format_elapsed(d: Duration) -> String {
    if d.as_millis() < 1000 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

/// Display the full dashboard table
fn display_dashboard(
    observations: &HashMap<String, PropertyObservation>,
    speaker_display: &HashMap<String, String>,
) {
    println!("\n{}", "=".repeat(100));
    println!(
        "{:<16} {:<20} {:<30} {:<10} {:<6} Mode",
        "Speaker", "Property", "Value", "Updated", "Count"
    );
    println!("{}", "-".repeat(100));

    // Sort by speaker name, then property
    let mut entries: Vec<_> = observations.iter().collect();
    entries.sort_by(|(ka, _), (kb, _)| {
        let (sa, pa) = ka.split_once('|').unwrap_or((ka, ""));
        let (sb, pb) = kb.split_once('|').unwrap_or((kb, ""));
        let name_a = speaker_display.get(sa).map(|s| s.as_str()).unwrap_or(sa);
        let name_b = speaker_display.get(sb).map(|s| s.as_str()).unwrap_or(sb);
        name_a
            .cmp(name_b)
            .then(property_order(pa).cmp(&property_order(pb)))
    });

    let mut last_speaker = String::new();
    for (key, obs) in &entries {
        let (entity_id, prop_name) = key.split_once('|').unwrap_or((key, "?"));
        let display_name = speaker_display
            .get(entity_id)
            .map(|s| s.as_str())
            .unwrap_or(entity_id);

        // Add separator between speakers
        let speaker_col = if display_name != last_speaker {
            if !last_speaker.is_empty() {
                println!("{}", "-".repeat(100));
            }
            last_speaker = display_name.to_string();
            truncate(display_name, 15)
        } else {
            String::new()
        };

        println!(
            "{:<16} {:<20} {:<30} {:<10} {:<6} {}",
            speaker_col,
            prop_name,
            truncate(&obs.value, 29),
            obs.age_str(),
            obs.update_count,
            obs.mode,
        );
    }

    println!("{}", "=".repeat(100));
    let total_events: u64 = observations.values().map(|o| o.update_count).sum();
    println!(
        "Watching {} properties | {} total events received",
        observations.len(),
        total_events
    );
}

/// Ordering for properties to group by service
fn property_order(prop: &str) -> u8 {
    match prop {
        "volume" => 0,
        "mute" => 1,
        "bass" => 2,
        "treble" => 3,
        "loudness" => 4,
        "playback_state" => 5,
        "position" => 6,
        "current_track" => 7,
        "group_membership" => 8,
        "group_volume" => 9,
        "group_mute" => 10,
        "group_volume_changeable" => 11,
        _ => 99,
    }
}

/// Truncate a string to max length with ellipsis
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
