//! Helper test to capture SSDP responses and device XML from real devices
//!
//! This test is designed to be run manually when you have Sonos devices on your network.
//! It will capture the actual SSDP responses and device XML that can be used to create
//! test fixtures for mocking in future tests.
//!
//! Run with: cargo test --test capture_fixtures -- --nocapture --ignored

use sonos_discovery::{get_iter_with_timeout, DeviceEvent};
use std::time::Duration;

#[test]
#[ignore] // Ignored by default - run manually with --ignored flag
fn capture_real_device_data() {
    println!("\n=== Capturing Real Sonos Device Data ===\n");
    println!("This test will discover real Sonos devices and display their information.");
    println!("You can use this data to create test fixtures for mocking.\n");
    
    let timeout = Duration::from_secs(3);
    let mut device_count = 0;
    
    for event in get_iter_with_timeout(timeout) {
        match event {
            DeviceEvent::Found(device) => {
                device_count += 1;
                
                println!("--- Device {} ---", device_count);
                println!("ID:         {}", device.id);
                println!("Name:       {}", device.name);
                println!("Room:       {}", device.room_name);
                println!("IP:         {}", device.ip_address);
                println!("Port:       {}", device.port);
                println!("Model:      {}", device.model_name);
                println!();
                
                // Fetch the device XML for this device
                let url = format!("http://{}:{}/xml/device_description.xml", 
                    device.ip_address, device.port);
                
                println!("Fetching device XML from: {}", url);
                
                match reqwest::blocking::get(&url) {
                    Ok(response) => {
                        match response.text() {
                            Ok(xml) => {
                                println!("Device XML:");
                                println!("{}", xml);
                                println!();
                                
                                // Suggest fixture filename
                                let model_slug = device.model_name
                                    .to_lowercase()
                                    .replace(" ", "_")
                                    .replace(":", "");
                                let filename = format!("sonos_{}_device.xml", model_slug);
                                println!("Suggested fixture filename: {}", filename);
                                println!("Save this XML to: sonos-discovery/tests/fixtures/{}", filename);
                                println!();
                            }
                            Err(e) => {
                                println!("Failed to read XML response: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("Failed to fetch device XML: {}", e);
                    }
                }
                
                println!("========================================\n");
            }
        }
    }
    
    if device_count == 0 {
        println!("No Sonos devices found on the network.");
        println!("Make sure you have Sonos devices powered on and connected to the same network.");
    } else {
        println!("Total devices captured: {}", device_count);
        println!("\nTo use this data for mocking:");
        println!("1. Create directory: sonos-discovery/tests/fixtures/");
        println!("2. Save each device XML to a separate file in that directory");
        println!("3. Use these fixtures in your tests to mock HTTP responses");
    }
}

#[test]
#[ignore]
fn capture_ssdp_response_format() {
    println!("\n=== SSDP Response Format ===\n");
    println!("This test shows the format of SSDP responses received from Sonos devices.");
    println!("Use this information to create mock SSDP responses for testing.\n");
    
    println!("Typical Sonos SSDP M-SEARCH response format:");
    println!("---");
    println!("HTTP/1.1 200 OK");
    println!("CACHE-CONTROL: max-age = 1800");
    println!("EXT:");
    println!("LOCATION: http://192.168.1.100:1400/xml/device_description.xml");
    println!("SERVER: Linux UPnP/1.0 Sonos/70.3-88200 (ZPS9)");
    println!("ST: urn:schemas-upnp-org:device:ZonePlayer:1");
    println!("USN: uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1");
    println!("X-RINCON-BOOTSEQ: 123");
    println!("X-RINCON-HOUSEHOLD: Sonos_abcdefghijklmnopqrstuvwxyz");
    println!("---\n");
    
    println!("Key fields for Sonos device identification:");
    println!("- LOCATION: URL to fetch device description XML");
    println!("- SERVER: Contains 'Sonos' for Sonos devices");
    println!("- ST: Service type, should be 'ZonePlayer:1' for Sonos");
    println!("- USN: Unique service name, contains 'RINCON' for Sonos devices");
}

#[test]
#[ignore]
fn document_test_fixture_structure() {
    println!("\n=== Test Fixture Structure ===\n");
    println!("Recommended directory structure for test fixtures:\n");
    println!("sonos-discovery/tests/");
    println!("├── fixtures/");
    println!("│   ├── sonos_one_device.xml");
    println!("│   ├── sonos_playbar_device.xml");
    println!("│   ├── sonos_amp_device.xml");
    println!("│   ├── sonos_roam_device.xml");
    println!("│   ├── non_sonos_router_device.xml");
    println!("│   └── minimal_sonos_device.xml");
    println!("├── discovery_integration.rs");
    println!("├── capture_fixtures.rs");
    println!("└── resource_cleanup.rs\n");
    
    println!("Each fixture file should contain:");
    println!("- Complete XML device description");
    println!("- All required UPnP fields");
    println!("- Sonos-specific fields (roomName, etc.)");
    println!("- Representative data for that device model\n");
    
    println!("Example minimal fixture (minimal_sonos_device.xml):");
    println!("---");
    println!(r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>Test Room</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>Sonos One</modelName>
    <UDN>uuid:RINCON_TEST123456</UDN>
    <roomName>Test Room</roomName>
  </device>
</root>"#);
    println!("---\n");
}
