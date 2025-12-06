//! Fixture-based integration tests for Sonos device discovery
//!
//! These tests use pre-captured device XML fixtures to test discovery
//! scenarios without requiring real devices on the network.

mod helpers;

use helpers::{DeviceFixture, FixtureSet};
use mockito::Server;
use rstest::rstest;
use sonos_discovery::device::DeviceDescription;

/// Test parsing device XML from various fixture files
#[rstest]
#[case("sonos_one_device.xml", "Sonos One", "Bedroom")]
#[case("sonos_play1_device.xml", "Sonos Play:1", "Dining Room")]
#[case("sonos_playbar_device.xml", "Sonos Playbar", "TV Room")]
#[case("sonos_amp_device.xml", "Sonos Amp", "Living Room")]
#[case("sonos_roam_device.xml", "Sonos Roam 2", "Roam / Office")]
fn test_parse_device_fixture(
    #[case] fixture_file: &str,
    #[case] expected_model: &str,
    #[case] expected_room: &str,
) {
    let fixture = DeviceFixture::load(fixture_file, "192.168.1.100");
    let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
        .expect("Failed to parse device XML");

    assert_eq!(device_desc.model_name, expected_model);
    assert!(device_desc.is_sonos_device());

    let device = device_desc.to_device("192.168.1.100".to_string());
    assert_eq!(device.room_name, expected_room);
    assert_eq!(device.model_name, expected_model);
    assert_eq!(device.ip_address, "192.168.1.100");
    assert_eq!(device.port, 1400);
}

/// Test that all Sonos device fixtures are correctly identified as Sonos devices
#[rstest]
#[case("sonos_one_device.xml")]
#[case("sonos_play1_device.xml")]
#[case("sonos_playbar_device.xml")]
#[case("sonos_amp_device.xml")]
#[case("sonos_roam_device.xml")]
#[case("minimal_sonos_device.xml")]
fn test_sonos_device_identification(#[case] fixture_file: &str) {
    let fixture = DeviceFixture::load(fixture_file, "192.168.1.100");
    let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
        .expect("Failed to parse device XML");

    assert!(
        device_desc.is_sonos_device(),
        "{} should be identified as a Sonos device",
        fixture_file
    );
}

/// Test that non-Sonos devices are correctly filtered out
#[test]
fn test_non_sonos_device_filtering() {
    let fixture = DeviceFixture::load("non_sonos_router_device.xml", "192.168.1.200");
    let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
        .expect("Failed to parse device XML");

    assert!(
        !device_desc.is_sonos_device(),
        "Router device should not be identified as Sonos"
    );
}

/// Test device parsing with different fixture sets
#[rstest]
#[case(FixtureSet::single_sonos_one(), 1, "Single device")]
#[case(FixtureSet::multiple_devices(), 3, "Multiple devices")]
#[case(FixtureSet::all_sonos_devices(), 5, "All Sonos devices")]
#[case(FixtureSet::minimal_device(), 1, "Minimal device")]
#[case(FixtureSet::empty(), 0, "No devices")]
fn test_fixture_set_parsing(
    #[case] fixture_set: FixtureSet,
    #[case] expected_count: usize,
    #[case] scenario: &str,
) {
    let mut parsed_devices = Vec::new();

    for fixture in &fixture_set.devices {
        let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
            .unwrap_or_else(|e| panic!("Failed to parse {} in {}: {}", fixture.name, scenario, e));

        if device_desc.is_sonos_device() {
            let device = device_desc.to_device(fixture.ip.to_string());
            parsed_devices.push(device);
        }
    }

    assert_eq!(
        parsed_devices.len(),
        expected_count,
        "Scenario '{}' should have {} Sonos devices",
        scenario,
        expected_count
    );

    // Verify all devices have required fields
    for device in &parsed_devices {
        assert!(!device.id.is_empty(), "Device ID should not be empty");
        assert!(!device.name.is_empty(), "Device name should not be empty");
        assert!(!device.ip_address.is_empty(), "Device IP should not be empty");
        assert!(!device.model_name.is_empty(), "Device model should not be empty");
        assert_eq!(device.port, 1400, "Sonos devices use port 1400");
    }
}

/// Test mixed device scenarios (Sonos and non-Sonos)
#[test]
fn test_mixed_device_filtering() {
    let fixture_set = FixtureSet::mixed_devices();
    let mut sonos_count = 0;
    let mut non_sonos_count = 0;

    for fixture in &fixture_set.devices {
        let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
            .expect("Failed to parse device XML");

        if device_desc.is_sonos_device() {
            sonos_count += 1;
        } else {
            non_sonos_count += 1;
        }
    }

    assert_eq!(sonos_count, 2, "Should have 2 Sonos devices");
    assert_eq!(non_sonos_count, 1, "Should have 1 non-Sonos device");
}

