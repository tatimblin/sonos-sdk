//! Live demo — discover speakers and stream all service events in real time.
//!
//! Run with:
//!   cargo run -p sonos-stream --example live_demo
//!
//! Then change volume, play/pause, group/ungroup speakers and watch the output.

use sonos_stream::{BrokerConfig, EventBroker, EventData, EventSource, Service};
use std::time::Duration;

const SERVICES: &[Service] = &[
    Service::AVTransport,
    Service::RenderingControl,
    Service::GroupRenderingControl,
    Service::ZoneGroupTopology,
    Service::GroupManagement,
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing so you can see internal broker/polling logs with RUST_LOG
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    println!("Sonos Live Demo");
    println!("===============");
    println!();

    // --- Discover speakers (blocking call — run off the async runtime) ---
    println!("Discovering speakers...");
    let devices = tokio::task::spawn_blocking(|| sonos_discovery::get())
        .await
        .expect("discovery task panicked");
    if devices.is_empty() {
        eprintln!("No Sonos devices found on the network.");
        return Ok(());
    }
    println!("Found {} speaker(s):\n", devices.len());
    for d in &devices {
        println!("  {} ({})", d.name, d.ip_address);
    }
    println!();

    // --- Create broker (force polling so it works behind any firewall) ---
    let config = BrokerConfig::firewall_simulation();
    let mut broker = EventBroker::new(config).await?;

    // Register every service on every speaker
    for device in &devices {
        let ip = device.ip_address.parse()?;
        for &svc in SERVICES {
            let reg = broker.register_speaker_service(ip, svc).await?;
            let mode = if reg.polling_reason.is_some() { "polling" } else { "UPnP" };
            println!("  Registered {:?} on {} [{}]", svc, device.name, mode);
        }
    }

    // Build a name lookup: ip -> speaker name
    let names: std::collections::HashMap<std::net::IpAddr, String> = devices
        .iter()
        .filter_map(|d| d.ip_address.parse().ok().map(|ip| (ip, d.name.clone())))
        .collect();

    println!();
    println!("Listening for events (Ctrl-C to quit)...");
    println!("  Change volume, play/pause, or group speakers to see events.");
    println!();

    // --- Event loop ---
    let mut events = broker.event_iterator()?;
    let mut count: u64 = 0;

    loop {
        match events.next_timeout(Duration::from_secs(60)).await {
            Ok(Some(event)) => {
                count += 1;
                let speaker = names
                    .get(&event.speaker_ip)
                    .map(String::as_str)
                    .unwrap_or("unknown");
                let source = match &event.event_source {
                    EventSource::UPnPNotification { .. } => "UPnP",
                    EventSource::PollingDetection { poll_interval } => {
                        if poll_interval.as_secs() > 0 {
                            "poll"
                        } else {
                            "poll"
                        }
                    }
                };

                print!("[{count}] {speaker} ({source}) ");

                match &event.event_data {
                    EventData::AVTransport(s) => {
                        let state = s.transport_state.as_deref().unwrap_or("?");
                        let track = s.current_track_uri.as_deref().unwrap_or("-");
                        let pos = s.rel_time.as_deref().unwrap_or("");
                        let dur = s.track_duration.as_deref().unwrap_or("");
                        let mode = s.play_mode.as_deref().unwrap_or("");
                        println!("AVTransport  state={state}  track={track}  pos={pos}/{dur}  mode={mode}");
                    }
                    EventData::RenderingControl(s) => {
                        let vol = s.master_volume.as_deref().unwrap_or("?");
                        let mute = s.master_mute.as_deref().unwrap_or("?");
                        let bass = s.bass.as_deref().unwrap_or("-");
                        let treble = s.treble.as_deref().unwrap_or("-");
                        println!("RenderingControl  vol={vol}  mute={mute}  bass={bass}  treble={treble}");
                    }
                    EventData::GroupRenderingControl(s) => {
                        let vol = s.group_volume.map(|v| v.to_string()).unwrap_or("?".into());
                        let mute = s.group_mute.map(|m| m.to_string()).unwrap_or("?".into());
                        println!("GroupRenderingControl  vol={vol}  mute={mute}");
                    }
                    EventData::ZoneGroupTopology(s) => {
                        let groups = s.zone_groups.len();
                        let speakers: usize = s.zone_groups.iter()
                            .map(|g| g.members.len())
                            .sum();
                        let group_names: Vec<String> = s.zone_groups.iter().map(|g| {
                            let room_names: Vec<&str> = g.members.iter()
                                .map(|m| m.zone_name.as_str())
                                .collect();
                            format!("[{}]", room_names.join(" + "))
                        }).collect();
                        println!("ZoneGroupTopology  {groups} group(s), {speakers} speaker(s): {}", group_names.join(", "));
                    }
                    EventData::GroupManagement(s) => {
                        let local = s.group_coordinator_is_local.map(|b| b.to_string()).unwrap_or("-".into());
                        let uuid = s.local_group_uuid.as_deref().unwrap_or("-");
                        println!("GroupManagement  coordinator_local={local}  group={uuid}");
                    }
                    EventData::DeviceProperties(s) => {
                        let name = s.zone_name.as_deref().unwrap_or("-");
                        let model = s.model_name.as_deref().unwrap_or("-");
                        println!("DeviceProperties  zone={name}  model={model}");
                    }
                }
            }
            Ok(None) => {
                println!("Event stream closed.");
                break;
            }
            Err(_) => {
                println!("(no events in 60s — waiting...)");
            }
        }
    }

    broker.shutdown().await?;
    Ok(())
}
