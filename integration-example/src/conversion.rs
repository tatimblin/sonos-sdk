//! Device conversion utilities for converting between different crate types.
//!
//! This module provides conversion functions between `sonos_discovery::Device` 
//! and `sonos_stream::Speaker` types, handling IP address parsing and error cases.

use std::net::IpAddr;
use thiserror::Error;

use sonos_discovery::Device as DiscoveryDevice;
use sonos_stream::{Speaker, SpeakerId};

/// Errors that can occur during device conversion.
#[derive(Error, Debug)]
pub enum ConversionError {
    /// Invalid IP address format in the discovery device
    #[error("Invalid IP address format: {ip_address}")]
    InvalidIpAddress { ip_address: String },
    
    /// Missing required field in the discovery device
    #[error("Missing required field: {field}")]
    MissingField { field: String },
}

/// Device converter for transforming discovery devices to streaming speakers.
pub struct DeviceConverter;

impl DeviceConverter {
    /// Convert a `sonos_discovery::Device` to a `sonos_stream::Speaker`.
    ///
    /// This function handles the type conversion between the discovery and streaming
    /// crates, parsing the IP address string and creating the appropriate wrapper types.
    ///
    /// # Arguments
    ///
    /// * `discovery_device` - The device from the discovery crate
    ///
    /// # Returns
    ///
    /// A `Result` containing the converted `Speaker` or a `ConversionError` if the
    /// conversion fails due to invalid IP address or missing fields.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use integration_example::conversion::DeviceConverter;
    /// use sonos_discovery::Device;
    ///
    /// let discovery_device = Device {
    ///     id: "uuid:RINCON_000E58A0123456".to_string(),
    ///     name: "Living Room".to_string(),
    ///     room_name: "Living Room".to_string(),
    ///     ip_address: "192.168.1.100".to_string(),
    ///     port: 1400,
    ///     model_name: "Sonos One".to_string(),
    /// };
    ///
    /// let speaker = DeviceConverter::discovery_to_stream(discovery_device)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn discovery_to_stream(discovery_device: DiscoveryDevice) -> Result<Speaker, ConversionError> {
        // Validate that required fields are present
        if discovery_device.id.is_empty() {
            return Err(ConversionError::MissingField {
                field: "id".to_string(),
            });
        }

        if discovery_device.name.is_empty() {
            return Err(ConversionError::MissingField {
                field: "name".to_string(),
            });
        }

        if discovery_device.ip_address.is_empty() {
            return Err(ConversionError::MissingField {
                field: "ip_address".to_string(),
            });
        }

        // Parse the IP address string to IpAddr
        let ip = discovery_device.ip_address.parse::<IpAddr>()
            .map_err(|_| ConversionError::InvalidIpAddress {
                ip_address: discovery_device.ip_address.clone(),
            })?;

        // Create the Speaker instance
        Ok(Speaker::new(
            SpeakerId::new(discovery_device.id),
            ip,
            discovery_device.name,
            discovery_device.room_name,
        ))
    }

    /// Convert multiple discovery devices to speakers, filtering out conversion failures.
    ///
    /// This is a convenience function that converts a collection of discovery devices
    /// and returns only the successfully converted speakers. Failed conversions are
    /// logged but do not stop the processing of other devices.
    ///
    /// # Arguments
    ///
    /// * `discovery_devices` - Iterator of discovery devices
    ///
    /// # Returns
    ///
    /// A vector of successfully converted speakers and a vector of conversion errors.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use integration_example::conversion::DeviceConverter;
    /// use sonos_discovery;
    ///
    /// let devices = sonos_discovery::get();
    /// let (speakers, errors) = DeviceConverter::convert_multiple(devices);
    ///
    /// println!("Converted {} devices, {} errors", speakers.len(), errors.len());
    /// ```
    pub fn convert_multiple<I>(discovery_devices: I) -> (Vec<Speaker>, Vec<ConversionError>)
    where
        I: IntoIterator<Item = DiscoveryDevice>,
    {
        let mut speakers = Vec::new();
        let mut errors = Vec::new();

        for device in discovery_devices {
            match Self::discovery_to_stream(device) {
                Ok(speaker) => speakers.push(speaker),
                Err(error) => {
                    tracing::warn!("Failed to convert device: {}", error);
                    errors.push(error);
                }
            }
        }

        (speakers, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_device() -> DiscoveryDevice {
        DiscoveryDevice {
            id: "uuid:RINCON_000E58A0123456".to_string(),
            name: "Living Room".to_string(),
            room_name: "Living Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }
    }

    #[test]
    fn test_successful_conversion() {
        let discovery_device = create_test_device();
        let speaker = DeviceConverter::discovery_to_stream(discovery_device).unwrap();

        assert_eq!(speaker.id.as_str(), "uuid:RINCON_000E58A0123456");
        assert_eq!(speaker.name, "Living Room");
        assert_eq!(speaker.room, "Living Room");
        assert_eq!(speaker.ip.to_string(), "192.168.1.100");
    }

    #[test]
    fn test_invalid_ip_address() {
        let mut discovery_device = create_test_device();
        discovery_device.ip_address = "invalid-ip".to_string();

        let result = DeviceConverter::discovery_to_stream(discovery_device);
        assert!(matches!(result, Err(ConversionError::InvalidIpAddress { .. })));
    }

    #[test]
    fn test_empty_id() {
        let mut discovery_device = create_test_device();
        discovery_device.id = "".to_string();

        let result = DeviceConverter::discovery_to_stream(discovery_device);
        assert!(matches!(result, Err(ConversionError::MissingField { .. })));
    }

    #[test]
    fn test_empty_name() {
        let mut discovery_device = create_test_device();
        discovery_device.name = "".to_string();

        let result = DeviceConverter::discovery_to_stream(discovery_device);
        assert!(matches!(result, Err(ConversionError::MissingField { .. })));
    }

    #[test]
    fn test_empty_ip_address() {
        let mut discovery_device = create_test_device();
        discovery_device.ip_address = "".to_string();

        let result = DeviceConverter::discovery_to_stream(discovery_device);
        assert!(matches!(result, Err(ConversionError::MissingField { .. })));
    }

    #[test]
    fn test_ipv6_address() {
        let mut discovery_device = create_test_device();
        discovery_device.ip_address = "2001:db8::1".to_string();

        let speaker = DeviceConverter::discovery_to_stream(discovery_device).unwrap();
        assert_eq!(speaker.ip.to_string(), "2001:db8::1");
    }

    #[test]
    fn test_convert_multiple_success() {
        let devices = vec![
            create_test_device(),
            {
                let mut device = create_test_device();
                device.id = "uuid:RINCON_ABCDEF123456".to_string();
                device.name = "Kitchen".to_string();
                device.room_name = "Kitchen".to_string();
                device.ip_address = "192.168.1.101".to_string();
                device
            },
        ];

        let (speakers, errors) = DeviceConverter::convert_multiple(devices);
        assert_eq!(speakers.len(), 2);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_convert_multiple_with_errors() {
        let devices = vec![
            create_test_device(),
            {
                let mut device = create_test_device();
                device.ip_address = "invalid-ip".to_string();
                device
            },
            {
                let mut device = create_test_device();
                device.id = "".to_string();
                device
            },
        ];

        let (speakers, errors) = DeviceConverter::convert_multiple(devices);
        assert_eq!(speakers.len(), 1);
        assert_eq!(errors.len(), 2);
    }
}