/// Test device uniqueness by ID
#[test]
fn test_device_id_uniqueness() {
    let fixture_set = FixtureSet::all_sonos_devices();
    let mut device_ids = std::collections::HashSet::new();

    for fixture in &fixture_set.devices {
        let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
            .expect("Failed to parse device XML");

        let device = device_desc.to_device(fixture.ip.to_string());

        assert!(
            device_ids.insert(device.id.clone()),
            "Duplicate device ID found: {}",
            device.id
        );
    }

    assert_eq!(
        device_ids.len(),
        fixture_set.devices.len(),
        "All devices should have unique IDs"
    );
}

/// Test device IP address assignment
#[rstest]
#[case("192.168.1.100")]
#[case("10.0.0.50")]
#[case("172.16.0.10")]
fn test_device_ip_assignment(#[case] ip_address: &str) {
    let fixture = DeviceFixture::load("sonos_one_device.xml", ip_address);
    let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
        .expect("Failed to parse device XML");

    let device = device_desc.to_device(ip_address.to_string());

    assert_eq!(device.ip_address, ip_address);
    assert_eq!(device.port, 1400);
}

/// Test minimal device fixture has all required fields
#[test]
fn test_minimal_device_completeness() {
    let fixture = DeviceFixture::load("minimal_sonos_device.xml", "192.168.1.100");
    let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
        .expect("Failed to parse minimal device XML");

    assert!(device_desc.is_sonos_device());
    assert!(!device_desc.friendly_name.is_empty());
    assert!(!device_desc.model_name.is_empty());
    assert!(!device_desc.udn.is_empty());

    let device = device_desc.to_device("192.168.1.100".to_string());
    assert!(!device.id.is_empty());
    assert!(!device.name.is_empty());
    assert!(!device.room_name.is_empty());
}

/// Test HTTP mock server with fixture data
#[test]
fn test_http_mock_with_fixture() {
    let mut server = Server::new();
    let fixture = DeviceFixture::load("sonos_one_device.xml", "192.168.1.100");

    let mock = server
        .mock("GET", "/xml/device_description.xml")
        .with_status(200)
        .with_header("content-type", "text/xml")
        .with_body(&fixture.xml_content)
        .create();

    // Simulate HTTP request
    let url = format!("{}/xml/device_description.xml", server.url());
    let response = reqwest::blocking::get(&url).expect("Failed to make request");
    let xml = response.text().expect("Failed to read response");

    let device_desc = DeviceDescription::from_xml(&xml).expect("Failed to parse XML");
    assert!(device_desc.is_sonos_device());

    mock.assert();
}

/// Test multiple HTTP mocks for different devices
#[test]
fn test_multiple_device_mocks() {
    let mut server = Server::new();
    let fixtures = vec![
        DeviceFixture::load("sonos_one_device.xml", "192.168.1.100"),
        DeviceFixture::load("sonos_play1_device.xml", "192.168.1.101"),
    ];

    let mut mocks = Vec::new();
    for (i, fixture) in fixtures.iter().enumerate() {
        let path = format!("/device{}/xml/device_description.xml", i);
        let mock = server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_header("content-type", "text/xml")
            .with_body(&fixture.xml_content)
            .create();
        mocks.push(mock);
    }

    // Fetch each device
    for (i, _fixture) in fixtures.iter().enumerate() {
        let url = format!("{}/device{}/xml/device_description.xml", server.url(), i);
        let response = reqwest::blocking::get(&url).expect("Failed to make request");
        let xml = response.text().expect("Failed to read response");

        let device_desc = DeviceDescription::from_xml(&xml).expect("Failed to parse XML");
        assert!(device_desc.is_sonos_device());
    }

    for mock in mocks {
        mock.assert();
    }
}

/// Test error handling with invalid XML
#[test]
fn test_invalid_xml_handling() {
    let invalid_xml = "<?xml version=\"1.0\"?><invalid>not a device</invalid>";
    let result = DeviceDescription::from_xml(invalid_xml);

    assert!(result.is_err(), "Should fail to parse invalid XML");
}

/// Test error handling with malformed XML
#[test]
fn test_malformed_xml_handling() {
    let malformed_xml = "not xml at all";
    let result = DeviceDescription::from_xml(malformed_xml);

    assert!(result.is_err(), "Should fail to parse malformed XML");
}

/// Test device field extraction from various fixtures
#[rstest]
#[case("sonos_one_device.xml", "uuid:RINCON_7828CA0E1E1801400")]
#[case("minimal_sonos_device.xml", "uuid:RINCON_TEST123456")]
fn test_device_id_extraction(#[case] fixture_file: &str, #[case] expected_id: &str) {
    let fixture = DeviceFixture::load(fixture_file, "192.168.1.100");
    let device_desc = DeviceDescription::from_xml(&fixture.xml_content)
        .expect("Failed to parse device XML");

    assert_eq!(device_desc.udn, expected_id);

    let device = device_desc.to_device("192.168.1.100".to_string());
    assert_eq!(device.id, expected_id);
}
