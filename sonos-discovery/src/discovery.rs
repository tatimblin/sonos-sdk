//! Core discovery logic and iterator implementation.
//!
//! This module implements the discovery algorithm that:
//! 1. Sends SSDP M-SEARCH requests for Sonos ZonePlayer devices
//! 2. Receives and filters SSDP responses
//! 3. Fetches device descriptions via HTTP
//! 4. Parses and validates device information
//! 5. Yields discovered devices as events

use crate::device::{extract_ip_from_url, DeviceDescription};
use crate::error::Result;
use crate::ssdp::{SsdpClient, SsdpResponse};
use crate::DeviceEvent;
use std::collections::HashSet;
use std::time::Duration;
use ureq::Agent;

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
    http_client: Agent,
    finished: bool,
}

impl DiscoveryIterator {
    /// Create a new discovery iterator with the specified timeout
    pub fn new(timeout: Duration) -> Result<Self> {
        let ssdp_client = SsdpClient::new(timeout)?;
        let http_client = Self::http_agent(timeout);

        Ok(Self {
            ssdp_client: Some(ssdp_client),
            ssdp_buffer: Vec::new(),
            buffer_index: 0,
            seen_locations: HashSet::new(),
            http_client,
            finished: false,
        })
    }

    /// Build the HTTP agent used to fetch device descriptions.
    ///
    /// `timeout_global` bounds the whole request — connect, send, and body read —
    /// which matches the single overall deadline the previous client applied.
    ///
    /// `http_status_as_error` is turned off so `fetch_device_description` owns the
    /// status check and can name the offending location in the error, rather than
    /// having a bare `StatusCode` error interleave with genuine transport failures.
    fn http_agent(timeout: Duration) -> Agent {
        Agent::new_with_config(
            Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build(),
        )
    }

    /// Create an empty iterator that yields no results
    /// Used as a fallback when initialization fails
    pub(crate) fn empty() -> Self {
        // `finished` is true and the buffer stays empty, so this agent never
        // issues a request; its config is irrelevant.
        let http_client = Agent::new_with_defaults();
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
        let mut response = self.http_client.get(location).call().map_err(|e| {
            crate::error::DiscoveryError::NetworkError(format!(
                "Failed to fetch device description: {e}"
            ))
        })?;

        // Check the status before touching the body. Without this a 404 or 500
        // HTML error page reaches the XML parser and surfaces as a confusing
        // parse error instead of the HTTP failure it actually is.
        let status = response.status();
        if !status.is_success() {
            return Err(crate::error::DiscoveryError::NetworkError(format!(
                "Device description request to {location} returned HTTP {status}"
            )));
        }

        let xml = response.body_mut().read_to_string().map_err(|e| {
            crate::error::DiscoveryError::NetworkError(format!("Failed to read response body: {e}"))
        })?;

        DeviceDescription::from_xml(&xml)
    }

    /// Fill the buffer with SSDP responses
    fn fill_buffer(&mut self) {
        if let Some(client) = self.ssdp_client.take() {
            match client.search("urn:schemas-upnp-org:device:ZonePlayer:1") {
                Ok(responses) => {
                    self.ssdp_buffer = responses;
                }
                Err(e) => {
                    // Every interface failed to send. Surface the reason: this
                    // is otherwise indistinguishable from "no speakers here".
                    tracing::warn!("SSDP search failed: {}", e);
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
        // Drop the SSDP client if the search never ran, so an unused iterator
        // releases it promptly. Probe sockets are owned by their own threads
        // and closed when each thread finishes, so there is nothing else to
        // release here.
        if let Some(client) = self.ssdp_client.take() {
            drop(client);
        }
        // HTTP client is automatically cleaned up when dropped
        // No additional cleanup needed for other fields
    }
}
