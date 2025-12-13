//! Device filtering and selection utilities.
//!
//! This module provides functions for finding target devices by name from discovered
//! devices, implementing fallback logic when devices are not found, and creating
//! user-friendly device listings for error cases.

use std::fmt;
use thiserror::Error;

use sonos_discovery::Device as DiscoveryDevice;
use sonos_stream::Speaker;

use crate::conversion::{ConversionError, DeviceConverter};

/// Errors that can occur during device selection.
#[derive(Error, Debug)]
pub enum SelectionError {
    /// Target device was not found among discovered devices
    #[error("Target device '{target_name}' not found. Available devices:\n{available_devices}")]
    DeviceNotFound {
        target_name: String,
        available_devices: String,
    },
    
    /// No devices were discovered on the network
    #[error("No Sonos devices found on the network. Please check:\n- Network connectivity\n- Devices are powered on\n- Devices are on the same network")]
    NoDevicesFound,
    
    /// Device conversion failed
    #[error("Failed to convert device: {0}")]
    ConversionFailed(#[from] ConversionError),
}

/// Device selector for finding and filtering Sonos devices.
pub struct DeviceSelector;

impl DeviceSelector {
    /// Find a target device by name from a list of discovered devices.
    ///
    /// This function searches for a device with the exact name match (case-insensitive)
    /// and converts it to a Speaker for use with the streaming crate.
    ///
    /// # Arguments
    ///
    /// * `target_name` - The name of the device to find
    /// * `discovered_devices` - List of devices from discovery
    ///
    /// # Returns
    ///
    /// A `Result` containing the converted `Speaker` or a `SelectionError` if the
    /// device is not found or conversion fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use integration_example::device_selection::DeviceSelector;
    /// use sonos_discovery;
    ///
    /// let devices = sonos_discovery::get();
    /// let speaker = DeviceSelector::find_device_by_name("Sonos Roam 2", devices)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn find_device_by_name(
        target_name: &str,
        discovered_devices: Vec<DiscoveryDevice>,
    ) -> Result<Speaker, SelectionError> {
        if discovered_devices.is_empty() {
            return Err(SelectionError::NoDevicesFound);
        }

        // Try to find the device by exact name match (case-insensitive)
        let target_device = discovered_devices
            .iter()
            .find(|device| device.name.to_lowercase() == target_name.to_lowercase())
            .cloned();

        match target_device {
            Some(device) => {
                // Convert the found device to a Speaker
                DeviceConverter::discovery_to_stream(device)
                    .map_err(SelectionError::from)
            }
            None => {
                // Device not found, create a helpful error message
                let available_devices = Self::format_device_list(&discovered_devices);
                Err(SelectionError::DeviceNotFound {
                    target_name: target_name.to_string(),
                    available_devices,
                })
            }
        }
    }

    /// Find a device by name with fallback options.
    ///
    /// This function first tries to find the exact device name, but if not found,
    /// it provides suggestions based on partial matches or similar names.
    ///
    /// # Arguments
    ///
    /// * `target_name` - The name of the device to find
    /// * `discovered_devices` - List of devices from discovery
    ///
    /// # Returns
    ///
    /// A `Result` containing the converted `Speaker` or a `SelectionError` with
    /// suggestions for similar device names.
    pub fn find_device_with_fallback(
        target_name: &str,
        discovered_devices: Vec<DiscoveryDevice>,
    ) -> Result<Speaker, SelectionError> {
        if discovered_devices.is_empty() {
            return Err(SelectionError::NoDevicesFound);
        }

        // First try exact match
        if let Ok(speaker) = Self::find_device_by_name(target_name, discovered_devices.clone()) {
            return Ok(speaker);
        }

        // Try partial matches
        let partial_matches: Vec<&DiscoveryDevice> = discovered_devices
            .iter()
            .filter(|device| {
                device.name.to_lowercase().contains(&target_name.to_lowercase())
                    || target_name.to_lowercase().contains(&device.name.to_lowercase())
            })
            .collect();

        if partial_matches.len() == 1 {
            // Single partial match found, use it
            let device = partial_matches[0].clone();
            tracing::info!(
                "Exact match not found for '{}', using partial match: '{}'",
                target_name,
                device.name
            );
            return DeviceConverter::discovery_to_stream(device)
                .map_err(SelectionError::from);
        }

        // No exact or single partial match, return error with suggestions
        let mut available_devices = Self::format_device_list(&discovered_devices);
        
        if !partial_matches.is_empty() {
            available_devices.push_str("\n\nDid you mean one of these?\n");
            for device in partial_matches {
                available_devices.push_str(&format!("  - {} ({})\n", device.name, device.room_name));
            }
        }

        Err(SelectionError::DeviceNotFound {
            target_name: target_name.to_string(),
            available_devices,
        })
    }

