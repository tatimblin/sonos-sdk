//! Debug topology to understand GetZoneGroupState response

use sonos_state::StateError;
use sonos_api::services::zone_group_topology::get_zone_group_state;
use sonos_api::services::zone_group_topology::events::ZoneGroupTopologyEvent;
use sonos_api::SonosClient;
use std::net::IpAddr;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Debug GetZoneGroupState topology...");

    // Step 1: Discover devices
    let devices = sonos_discovery::get_with_timeout(Duration::from_secs(5));

    if devices.is_empty() {
        println!("❌ No Sonos devices found on the network.");
        return Ok(());
    }

    println!("✅ Found {} devices:", devices.len());
    for (i, device) in devices.iter().enumerate() {
        println!("  {}: {} ({}) at {}", i + 1, device.name, device.model_name, device.ip_address);
    }

    let device_ip: IpAddr = devices[0].ip_address.parse()?;
    println!("\n🔍 Using '{}' at {} for GetZoneGroupState", devices[0].name, device_ip);

    // Step 2: Execute GetZoneGroupState operation
    let client = SonosClient::new();

    let operation = get_zone_group_state()
        .build()
        .map_err(|e| StateError::Init(format!("Failed to build operation: {}", e)))?;

    println!("\n📡 Executing GetZoneGroupState operation...");

    let response = client
        .execute_enhanced(&device_ip.to_string(), operation)
        .map_err(|e| StateError::Init(format!("Failed to get topology: {}", e)))?;

    println!("\n✅ Got raw response:");
    println!("Zone group state length: {} characters", response.zone_group_state.len());
    println!("Raw XML: {}", &response.zone_group_state[..std::cmp::min(500, response.zone_group_state.len())]);
    if response.zone_group_state.len() > 500 {
        println!("... [truncated]");
    }

    // Check if the raw response is already HTML-encoded
    if response.zone_group_state.contains("&lt;") {
        println!("⚠️  Raw XML is already HTML-encoded!");
    } else {
        println!("✅ Raw XML appears to be proper XML");
    }

    // Step 3: Extract inner content and HTML-encode for event parsing
    let inner_content = if response.zone_group_state.starts_with("<ZoneGroupState>")
        && response.zone_group_state.ends_with("</ZoneGroupState>") {
        // Extract content between <ZoneGroupState> and </ZoneGroupState>
        &response.zone_group_state[16..response.zone_group_state.len()-17]
    } else {
        // If it doesn't have the wrapper, use as-is
        &response.zone_group_state
    };

    let xml = format!(
        r#"<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">
            <e:property><ZoneGroupState>{}</ZoneGroupState></e:property>
        </e:propertyset>"#,
        escape_xml(inner_content)
    );

    println!("\n🔄 Parsing as ZoneGroupTopologyEvent (with HTML-encoded content)...");

    match ZoneGroupTopologyEvent::from_xml(&xml) {
        Ok(event) => {
            let zone_groups = event.zone_groups();
            println!("✅ Successfully parsed {} zone groups:", zone_groups.len());

            for (i, zone_group) in zone_groups.iter().enumerate() {
                println!("  Group {}: ID={}, Coordinator={}, Members={}",
                    i + 1, zone_group.id, zone_group.coordinator, zone_groups[i].members.len());

                for (j, member) in zone_group.members.iter().enumerate() {
                    println!("    Member {}: UUID={}, Name={}, Location={}",
                        j + 1, member.uuid, member.zone_name, member.location);
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to parse topology: {}", e);
            println!("Wrapped XML: {}", xml);
        }
    }

    Ok(())
}

/// Escape XML special characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}