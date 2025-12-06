//! Device description parsing and validation.
//!
//! This module handles parsing UPnP device description XML and validating
//! that devices are Sonos speakers.

use crate::error::{DiscoveryError, Result};
use crate::Device;
use serde::Deserialize;

/// UPnP device description root element.
#[derive(Debug, Deserialize)]
pub struct Root {
    pub device: DeviceDescription,
}

/// Internal device description parsed from XML.
///
/// This structure represents the UPnP device description format used by Sonos devices.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDescription {
    pub device_type: String,
    pub friendly_name: String,
    pub manufacturer: String,
    pub manufacturer_url: Option<String>,
    pub model_description: Option<String>,
    pub model_name: String,
    pub model_number: Option<String>,
    pub model_url: Option<String>,
    pub serial_number: Option<String>,
    #[serde(rename = "UDN")]
    pub udn: String,
    pub room_name: Option<String>,
    pub display_name: Option<String>,
}

impl DeviceDescription {
    /// Parse device description from XML.
    ///
    /// # Arguments
    ///
    /// * `xml` - UPnP device description XML string
    ///
    /// # Errors
    ///
    /// Returns `DiscoveryError::ParseError` if the XML is malformed or missing required fields.
    pub fn from_xml(xml: &str) -> Result<Self> {
        let root: Root = quick_xml::de::from_str(xml)
            .map_err(|e| DiscoveryError::ParseError(format!("Failed to parse device XML: {}", e)))?;

        Ok(root.device)
    }

    /// Convert device description to public Device type.
    ///
    /// # Arguments
    ///
    /// * `ip_address` - IP address extracted from the device's location URL
    pub fn to_device(&self, ip_address: String) -> Device {
        Device {
            id: self.udn.clone(),
            name: self.friendly_name.clone(),
            room_name: self
                .room_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            ip_address,
            port: 1400,
            model_name: self.model_name.clone(),
        }
    }

    /// Check if this device is a Sonos device.
    ///
    /// Validates by checking manufacturer name and device type.
    pub fn is_sonos_device(&self) -> bool {
        self.manufacturer.to_lowercase().contains("sonos")
            || self.device_type.contains("ZonePlayer")
            || self.device_type.contains("MediaRenderer")
    }
}

/// Extract IP address from a URL.
///
/// # Arguments
///
/// * `url` - URL string (e.g., "http://192.168.1.100:1400/xml/device_description.xml")
///
/// # Returns
///
/// The IP address portion of the URL, or `None` if the URL is malformed.
pub fn extract_ip_from_url(url: &str) -> Option<String> {
    url.split("//")
        .nth(1)?
        .split(':')
        .next()
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ip_from_url() {
        assert_eq!(
            extract_ip_from_url("http://192.168.1.100:1400/xml/device_description.xml"),
            Some("192.168.1.100".to_string())
        );
        assert_eq!(
            extract_ip_from_url("https://10.0.0.5:8080/path"),
            Some("10.0.0.5".to_string())
        );
        assert_eq!(extract_ip_from_url("invalid-url"), None);
    }

    #[test]
    fn test_device_from_xml() {
        // Test with a minimal valid Sonos device XML
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>Living Room</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>Sonos One</modelName>
    <UDN>uuid:RINCON_000E58A0123456</UDN>
    <roomName>Living Room</roomName>
  </device>
</root>"#;

        let device = DeviceDescription::from_xml(xml).unwrap();

        assert_eq!(device.friendly_name, "Living Room");
        assert_eq!(device.manufacturer, "Sonos, Inc.");
        assert_eq!(device.model_name, "Sonos One");
        assert_eq!(device.udn, "uuid:RINCON_000E58A0123456");
        assert_eq!(device.room_name, Some("Living Room".to_string()));
        assert!(device.is_sonos_device());
    }

    #[test]
    fn test_is_sonos_device_by_manufacturer() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>Test Device</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>Test Model</modelName>
    <UDN>uuid:TEST123</UDN>
  </device>
</root>"#;

        let device = DeviceDescription::from_xml(xml).unwrap();
        assert!(device.is_sonos_device());
    }

    #[test]
    fn test_is_sonos_device_by_device_type() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>Test Device</friendlyName>
    <manufacturer>Other Manufacturer</manufacturer>
    <modelName>Test Model</modelName>
    <UDN>uuid:TEST123</UDN>
  </device>
</root>"#;

        let device = DeviceDescription::from_xml(xml).unwrap();
        assert!(device.is_sonos_device());
    }

    #[test]
    fn test_not_sonos_device() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:Basic:1</deviceType>
    <friendlyName>Router</friendlyName>
    <manufacturer>Other Company</manufacturer>
    <modelName>Router Model</modelName>
    <UDN>uuid:ROUTER123</UDN>
  </device>
</root>"#;

        let device = DeviceDescription::from_xml(xml).unwrap();
        assert!(!device.is_sonos_device());
    }

    #[test]
    fn test_to_device_conversion() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>Kitchen</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>Sonos Play:1</modelName>
    <UDN>uuid:RINCON_ABCDEF123456</UDN>
    <roomName>Kitchen</roomName>
  </device>
</root>"#;

        let device_desc = DeviceDescription::from_xml(xml).unwrap();
        let device = device_desc.to_device("192.168.1.50".to_string());

        assert_eq!(device.id, "uuid:RINCON_ABCDEF123456");
        assert_eq!(device.name, "Kitchen");
        assert_eq!(device.room_name, "Kitchen");
        assert_eq!(device.ip_address, "192.168.1.50");
        assert_eq!(device.port, 1400);
        assert_eq!(device.model_name, "Sonos Play:1");
    }

    #[test]
    fn test_to_device_with_missing_room_name() {
        let xml = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>Bedroom</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>Sonos One</modelName>
    <UDN>uuid:RINCON_XYZ789</UDN>
  </device>
</root>"#;

        let device_desc = DeviceDescription::from_xml(xml).unwrap();
        let device = device_desc.to_device("192.168.1.100".to_string());

        assert_eq!(device.room_name, "Unknown");
    }
}
