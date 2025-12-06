//! Test helpers for fixture-based integration tests

use std::fs;
use std::path::PathBuf;

/// Represents a test fixture with device XML data
#[derive(Debug, Clone)]
pub struct DeviceFixture {
    pub name: String,
    pub ip: String,
    pub xml_content: String,
}

impl DeviceFixture {
    /// Load a fixture from the fixtures directory
    pub fn load(filename: &str, ip: &str) -> Self {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("tests/fixtures");
        path.push(filename);

        let xml_content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load fixture {}: {}", filename, e));

        Self {
            name: filename.to_string(),
            ip: ip.to_string(),
            xml_content,
        }
    }

    /// Get the SSDP location URL for this device
    pub fn location_url(&self) -> String {
        format!("http://{}:1400/xml/device_description.xml", self.ip)
    }

    /// Create a mock SSDP response for this device
    pub fn ssdp_response(&self, usn_suffix: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\n\
             CACHE-CONTROL: max-age = 1800\r\n\
             EXT:\r\n\
             LOCATION: {}\r\n\
             SERVER: Linux UPnP/1.0 Sonos/70.3-88200 (ZPS9)\r\n\
             ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
             USN: uuid:RINCON_{}::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
             X-RINCON-BOOTSEQ: 123\r\n\
             X-RINCON-HOUSEHOLD: Sonos_test\r\n\r\n",
            self.location_url(),
            usn_suffix
        )
    }
}

/// Collection of device fixtures for testing scenarios
pub struct FixtureSet {
    pub devices: Vec<DeviceFixture>,
}

impl FixtureSet {
    /// Create a fixture set with specific devices
    pub fn new(devices: Vec<DeviceFixture>) -> Self {
        Self { devices }
    }

    /// Single Sonos One device
    pub fn single_sonos_one() -> Self {
        Self::new(vec![DeviceFixture::load(
            "sonos_one_device.xml",
            "192.168.1.100",
        )])
    }

    /// Multiple different Sonos devices
    pub fn multiple_devices() -> Self {
        Self::new(vec![
            DeviceFixture::load("sonos_one_device.xml", "192.168.1.100"),
            DeviceFixture::load("sonos_play1_device.xml", "192.168.1.101"),
            DeviceFixture::load("sonos_playbar_device.xml", "192.168.1.102"),
        ])
    }

    /// All available Sonos device types
    pub fn all_sonos_devices() -> Self {
        Self::new(vec![
            DeviceFixture::load("sonos_one_device.xml", "192.168.1.100"),
            DeviceFixture::load("sonos_play1_device.xml", "192.168.1.101"),
            DeviceFixture::load("sonos_playbar_device.xml", "192.168.1.102"),
            DeviceFixture::load("sonos_amp_device.xml", "192.168.1.103"),
            DeviceFixture::load("sonos_roam_device.xml", "192.168.1.104"),
        ])
    }

    /// Mix of Sonos and non-Sonos devices (for filtering tests)
    pub fn mixed_devices() -> Self {
        Self::new(vec![
            DeviceFixture::load("sonos_one_device.xml", "192.168.1.100"),
            DeviceFixture::load("non_sonos_router_device.xml", "192.168.1.200"),
            DeviceFixture::load("sonos_play1_device.xml", "192.168.1.101"),
        ])
    }

    /// Minimal Sonos device
    pub fn minimal_device() -> Self {
        Self::new(vec![DeviceFixture::load(
            "minimal_sonos_device.xml",
            "192.168.1.100",
        )])
    }

    /// Empty set (no devices)
    pub fn empty() -> Self {
        Self::new(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_fixture() {
        let fixture = DeviceFixture::load("sonos_one_device.xml", "192.168.1.100");
        assert!(!fixture.xml_content.is_empty());
        assert!(fixture.xml_content.contains("<?xml"));
        assert!(fixture.xml_content.contains("Sonos"));
    }

    #[test]
    fn test_location_url() {
        let fixture = DeviceFixture::load("sonos_one_device.xml", "192.168.1.100");
        assert_eq!(
            fixture.location_url(),
            "http://192.168.1.100:1400/xml/device_description.xml"
        );
    }

    #[test]
    fn test_ssdp_response() {
        let fixture = DeviceFixture::load("sonos_one_device.xml", "192.168.1.100");
        let response = fixture.ssdp_response("TEST123");
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("LOCATION: http://192.168.1.100:1400"));
        assert!(response.contains("uuid:RINCON_TEST123"));
    }

    #[test]
    fn test_fixture_sets() {
        assert_eq!(FixtureSet::single_sonos_one().devices.len(), 1);
        assert_eq!(FixtureSet::multiple_devices().devices.len(), 3);
        assert_eq!(FixtureSet::all_sonos_devices().devices.len(), 5);
        assert_eq!(FixtureSet::mixed_devices().devices.len(), 3);
        assert_eq!(FixtureSet::minimal_device().devices.len(), 1);
        assert_eq!(FixtureSet::empty().devices.len(), 0);
    }
}