    /// Get the first available device if no specific target is provided.
    ///
    /// This is a fallback function that returns the first discovered device,
    /// useful for testing or when the user doesn't specify a particular device.
    ///
    /// # Arguments
    ///
    /// * `discovered_devices` - List of devices from discovery
    ///
    /// # Returns
    ///
    /// A `Result` containing the first converted `Speaker` or a `SelectionError`
    /// if no devices are available.
    pub fn get_first_available(
        discovered_devices: Vec<DiscoveryDevice>,
    ) -> Result<Speaker, SelectionError> {
        if discovered_devices.is_empty() {
            return Err(SelectionError::NoDevicesFound);
        }

        let first_device = discovered_devices.into_iter().next().unwrap();
        tracing::info!("Using first available device: '{}'", first_device.name);
        
        DeviceConverter::discovery_to_stream(first_device)
            .map_err(SelectionError::from)
    }

    /// Format a list of devices for display in error messages.
    ///
    /// Creates a human-readable list showing device names, rooms, and IP addresses.
    ///
    /// # Arguments
    ///
    /// * `devices` - List of devices to format
    ///
    /// # Returns
    ///
    /// A formatted string listing all devices with their details.
    fn format_device_list(devices: &[DiscoveryDevice]) -> String {
        if devices.is_empty() {
            return "  (none)".to_string();
        }

        devices
            .iter()
            .map(|device| {
                format!(
                    "  - {} ({}) at {} [{}]",
                    device.name, device.room_name, device.ip_address, device.model_name
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// List all discovered devices in a user-friendly format.
    ///
    /// This function provides a formatted display of all discovered devices,
    /// useful for showing users what devices are available on the network.
    ///
    /// # Arguments
    ///
    /// * `devices` - List of devices to display
    ///
    /// # Returns
    ///
    /// A formatted string with device information.
    pub fn list_devices(devices: &[DiscoveryDevice]) -> String {
        if devices.is_empty() {
            return "No Sonos devices found on the network.".to_string();
        }

        let mut output = format!("Found {} Sonos device(s):\n", devices.len());
        output.push_str(&Self::format_device_list(devices));
        output
    }
}

/// Helper struct for displaying device information in a structured way.
#[derive(Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub room: String,
    pub ip_address: String,
    pub model: String,
    pub id: String,
}

impl From<&DiscoveryDevice> for DeviceInfo {
    fn from(device: &DiscoveryDevice) -> Self {
        Self {
            name: device.name.clone(),
            room: device.room_name.clone(),
            ip_address: device.ip_address.clone(),
            model: device.model_name.clone(),
            id: device.id.clone(),
        }
    }
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) - {} [{}] - ID: {}",
            self.name, self.room, self.ip_address, self.model, self.id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_devices() -> Vec<DiscoveryDevice> {
        vec![
            DiscoveryDevice {
                id: "uuid:RINCON_000E58A0123456".to_string(),
                name: "Sonos Roam 2".to_string(),
                room_name: "Living Room".to_string(),
                ip_address: "192.168.1.100".to_string(),
                port: 1400,
                model_name: "Sonos Roam".to_string(),
            },
            DiscoveryDevice {
                id: "uuid:RINCON_ABCDEF123456".to_string(),
                name: "Kitchen Speaker".to_string(),
                room_name: "Kitchen".to_string(),
                ip_address: "192.168.1.101".to_string(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            },
            DiscoveryDevice {
                id: "uuid:RINCON_FEDCBA654321".to_string(),
                name: "Bedroom Sonos".to_string(),
                room_name: "Bedroom".to_string(),
                ip_address: "192.168.1.102".to_string(),
                port: 1400,
                model_name: "Sonos Play:1".to_string(),
            },
        ]
    }

    #[test]
    fn test_find_device_by_name_success() {
        let devices = create_test_devices();
        let result = DeviceSelector::find_device_by_name("Sonos Roam 2", devices);
        
        assert!(result.is_ok());
        let speaker = result.unwrap();
        assert_eq!(speaker.name, "Sonos Roam 2");
        assert_eq!(speaker.room, "Living Room");
    }

    #[test]
    fn test_find_device_by_name_case_insensitive() {
        let devices = create_test_devices();
        let result = DeviceSelector::find_device_by_name("sonos roam 2", devices);
        
        assert!(result.is_ok());
        let speaker = result.unwrap();
        assert_eq!(speaker.name, "Sonos Roam 2");
    }

    #[test]
    fn test_find_device_not_found() {
        let devices = create_test_devices();
        let result = DeviceSelector::find_device_by_name("Nonexistent Device", devices);
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SelectionError::DeviceNotFound { target_name, available_devices } => {
                assert_eq!(target_name, "Nonexistent Device");
                assert!(available_devices.contains("Sonos Roam 2"));
                assert!(available_devices.contains("Kitchen Speaker"));
                assert!(available_devices.contains("Bedroom Sonos"));
            }
            _ => panic!("Expected DeviceNotFound error"),
        }
    }

    #[test]
    fn test_no_devices_found() {
        let devices = vec![];
        let result = DeviceSelector::find_device_by_name("Any Device", devices);
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SelectionError::NoDevicesFound));
    }

