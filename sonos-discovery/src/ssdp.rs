//! SSDP (Simple Service Discovery Protocol) implementation for device discovery
//!
//! This module provides internal SSDP client functionality for discovering UPnP devices
//! on the local network. It is not part of the public API.

use crate::error::{DiscoveryError, Result};
use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

/// SSDP multicast group and port for UPnP discovery
const SSDP_ADDR: &str = "239.255.255.250:1900";

/// SSDP response containing device information
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SsdpResponse {
    pub location: String,
    pub urn: String,
    pub usn: String,
    pub server: Option<String>,
}

/// SSDP client for device discovery.
///
/// Probes every usable IPv4 interface rather than relying on the OS routing
/// table to pick a multicast egress interface. A socket bound to `0.0.0.0`
/// leaves that choice to the routing table, and on hosts with virtual adapters
/// (Hyper-V/WSL `vEthernet`, Docker bridges, VPN tunnels) the winning route is
/// frequently an interface with no path to the speakers. The M-SEARCH then
/// leaves via that interface and no Sonos device ever sees it, so discovery
/// silently returns nothing on an otherwise healthy network.
pub(crate) struct SsdpClient {
    /// Candidate interface addresses to send M-SEARCH from
    interfaces: Vec<Ipv4Addr>,
    timeout: Duration,
}

impl SsdpClient {
    /// Create a new SSDP client with the specified timeout
    pub fn new(timeout: Duration) -> Result<Self> {
        let interfaces = usable_interfaces()?;

        if interfaces.is_empty() {
            return Err(DiscoveryError::NetworkError(
                "no usable IPv4 network interfaces found".to_string(),
            ));
        }

        tracing::debug!(
            "SSDP will probe {} interface(s): {:?}",
            interfaces.len(),
            interfaces
        );

        Ok(Self {
            interfaces,
            timeout,
        })
    }

    /// Send an M-SEARCH request from every usable interface and collect responses.
    ///
    /// Interfaces are probed concurrently so that overall wall-clock stays at
    /// roughly `timeout` regardless of interface count; probing serially would
    /// multiply the timeout by the number of interfaces.
    ///
    /// A failure on one interface (a virtual adapter that cannot route
    /// multicast, for example) is not fatal: that interface is skipped and the
    /// remaining ones still report. An error is only returned when every
    /// interface fails, which indicates a genuine local network problem.
    pub fn search(&self, search_target: &str) -> Result<Vec<SsdpResponse>> {
        let request = format!(
            "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 2\r\n\
             ST: {search_target}\r\n\
             USER-AGENT: sonos-rs/1.0 UPnP/1.0\r\n\
             \r\n"
        );

        let handles: Vec<_> = self
            .interfaces
            .iter()
            .map(|&ip| {
                let request = request.clone();
                let timeout = self.timeout;
                std::thread::spawn(move || search_on_interface(ip, &request, timeout))
            })
            .collect();

        let mut responses = Vec::new();
        let mut errors = Vec::new();

        for handle in handles {
            // A panic in a probe thread is treated like any other interface
            // failure so one bad interface cannot abort the whole discovery.
            match handle.join() {
                Ok(Ok(found)) => responses.extend(found),
                Ok(Err(e)) => errors.push(e),
                Err(_) => errors.push("probe thread panicked".to_string()),
            }
        }

        if responses.is_empty() && errors.len() == self.interfaces.len() {
            return Err(DiscoveryError::NetworkError(format!(
                "M-SEARCH failed on all {} interface(s): {}",
                self.interfaces.len(),
                errors.join("; ")
            )));
        }

        Ok(responses)
    }
}

/// Send an M-SEARCH from a single interface address and read responses until timeout.
fn search_on_interface(
    ip: Ipv4Addr,
    request: &str,
    timeout: Duration,
) -> std::result::Result<Vec<SsdpResponse>, String> {
    let socket =
        UdpSocket::bind((ip, 0)).map_err(|e| format!("{ip}: failed to bind UDP socket: {e}"))?;

    socket
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("{ip}: failed to set read timeout: {e}"))?;

    // Best-effort: some interfaces reject this but can still send M-SEARCH.
    let _ = socket.set_multicast_loop_v4(true);

    // Binding to an interface address succeeds even when that interface has no
    // multicast route, so the real failure usually surfaces here on send.
    socket
        .send_to(request.as_bytes(), SSDP_ADDR)
        .map_err(|e| format!("{ip}: failed to send M-SEARCH: {e}"))?;

    let mut buffer = [0u8; 2048];
    let mut responses = Vec::new();

    loop {
        match socket.recv_from(&mut buffer) {
            Ok((size, _)) => {
                if let Ok(text) = std::str::from_utf8(&buffer[..size]) {
                    if let Some(response) = parse_ssdp_response(text) {
                        responses.push(response);
                    }
                }
                // Malformed responses and invalid UTF-8 are skipped; unrelated
                // UPnP devices on the network legitimately send both.
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Read timeout reached: normal end of the response window.
                break;
            }
            Err(_) => break,
        }
    }

    Ok(responses)
}

/// Enumerate IPv4 interface addresses worth sending an M-SEARCH from.
///
/// Loopback is excluded because it cannot reach speakers, and link-local
/// (169.254.0.0/16) is excluded because it indicates a failed DHCP lease.
fn usable_interfaces() -> Result<Vec<Ipv4Addr>> {
    let addrs = if_addrs::get_if_addrs().map_err(|e| {
        DiscoveryError::NetworkError(format!("Failed to enumerate network interfaces: {e}"))
    })?;

    let mut interfaces: Vec<Ipv4Addr> = addrs
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            std::net::IpAddr::V4(v4) if is_usable_ipv4(&v4) => Some(v4),
            _ => None,
        })
        .collect();

    // Overlapping interfaces can surface the same speaker twice; dedup here so
    // fewer duplicate responses reach the caller. Location-level dedup in
    // DiscoveryIterator still handles distinct interfaces on the same subnet.
    interfaces.sort();
    interfaces.dedup();

    Ok(interfaces)
}

