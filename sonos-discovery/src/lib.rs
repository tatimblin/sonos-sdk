//! Sonos device discovery library
//!
//! This crate provides a simple API for discovering Sonos devices on a local network
//! using SSDP (Simple Service Discovery Protocol) and UPnP device descriptions.
//!
//! # Quick Start
//!
//! ```no_run
//! use sonos_discovery::get;
//!
//! // Discover all Sonos devices on the network
//! let devices = get();
//! for device in devices {
//!     println!("Found {} at {}", device.name, device.ip_address);
//! }
//! ```
//!
//! # Iterator-based Discovery
//!
//! For more control, use the iterator API:
//!
//! ```no_run
//! use sonos_discovery::{get_iter, DeviceEvent};
//!
//! for event in get_iter() {
//!     match event {
//!         DeviceEvent::Found(device) => {
//!             println!("Found: {}", device.name);
//!             // Can break early if needed
//!         }
//!     }
//! }
//! ```

mod error;
mod ssdp;
pub mod device;
mod discovery;

pub use error::{DiscoveryError, Result};
pub use discovery::DiscoveryIterator;

/// Information about a discovered Sonos device.
///
/// Contains all relevant metadata needed to identify and connect to a Sonos speaker.
#[derive(Debug, Clone)]
pub struct Device {
    /// Unique device identifier (UDN), e.g., "uuid:RINCON_000E58A0123456"
    pub id: String,
    /// Friendly name of the device
    pub name: String,
    /// Room name where the device is located
    pub room_name: String,
    /// IP address of the device
    pub ip_address: String,
    /// Port number (typically 1400)
    pub port: u16,
    /// Model name (e.g., "Sonos One", "Sonos Play:1")
    pub model_name: String,
}

/// Events emitted during device discovery.
///
/// Currently only supports device found events. Future versions may add additional
/// event types (e.g., device lost, device updated).
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    /// A Sonos device was found on the network
    Found(Device),
}

use std::time::Duration;

/// Discover all Sonos devices on the local network with a default 3-second timeout.
///
/// This is a convenience function that collects all discovered devices into a Vec.
/// For more control over the discovery process, use `get_iter()` instead.
///
/// # Examples
///
/// ```no_run
/// use sonos_discovery::get;
///
/// let devices = get();
/// for device in devices {
///     println!("Found: {} at {}", device.name, device.ip_address);
/// }
/// ```
pub fn get() -> Vec<Device> {
    get_with_timeout(Duration::from_secs(3))
}

/// Discover all Sonos devices on the local network with a custom timeout.
///
/// This function collects all discovered devices into a Vec. The timeout parameter
/// controls how long to wait for SSDP responses and HTTP requests.
///
/// # Arguments
///
/// * `timeout` - Maximum duration to wait for network operations
///
/// # Examples
///
/// ```no_run
/// use sonos_discovery::get_with_timeout;
/// use std::time::Duration;
///
/// let devices = get_with_timeout(Duration::from_secs(5));
/// for device in devices {
///     println!("Found: {} at {}", device.name, device.ip_address);
/// }
/// ```
pub fn get_with_timeout(timeout: Duration) -> Vec<Device> {
    get_iter_with_timeout(timeout)
        .filter_map(|event| match event {
            DeviceEvent::Found(device) => Some(device),
        })
        .collect()
}

/// Get an iterator for discovering Sonos devices with a default 3-second timeout.
///
/// This function returns an iterator that yields `DeviceEvent::Found` for each
/// discovered device. This allows for streaming processing and early termination.
///
/// # Examples
///
/// ```no_run
/// use sonos_discovery::{get_iter, DeviceEvent};
///
/// for event in get_iter() {
///     match event {
///         DeviceEvent::Found(device) => {
///             println!("Found: {} at {}", device.name, device.ip_address);
///             // Can break early if needed
///             break;
///         }
///     }
/// }
/// ```
pub fn get_iter() -> DiscoveryIterator {
    get_iter_with_timeout(Duration::from_secs(3))
}

/// Get an iterator for discovering Sonos devices with a custom timeout.
///
/// This function returns an iterator that yields `DeviceEvent::Found` for each
/// discovered device. The timeout parameter controls how long to wait for
/// SSDP responses and HTTP requests.
///
/// # Arguments
///
/// * `timeout` - Maximum duration to wait for network operations
///
/// # Examples
///
/// ```no_run
/// use sonos_discovery::{get_iter_with_timeout, DeviceEvent};
/// use std::time::Duration;
///
/// for event in get_iter_with_timeout(Duration::from_secs(5)) {
///     match event {
///         DeviceEvent::Found(device) => {
///             println!("Found: {} at {}", device.name, device.ip_address);
///         }
///     }
/// }
/// ```
pub fn get_iter_with_timeout(timeout: Duration) -> DiscoveryIterator {
    DiscoveryIterator::new(timeout).unwrap_or_else(|_| {
        // If we fail to create the iterator, return an empty one
        // This is better than panicking
        DiscoveryIterator::empty()
    })
}
