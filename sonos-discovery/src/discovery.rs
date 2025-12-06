//! Core discovery logic and iterator implementation.
//!
//! This module implements the discovery algorithm that:
//! 1. Sends SSDP M-SEARCH requests for Sonos ZonePlayer devices
//! 2. Receives and filters SSDP responses
//! 3. Fetches device descriptions via HTTP
//! 4. Parses and validates device information
//! 5. Yields discovered devices as events

use std::collections::HashSet;
use std::time::Duration;
use crate::error::Result;
use crate::ssdp::{SsdpClient, SsdpResponse};
use crate::device::{DeviceDescription, extract_ip_from_url};
use crate::DeviceEvent;

/// Iterator that discovers Sonos devices on the local network.
///
/// This iterator performs network discovery using SSDP and yields `DeviceEvent::Found`
/// for each discovered Sonos device. The iterator automatically handles deduplication,
/// filtering of non-Sonos devices, and resource cleanup.
///
/// # Examples
///
/// ```no_run
/// use sonos_discovery::{get_iter, DeviceEvent};
///
/// for event in get_iter() {
///     match event {
///         DeviceEvent::Found(device) => {
///             println!("Found: {}", device.name);
///         }
///     }
/// }
/// ```
pub struct DiscoveryIterator {
    ssdp_client: Option<SsdpClient>,
    ssdp_buffer: Vec<SsdpResponse>,
    buffer_index: usize,
    seen_locations: HashSet<String>,
    http_client: reqwest::blocking::Client,
    finished: bool,
}

impl DiscoveryIterator {
    /// Create a new discovery iterator with the specified timeout
    pub fn new(timeout: Duration) -> Result<Self> {
        let ssdp_client = SsdpClient::new(timeout)?;
        let http_client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| crate::error::DiscoveryError::NetworkError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            ssdp_client: Some(ssdp_client),
            ssdp_buffer: Vec::new(),
            buffer_index: 0,
            seen_locations: HashSet::new(),
            http_client,
            finished: false,
        })
    }

    /// Create an empty iterator that yields no results
    /// Used as a fallback when initialization fails
    pub(crate) fn empty() -> Self {
        let http_client = reqwest::blocking::Client::new();
        Self {
            ssdp_client: None,
            ssdp_buffer: Vec::new(),
            buffer_index: 0,
            seen_locations: HashSet::new(),
            http_client,
            finished: true,
        }
    }

    /// Check if an SSDP response is likely from a Sonos device (early filtering)
    fn is_likely_sonos(response: &SsdpResponse) -> bool {
        // Check URN for ZonePlayer
        if response.urn.contains("ZonePlayer") {
            return true;
        }

        // Check USN for RINCON (Sonos device identifier)
        if response.usn.contains("RINCON") {
            return true;
        }

        // Check server header for Sonos
        if let Some(ref server) = response.server {
            if server.to_lowercase().contains("sonos") {
                return true;
            }
        }

        false
    }

    /// Fetch and parse device description from a location URL
    fn fetch_device_description(&self, location: &str) -> Result<DeviceDescription> {
        let response = self.http_client
            .get(location)
            .send()
            .map_err(|e| crate::error::DiscoveryError::NetworkError(format!("Failed to fetch device description: {}", e)))?;

        let xml = response
            .text()
            .map_err(|e| crate::error::DiscoveryError::NetworkError(format!("Failed to read response body: {}", e)))?;

        DeviceDescription::from_xml(&xml)
    }

    /// Fill the buffer with SSDP responses
    fn fill_buffer(&mut self) {
        if let Some(client) = self.ssdp_client.take() {
            match client.search("urn:schemas-upnp-org:device:ZonePlayer:1") {
                Ok(iter) => {
                    // Collect all SSDP responses into buffer
                    for result in iter {
                        if let Ok(response) = result {
                            self.ssdp_buffer.push(response);
                        }
                    }
                }
                Err(_) => {
                    // Failed to start search
                }
            }
            self.finished = true;
        }
    }
}

impl Iterator for DiscoveryIterator {
    type Item = DeviceEvent;

    fn next(&mut self) -> Option<Self::Item> {
        // Fill buffer on first call
        if self.ssdp_client.is_some() {
            self.fill_buffer();
        }

        // Process buffered SSDP responses
        loop {
            // Check if we've processed all responses
            if self.buffer_index >= self.ssdp_buffer.len() {
                return None;
            }

            let ssdp_response = &self.ssdp_buffer[self.buffer_index];
            self.buffer_index += 1;

            // Deduplicate by location
            if self.seen_locations.contains(&ssdp_response.location) {
                continue;
            }
            self.seen_locations.insert(ssdp_response.location.clone());

            // Early filtering: skip non-Sonos devices
            if !Self::is_likely_sonos(ssdp_response) {
                continue;
            }

            // Fetch device description
            let device_desc = match self.fetch_device_description(&ssdp_response.location) {
                Ok(desc) => desc,
                Err(_) => continue, // Skip devices that fail to fetch
            };

            // Validate it's a Sonos device
            if !device_desc.is_sonos_device() {
                continue;
            }

            // Extract IP address from location URL
            let ip_address = match extract_ip_from_url(&ssdp_response.location) {
                Some(ip) => ip,
                None => continue, // Skip if we can't extract IP
            };

            // Convert to public Device type
            let device = device_desc.to_device(ip_address);

            // Yield the found device event
            return Some(DeviceEvent::Found(device));
        }
    }
}

impl Drop for DiscoveryIterator {
    fn drop(&mut self) {
        // Explicitly drop the SSDP client to ensure UDP socket cleanup
        // This is important for early iterator termination
        if let Some(client) = self.ssdp_client.take() {
            drop(client);
        }
        // HTTP client is automatically cleaned up when dropped
        // No additional cleanup needed for other fields
    }
}