    #[test]
    fn test_find_device_with_fallback_exact_match() {
        let devices = create_test_devices();
        let result = DeviceSelector::find_device_with_fallback("Kitchen Speaker", devices);
        
        assert!(result.is_ok());
        let speaker = result.unwrap();
        assert_eq!(speaker.name, "Kitchen Speaker");
    }

    #[test]
    fn test_find_device_with_fallback_partial_match() {
        let devices = create_test_devices();
        let result = DeviceSelector::find_device_with_fallback("Roam", devices);
        
        assert!(result.is_ok());
        let speaker = result.unwrap();
        assert_eq!(speaker.name, "Sonos Roam 2");
    }

    #[test]
    fn test_find_device_with_fallback_multiple_partial_matches() {
        let devices = create_test_devices();
        let result = DeviceSelector::find_device_with_fallback("Sonos", devices);
        
        assert!(result.is_err());
        match result.unwrap_err() {
            SelectionError::DeviceNotFound { available_devices, .. } => {
                assert!(available_devices.contains("Did you mean one of these?"));
            }
            _ => panic!("Expected DeviceNotFound error with suggestions"),
        }
    }

    #[test]
    fn test_get_first_available() {
        let devices = create_test_devices();
        let result = DeviceSelector::get_first_available(devices);
        
        assert!(result.is_ok());
        let speaker = result.unwrap();
        assert_eq!(speaker.name, "Sonos Roam 2"); // First device in the list
    }

    #[test]
    fn test_get_first_available_empty_list() {
        let devices = vec![];
        let result = DeviceSelector::get_first_available(devices);
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SelectionError::NoDevicesFound));
    }

    #[test]
    fn test_list_devices() {
        let devices = create_test_devices();
        let output = DeviceSelector::list_devices(&devices);
        
        assert!(output.contains("Found 3 Sonos device(s)"));
        assert!(output.contains("Sonos Roam 2"));
        assert!(output.contains("Kitchen Speaker"));
        assert!(output.contains("Bedroom Sonos"));
        assert!(output.contains("192.168.1.100"));
    }

    #[test]
    fn test_list_devices_empty() {
        let devices = vec![];
        let output = DeviceSelector::list_devices(&devices);
        
        assert_eq!(output, "No Sonos devices found on the network.");
    }

    #[test]
    fn test_device_info_display() {
        let device = &create_test_devices()[0];
        let info = DeviceInfo::from(device);
        let display = format!("{}", info);
        
        assert!(display.contains("Sonos Roam 2"));
        assert!(display.contains("Living Room"));
        assert!(display.contains("192.168.1.100"));
        assert!(display.contains("Sonos Roam"));
        assert!(display.contains("uuid:RINCON_000E58A0123456"));
    }
}