/// Whether an IPv4 address belongs to an interface that could reach a speaker.
fn is_usable_ipv4(addr: &Ipv4Addr) -> bool {
    !addr.is_loopback() && !addr.is_link_local() && !addr.is_unspecified()
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
        (Some(location), Some(urn), Some(usn)) => Some(SsdpResponse {
            location,
            urn,
            usn,
            server,
        }),
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
    fn test_is_usable_ipv4_excludes_unroutable() {
        // Loopback and link-local cannot reach a speaker; link-local in
        // particular signals a failed DHCP lease.
        assert!(!is_usable_ipv4(&Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_usable_ipv4(&Ipv4Addr::new(169, 254, 1, 5)));
        assert!(!is_usable_ipv4(&Ipv4Addr::UNSPECIFIED));
    }

    #[test]
    fn test_is_usable_ipv4_accepts_lan_and_virtual_addresses() {
        // Real LAN addresses.
        assert!(is_usable_ipv4(&Ipv4Addr::new(192, 168, 1, 50)));
        assert!(is_usable_ipv4(&Ipv4Addr::new(10, 83, 0, 10)));
        // Virtual adapters (Hyper-V/WSL, Docker) are still probed: we cannot
        // tell from the address alone whether speakers live behind them, and
        // a failed send simply skips the interface.
        assert!(is_usable_ipv4(&Ipv4Addr::new(172, 28, 80, 1)));
    }

    #[test]
    fn test_usable_interfaces_are_deduped_and_routable() {
        let interfaces = usable_interfaces().expect("interface enumeration should succeed");

        let mut deduped = interfaces.clone();
        deduped.dedup();
        assert_eq!(interfaces, deduped, "interfaces must be deduplicated");

        assert!(interfaces.iter().all(is_usable_ipv4));
    }

    #[test]
    fn test_search_on_interface_skips_unusable_interface() {
        // Loopback has no route to the SSDP group, so this stands in for the
        // virtual adapter case: it must return an error for the caller to skip
        // rather than hanging or panicking.
        let result = search_on_interface(
            Ipv4Addr::new(127, 0, 0, 1),
            "M-SEARCH * HTTP/1.1\r\n\r\n",
            Duration::from_millis(50),
        );

        match result {
            Err(msg) => assert!(msg.contains("127.0.0.1"), "error should name the interface"),
            // If a platform does permit the send, no responses are expected.
            Ok(responses) => assert!(responses.is_empty()),
        }
    }

    #[test]
    fn test_parse_ssdp_response_valid() {
        let response = "HTTP/1.1 200 OK\r\n\
            LOCATION: http://192.168.1.100:1400/xml/device_description.xml\r\n\
            ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            USN: uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            SERVER: Linux/3.14.0 UPnP/1.0 Sonos/70.3-35220\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response).unwrap();

        assert_eq!(
            parsed.location,
            "http://192.168.1.100:1400/xml/device_description.xml"
        );
        assert_eq!(parsed.urn, "urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(
            parsed.usn,
            "uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1"
        );
        assert_eq!(
            parsed.server,
            Some("Linux/3.14.0 UPnP/1.0 Sonos/70.3-35220".to_string())
        );
    }

    #[test]
    fn test_parse_ssdp_response_without_server() {
        let response = "HTTP/1.1 200 OK\r\n\
            LOCATION: http://192.168.1.101:1400/xml/device_description.xml\r\n\
            ST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            USN: uuid:RINCON_000E58A0654321::urn:schemas-upnp-org:device:ZonePlayer:1\r\n\
            \r\n";

        let parsed = parse_ssdp_response(response).unwrap();

        assert_eq!(
            parsed.location,
            "http://192.168.1.101:1400/xml/device_description.xml"
        );
        assert_eq!(parsed.urn, "urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(
            parsed.usn,
            "uuid:RINCON_000E58A0654321::urn:schemas-upnp-org:device:ZonePlayer:1"
        );
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

        assert_eq!(
            parsed.location,
            "http://192.168.1.102:1400/xml/device_description.xml"
        );
        assert_eq!(parsed.urn, "urn:schemas-upnp-org:device:ZonePlayer:1");
        assert_eq!(
            parsed.usn,
            "uuid:RINCON_000E58A0ABCDEF::urn:schemas-upnp-org:device:ZonePlayer:1"
        );
        assert_eq!(
            parsed.server,
            Some("Linux/3.14.0 UPnP/1.0 Sonos/70.3-35220".to_string())
        );
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
        assert_eq!(extract_header_value("LOCATION:", "LOCATION:"), None);
    }

    #[test]
    fn test_extract_header_value_no_match() {
        assert_eq!(extract_header_value("OTHER: value", "LOCATION:"), None);
        assert_eq!(extract_header_value("LOCATIONS: value", "LOCATION:"), None);
        assert_eq!(extract_header_value("LOC: value", "LOCATION:"), None);
    }

    #[test]
    fn test_extract_header_value_complex_value() {
        assert_eq!(
            extract_header_value(
                "USN: uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1",
                "USN:"
            ),
            Some(
                "uuid:RINCON_000E58A0123456::urn:schemas-upnp-org:device:ZonePlayer:1".to_string()
            )
        );
    }
}
