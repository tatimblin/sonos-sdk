//! Live demo of the Sonos SDK API
//!
//! Demonstrates the key SDK features on real speakers:
//! - Cache-first discovery
//! - New concise method names (speaker(), group(), etc.)
//! - Three property access patterns: get(), fetch(), watch()
//! - Fluent navigation: speaker.group(), group.speaker("name")
//! - Lazy event manager (only starts on first watch())
//! - Reactive event iteration
//!
//! Run with: cargo run -p sonos-sdk --example sdk_demo

use sonos_sdk::prelude::*;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), SdkError> {
    println!();
    println!("  Sonos SDK Demo");
    println!("  ==============");
    println!();

    // =========================================================================
    // 1. Discovery (cache-first, sync)
    // =========================================================================
    step("Discovering speakers (cache-first, then SSDP fallback)...");
    let system = SonosSystem::new()?;

    let names = system.speaker_names();
    println!("  Found {} speaker(s): {}", names.len(), names.join(", "));
    println!();

    if names.is_empty() {
        println!("  No speakers on the network. Exiting.");
        return Ok(());
    }

    // =========================================================================
    // 2. Speaker lookup — find a reachable speaker
    // =========================================================================
    step("Finding a reachable speaker...");

    let mut speaker = None;
    for name in &names {
        if let Some(s) = system.speaker(name) {
            // Probe with a quick fetch to confirm it's reachable
            match s.volume.fetch() {
                Ok(_) => {
                    println!("  {} — {} at {} (reachable)", s.name, s.model_name, s.ip);
                    speaker = Some(s);
                    break;
                }
                Err(_) => {
                    println!("  {} at {} — skipping (unreachable)", s.name, s.ip);
                }
            }
        }
    }

    let speaker = speaker.ok_or_else(|| {
        SdkError::DiscoveryFailed("no reachable speakers found".to_string())
    })?;
    println!();

    // =========================================================================
    // 3. Property fetch() — direct SOAP call, updates cache
    // =========================================================================
    step("Fetching properties from device (SOAP calls)...");

    // Volume already fetched during probe — read from cache
    let volume = speaker.volume.get().unwrap_or(Volume(0));
    println!("  volume.fetch()          => {}%", volume.0);

    let playback = speaker.playback_state.fetch()?;
    println!("  playback_state.fetch()  => {playback:?}");

    let mute = speaker.mute.fetch()?;
    println!(
        "  mute.fetch()            => {}",
        if mute.0 { "muted" } else { "unmuted" }
    );

    let bass = speaker.bass.fetch()?;
    println!("  bass.fetch()            => {}", bass.0);

    let treble = speaker.treble.fetch()?;
    println!("  treble.fetch()          => {}", treble.0);

    let loudness = speaker.loudness.fetch()?;
    println!(
        "  loudness.fetch()        => {}",
        if loudness.0 { "on" } else { "off" }
    );

    println!();

    // =========================================================================
    // 4. Property get() — cached value (no network)
    // =========================================================================
    step("Reading cached values (no network calls)...");

    match speaker.volume.get() {
        Some(v) => println!("  volume.get()            => {}%  (cached)", v.0),
        None => println!("  volume.get()            => None (no cache)"),
    }
    match speaker.playback_state.get() {
        Some(s) => println!("  playback_state.get()    => {s:?}  (cached)"),
        None => println!("  playback_state.get()    => None (no cache)"),
    }

    println!();

    // =========================================================================
    // 5. Fluent navigation — speaker.group(), group.speaker()
    // =========================================================================
    step("Fluent navigation: speaker -> group -> members...");

    // Ensure topology is loaded
    let groups = system.groups();
    println!("  system.groups()         => {} group(s)", groups.len());

    if let Some(group) = speaker.group() {
        println!("  speaker.group()         => group {}", short_id(&group.id));
        println!(
            "  group.member_count()    => {} speaker(s)",
            group.member_count()
        );

        if let Some(coord) = group.coordinator() {
            println!("  group.coordinator()     => {}", coord.name);
        }

        for member in group.members() {
            let role = if group.is_coordinator(&member.id) {
                "coordinator"
            } else {
                "member"
            };
            println!("  group.members()         => {} ({})", member.name, role);
        }

        // Reverse navigation: group.speaker("name")
        if let Some(found) = group.speaker(&speaker.name) {
            println!(
                "  group.speaker(\"{}\")  => found (id: {})",
                speaker.name,
                found.id.as_str()
            );
        }
    } else {
        println!("  speaker.group()         => None (topology not loaded)");
    }

    println!();

    // =========================================================================
    // 6. Write operations with cache update
    // =========================================================================
    let original_volume = speaker.volume.get().map(|v| v.0).unwrap_or(20);
    let demo_volume = if original_volume > 10 {
        original_volume - 5
    } else {
        original_volume + 5
    };

    step(&format!(
        "Adjusting volume {original_volume} -> {demo_volume} (will restore)..."
    ));
    speaker.set_volume(demo_volume)?;
    println!("  set_volume({demo_volume})          => OK");

    // Cache should reflect the new value immediately
    if let Some(v) = speaker.volume.get() {
        println!("  volume.get()            => {}%  (cache updated)", v.0);
    }

    // Pause briefly so the change is audible
    thread::sleep(Duration::from_secs(1));

    // Restore
    speaker.set_volume(original_volume)?;
    println!("  set_volume({original_volume})          => OK (restored)");
    println!();

    // =========================================================================
    // 7. Lazy event manager — watch() triggers init
    // =========================================================================
    step("Starting watch() — lazy event manager initializes now...");
    let watch_status = speaker.volume.watch()?;
    println!(
        "  volume.watch()          => mode: {}, current: {}%",
        watch_status.mode,
        watch_status
            .current
            .map(|v| v.0.to_string())
            .unwrap_or_else(|| "?".to_string())
    );

    speaker.playback_state.watch()?;
    println!("  playback_state.watch()  => OK");
    println!();

    // =========================================================================
    // 8. Reactive event iteration
    // =========================================================================
    step("Listening for live events (5 seconds)...");
    println!("  Tip: change volume on the speaker or in the Sonos app to see events.");
    println!();

    let iter = system.iter();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut event_count = 0;

    loop {
        match iter.recv_timeout(Duration::from_millis(500)) {
            Some(event) => {
                event_count += 1;

                // Read the fresh cached value after the change notification
                let value_str = match event.property_key {
                    "volume" => speaker
                        .volume
                        .get()
                        .map(|v| format!("{}%", v.0))
                        .unwrap_or_else(|| "?".to_string()),
                    "playback_state" => speaker
                        .playback_state
                        .get()
                        .map(|s| format!("{s:?}"))
                        .unwrap_or_else(|| "?".to_string()),
                    "mute" => speaker
                        .mute
                        .get()
                        .map(|m| if m.0 { "muted" } else { "unmuted" }.to_string())
                        .unwrap_or_else(|| "?".to_string()),
                    other => other.to_string(),
                };

                println!(
                    "  [event {event_count}] {} => {value_str} (speaker: {})",
                    event.property_key, event.speaker_id
                );
            }
            None => {
                // Timeout — check if 5 seconds total have elapsed
                if event_count > 0 || std::time::Instant::now() > deadline {
                    break;
                }
            }
        }

        if std::time::Instant::now() > deadline {
            break;
        }
    }

    if event_count == 0 {
        println!("  (no events received — speakers were idle)");
    } else {
        println!("  {event_count} event(s) received");
    }

    println!();

    // =========================================================================
    // 9. Cleanup
    // =========================================================================
    step("Cleaning up watches...");
    speaker.volume.unwatch();
    speaker.playback_state.unwatch();
    println!("  volume.unwatch()        => OK");
    println!("  playback_state.unwatch()=> OK");

    println!();
    println!("  Demo complete.");
    println!();

    Ok(())
}

fn step(msg: &str) {
    println!("  >> {msg}");
}

fn short_id(id: &GroupId) -> &str {
    let s = id.as_str();
    if s.len() > 16 {
        &s[..16]
    } else {
        s
    }
}
