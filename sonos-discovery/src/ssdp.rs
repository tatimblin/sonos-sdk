//! SSDP (Simple Service Discovery Protocol) implementation for device discovery
//!
//! This module provides internal SSDP client functionality for discovering UPnP devices
//! on the local network. It is not part of the public API.

use std::net::UdpSocket;
use std::time::Duration;
use crate::error::{DiscoveryError, Result};

/// SSDP response containing device information
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SsdpResponse {
    pub location: String,
    pub urn: String,
    pub usn: String,
    pub server: Option<String>,
}

/// SSDP client for device discovery
pub(crate) struct SsdpClient {
    socket: UdpSocket,
}

impl SsdpClient {
    /// Create a new SSDP client with the specified timeout
    pub fn new(timeout: Duration) -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| DiscoveryError::NetworkError(format!("Failed to bind UDP socket: {}", e)))?;
        
        socket.set_read_timeout(Some(timeout))
            .map_err(|e| DiscoveryError::NetworkError(format!("Failed to set read timeout: {}", e)))?;
        
        socket.set_multicast_loop_v4(true)
            .map_err(|e| DiscoveryError::NetworkError(format!("Failed to set multicast loop: {}", e)))?;
        
        Ok(Self { socket })
    }

    /// Send an M-SEARCH request and return an iterator of responses
    pub fn search(&self, search_target: &str) -> Result<SsdpResponseIterator> {
        let request = format!(
            "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 2\r\n\
             ST: {}\r\n\
             USER-AGENT: sonos-rs/1.0 UPnP/1.0\r\n\
             \r\n",
            search_target
        );

        self.socket.send_to(request.as_bytes(), "239.255.255.250:1900")
            .map_err(|e| DiscoveryError::NetworkError(format!("Failed to send M-SEARCH: {}", e)))?;
        
        Ok(SsdpResponseIterator::new(&self.socket))
    }
}

/// Iterator for SSDP responses
pub(crate) struct SsdpResponseIterator<'a> {
    socket: &'a UdpSocket,
    buffer: [u8; 2048],
    finished: bool,
}

impl<'a> SsdpResponseIterator<'a> {
    fn new(socket: &'a UdpSocket) -> Self {
        Self {
            socket,
            buffer: [0; 2048],
            finished: false,
        }
    }
}

impl<'a> Iterator for SsdpResponseIterator<'a> {
    type Item = Result<SsdpResponse>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        match self.socket.recv_from(&mut self.buffer) {
            Ok((size, _)) => {
                match std::str::from_utf8(&self.buffer[..size]) {
                    Ok(response_text) => {
                        match parse_ssdp_response(response_text) {
                            Some(response) => Some(Ok(response)),
                            None => {
                                // Invalid response, try next one
                                self.next()
                            }
                        }
                    }
                    Err(_) => {
                        // Invalid UTF-8, try next one
                        self.next()
                    }
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    self.finished = true;
                    None
                } else {
                    Some(Err(DiscoveryError::NetworkError(format!("Socket error: {}", e))))
                }
            }
        }
    }
}

/// Parse an SSDP response from HTTP text
fn parse_ssdp_response(response: &str) -> Option<SsdpResponse> {
    let mut location = None;
    let mut urn = None;
    let mut usn = None;
    let mut server = None;

    for line in response.lines() {
        let line = line.trim();
        
        if let Some(value) = extract_header_value(line, "LOCATION:") {
            location = Some(value);
        } else if let Some(value) = extract_header_value(line, "ST:") {
            urn = Some(value);
        } else if let Some(value) = extract_header_value(line, "USN:") {
            usn = Some(value);
        } else if let Some(value) = extract_header_value(line, "SERVER:") {
            server = Some(value);
        }
    }

    match (location, urn, usn) {
        (Some(location), Some(urn), Some(usn)) => {
            Some(SsdpResponse {
                location,
                urn,
                usn,
                server,
            })
        }
        _ => None,
    }
}

