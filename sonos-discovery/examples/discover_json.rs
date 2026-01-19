//! Simple speaker discovery that outputs JSON for scripting
//!
//! Usage: cargo run -p sonos-discovery --example discover_json

use serde::Serialize;
use sonos_discovery::get_with_timeout;
use std::time::Duration;

#[derive(Serialize)]
struct SpeakerInfo {
    id: String,
    name: String,
    room_name: String,
    ip_address: String,
    port: u16,
    model_name: String,
}

fn main() {
    let timeout = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let devices = get_with_timeout(Duration::from_secs(timeout));

    let speakers: Vec<SpeakerInfo> = devices
        .into_iter()
        .map(|d| SpeakerInfo {
            id: d.id,
            name: d.name,
            room_name: d.room_name,
            ip_address: d.ip_address,
            port: d.port,
            model_name: d.model_name,
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&speakers).unwrap());
}