/// Extract header value from a line like "HEADER: value"
fn extract_header_value(line: &str, header: &str) -> Option<String> {
    if line.len() > header.len() && line[..header.len()].eq_ignore_ascii_case(header) {
        Some(line[header.len()..].trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ssdp_response_valid() {
        let response = "HTTP/1.1 200 OK\r\n\
            LOCATION: http://192.168.1.100:1400/xml/device_description.xml\r\n\
            ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            USN: uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            SERVER: Linux/3.14.0 UPnP/1.0 Sonos/70.3-35220\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response).unwrap();
        
        assert_eq!(parsed.location, "http://192.168.1.100:1400/xml/device_description.xml");
        assert_eq!(parsed.urn, "urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(parsed.usn, "uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(parsed.server, Some("Linux/3.14.0 UPnP/1.0 Sonos/70.3-35220".to_string()));
    }

    #[test]
    fn test_parse_ssdp_response_without_server() {
        let response = "HTTP/1.1 200 OK\r\n\
            LOCATION: http://192.168.1.101:1400/xml/device_description.xml\r\n\
            ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            USN: uuid:RINCON_000E58A0654321::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response).unwrap();
        
        assert_eq!(parsed.location, "http://192.168.1.101:1400/xml/device_description.xml");
        assert_eq!(parsed.urn, "urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(parsed.usn, "uuid:RINCON_000E58A0654321::urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(parsed.server, None);
    }

    #[test]
    fn test_parse_ssdp_response_case_insensitive() {
        let response = "HTTP/1.1 200 OK\r\n\
            location: http://192.168.1.102:1400/xml/device_description.xml\r\n\
            st: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            usn: uuid:RINCON_000E58A0ABCDEF::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            server: Linux/3.14.0 UPnP/1.0 Sonos/70.3-35220\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response).unwrap();
        
        assert_eq!(parsed.location, "http://192.168.1.102:1400/xml/device_description.xml");
        assert_eq!(parsed.urn, "urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(parsed.usn, "uuid:RINCON_000E58A0ABCDEF::urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(parsed.server, Some("Linux/3.14.0 UPnP/1.0 Sonos/70.3-35220".to_string()));
    }

    #[test]
    fn test_parse_ssdp_response_missing_location() {
        let response = "HTTP/1.1 200 OK\r\n\
            ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            USN: uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_ssdp_response_missing_st() {
        let response = "HTTP/1.1 200 OK\r\n\
            LOCATION: http://192.168.1.100:1400/xml/device_description.xml\r\n\
            USN: uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_ssdp_response_missing_usn() {
        let response = "HTTP/1.1 200 OK\r\n\
            LOCATION: http://192.168.1.100:1400/xml/device_description.xml\r\n\
            ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_ssdp_response_empty() {
        let response = "";
        let parsed = parse_ssdp_response(response);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_parse_ssdp_response_malformed() {
        let response = "This is not a valid SSDP response\r\n\
            Some random text\r\n";

        let parsed = parse_ssdp_response(response);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_extract_header_value_basic() {
        assert_eq!(
            extract_header_value("LOCATION: http://example.com", "LOCATION:"),
            Some("http://example.com".to_string())
        );
    }

    #[test]
    fn test_extract_header_value_case_insensitive() {
        assert_eq!(
            extract_header_value("location: http://example.com", "LOCATION:"),
            Some("http://example.com".to_string())
        );
        assert_eq!(
            extract_header_value("Location: http://example.com", "LOCATION:"),
            Some("http://example.com".to_string())
        );
        assert_eq!(
            extract_header_value("LoCaTiOn: http://example.com", "LOCATION:"),
            Some("http://example.com".to_string())
        );
    }

    #[test]
    fn test_extract_header_value_with_whitespace() {
        assert_eq!(
            extract_header_value("LOCATION:    http://example.com   ", "LOCATION:"),
            Some("http://example.com".to_string())
        );
        assert_eq!(
            extract_header_value("LOCATION:\thttp://example.com", "LOCATION:"),
            Some("http://example.com".to_string())
        );
    }

    #[test]
    fn test_extract_header_value_empty_value() {
        // When there's whitespace after the colon, it returns empty string
        assert_eq!(
            extract_header_value("LOCATION: ", "LOCATION:"),
            Some("".to_string())
        );
        // When there's no character after the header, it returns None (line too short)
        assert_eq!(
            extract_header_value("LOCATION:", "LOCATION:"),
            None
        );
    }

    #[test]
    fn test_extract_header_value_no_match() {
        assert_eq!(
            extract_header_value("OTHER: value", "LOCATION:"),
            None
        );
        assert_eq!(
            extract_header_value("LOCATIONS: value", "LOCATION:"),
            None
        );
        assert_eq!(
            extract_header_value("LOC: value", "LOCATION:"),
            None
        );
    }

    #[test]
    fn test_extract_header_value_complex_value() {
        assert_eq!(
            extract_header_value("USN: uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1", "USN:"),
            Some("uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1".to_string())
        );
    }
}
