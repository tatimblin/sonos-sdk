//! Internal implementation detail of [`sonos-sdk`](https://crates.io/crates/sonos-sdk). Not intended for direct use.
//!
//! Private SOAP client for UPnP device communication
//!
//! This crate provides a minimal SOAP client specifically designed for
//! communicating with UPnP devices like Sonos speakers. It also supports
//! UPnP event subscriptions using SUBSCRIBE/UNSUBSCRIBE methods.

mod error;

pub use error::SoapError;

use std::sync::{Arc, LazyLock};
use std::time::Duration;
use xmltree::Element;

/// Element name carrying UPnP fault details, as spelled in the UPnP Device
/// Architecture specification.
const UPNP_ERROR_ELEMENT: &str = "UPnPError";

/// Historical misspelling of [`UPNP_ERROR_ELEMENT`]. Accepted on read only, so
/// that devices (or older firmware) emitting it still produce a usable code.
const UPNP_ERROR_ELEMENT_LEGACY: &str = "UpnPError";

/// UPnP error code reported when a fault body is present but unparseable.
const UNKNOWN_FAULT_CODE: u16 = 500;

/// Response from a UPnP subscription request
#[derive(Debug, Clone)]
pub struct SubscriptionResponse {
    /// Subscription ID returned by the device
    pub sid: String,
    /// Actual timeout granted by the device (in seconds)
    pub timeout_seconds: u32,
}

/// A minimal SOAP client for UPnP device communication
///
/// Uses Arc internally for efficient sharing of the underlying HTTP client
/// and connection pool across multiple instances.
#[derive(Debug, Clone)]
pub struct SoapClient {
    agent: Arc<ureq::Agent>,
}

/// Global shared SOAP client instance for maximum resource efficiency
static SHARED_SOAP_CLIENT: LazyLock<SoapClient> = LazyLock::new(|| SoapClient {
    agent: Arc::new(
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .build(),
    ),
});

impl SoapClient {
    /// Get the global shared SOAP client instance
    ///
    /// This provides a singleton-like pattern for maximum resource efficiency.
    /// All clients returned by this method share the same underlying HTTP agent
    /// and connection pool, reducing memory usage and improving performance.
    pub fn get() -> &'static Self {
        &SHARED_SOAP_CLIENT
    }

    /// Create a SOAP client with a custom agent (for advanced use cases only)
    ///
    /// Most applications should use `SoapClient::get()` instead for better
    /// resource efficiency. This method is provided for cases where custom
    /// timeout values or other HTTP client configuration is needed.
    pub fn with_agent(agent: Arc<ureq::Agent>) -> Self {
        Self { agent }
    }

    /// Create a new SOAP client with default configuration
    ///
    /// **DEPRECATED**: Use `SoapClient::get()` instead for better resource efficiency.
    /// This method creates a separate HTTP agent instance, which wastes resources
    /// when multiple SOAP clients are used.
    #[deprecated(since = "0.1.0", note = "Use SoapClient::get() for shared resources")]
    pub fn new() -> Self {
        Self::with_agent(Arc::new(
            ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(10))
                .build(),
        ))
    }

    /// Send a SOAP request and return the parsed response element
    pub fn call(
        &self,
        ip: &str,
        endpoint: &str,
        service_uri: &str,
        action: &str,
        payload: &str,
    ) -> Result<Element, SoapError> {
        // Inline SOAP envelope construction - no separate module needed
        let body = format!(
            r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
                <s:Body>
                    <u:{action} xmlns:u="{service_uri}">
                        {payload}
                    </u:{action}>
                </s:Body>
            </s:Envelope>"#
        );

        let url = format!("http://{ip}:1400/{endpoint}");
        let soap_action = format!("\"{service_uri}#{action}\"");

        let response = self
            .agent
            .post(&url)
            .set("Content-Type", "text/xml; charset=\"utf-8\"")
            .set("SOAPACTION", &soap_action)
            .send_string(&body)
            .map_err(|e| map_ureq_error(e, action))?;

        let xml_text = response
            .into_string()
            .map_err(|e| SoapError::Network(e.to_string()))?;

        let xml =
            Element::parse(xml_text.as_bytes()).map_err(|e| SoapError::Parse(e.to_string()))?;

        // Extract response or handle SOAP fault
        self.extract_response(&xml, action)
    }

    /// Subscribe to UPnP events for a specific service endpoint
    ///
    /// # Arguments
    /// * `ip` - Device IP address
    /// * `port` - Device port (typically 1400)
    /// * `event_endpoint` - Event endpoint path (e.g., "MediaRenderer/AVTransport/Event")
    /// * `callback_url` - URL where events should be sent
    /// * `timeout_seconds` - Requested subscription timeout in seconds
    ///
    /// # Returns
    /// A `SubscriptionResponse` containing the SID and actual timeout
    pub fn subscribe(
        &self,
        ip: &str,
        port: u16,
        event_endpoint: &str,
        callback_url: &str,
        timeout_seconds: u32,
    ) -> Result<SubscriptionResponse, SoapError> {
        let url = format!("http://{ip}:{port}/{event_endpoint}");
        let host = format!("{ip}:{port}");

        let response = self
            .agent
            .request("SUBSCRIBE", &url)
            .set("HOST", &host)
            .set("CALLBACK", &format!("<{callback_url}>"))
            .set("NT", "upnp:event")
            .set("TIMEOUT", &format!("Second-{timeout_seconds}"))
            .call()
            .map_err(|e| map_subscription_error("SUBSCRIBE", e))?;

        if !is_success(response.status()) {
            return Err(SoapError::Network(format!(
                "SUBSCRIBE failed: HTTP {}",
                response.status()
            )));
        }

        // Extract SID from response headers
        let sid = response
            .header("SID")
            .ok_or_else(|| {
                SoapError::Parse("Missing SID header in SUBSCRIBE response".to_string())
            })?
            .to_string();

        // Extract timeout from response headers (optional, fallback to requested timeout)
        let actual_timeout_seconds = response
            .header("TIMEOUT")
            .and_then(|s| {
                // Parse "Second-1800" format
                if s.starts_with("Second-") {
                    s.strip_prefix("Second-")?.parse::<u32>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(timeout_seconds);

        Ok(SubscriptionResponse {
            sid,
            timeout_seconds: actual_timeout_seconds,
        })
    }

    /// Renew an existing UPnP subscription
    ///
    /// # Arguments
    /// * `ip` - Device IP address
    /// * `port` - Device port (typically 1400)
    /// * `event_endpoint` - Event endpoint path
    /// * `sid` - Subscription ID to renew
    /// * `timeout_seconds` - Requested renewal timeout in seconds
    ///
    /// # Returns
    /// The actual timeout granted by the device
    pub fn renew_subscription(
        &self,
        ip: &str,
        port: u16,
        event_endpoint: &str,
        sid: &str,
        timeout_seconds: u32,
    ) -> Result<u32, SoapError> {
        let url = format!("http://{ip}:{port}/{event_endpoint}");
        let host = format!("{ip}:{port}");

        let response = self
            .agent
            .request("SUBSCRIBE", &url)
            .set("HOST", &host)
            .set("SID", sid)
            .set("TIMEOUT", &format!("Second-{timeout_seconds}"))
            .call()
            .map_err(|e| map_subscription_error("SUBSCRIBE renewal", e))?;

        if !is_success(response.status()) {
            return Err(SoapError::Network(format!(
                "SUBSCRIBE renewal failed: HTTP {}",
                response.status()
            )));
        }

        // Extract timeout from response headers
        let actual_timeout_seconds = response
            .header("TIMEOUT")
            .and_then(|s| {
                if s.starts_with("Second-") {
                    s.strip_prefix("Second-")?.parse::<u32>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(timeout_seconds);

        Ok(actual_timeout_seconds)
    }

    /// Unsubscribe from UPnP events
    ///
    /// # Arguments
    /// * `ip` - Device IP address
    /// * `port` - Device port (typically 1400)
    /// * `event_endpoint` - Event endpoint path
    /// * `sid` - Subscription ID to cancel
    pub fn unsubscribe(
        &self,
        ip: &str,
        port: u16,
        event_endpoint: &str,
        sid: &str,
    ) -> Result<(), SoapError> {
        let url = format!("http://{ip}:{port}/{event_endpoint}");
        let host = format!("{ip}:{port}");

        let response = self
            .agent
            .request("UNSUBSCRIBE", &url)
            .set("HOST", &host)
            .set("SID", sid)
            .call()
            .map_err(|e| map_subscription_error("UNSUBSCRIBE", e))?;

        if !is_success(response.status()) {
            return Err(SoapError::Network(format!(
                "UNSUBSCRIBE failed: HTTP {}",
                response.status()
            )));
        }

        Ok(())
    }

    fn extract_response(&self, xml: &Element, action: &str) -> Result<Element, SoapError> {
        extract_response(xml, action)
    }
}

impl Default for SoapClient {
    fn default() -> Self {
        Self::get().clone()
    }
}

/// Whether an HTTP status should be treated as success.
///
/// `ureq` already converts any status >= 400 into `Error::Status`, so the only
/// codes reaching a `Result::Ok` are 1xx-3xx. A strict `== 200` check would
/// therefore reject spec-legal successes such as `201 Created`, never actual
/// failures.
fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Map a `ureq` error from a SOAP control request into a [`SoapError`].
///
/// UPnP devices report action failures as **HTTP 500 with a SOAP fault body**.
/// `ureq` surfaces any status >= 400 as `Error::Status(code, Response)`, so the
/// fault body must be read out of the response here; otherwise every device
/// rejection is indistinguishable from an unreachable speaker.
fn map_ureq_error(error: ureq::Error, action: &str) -> SoapError {
    match error {
        ureq::Error::Status(status, response) => match response.into_string() {
            Ok(body) => match Element::parse(body.as_bytes()) {
                // A parseable envelope tells us why the device refused.
                Ok(xml) => match extract_response(&xml, action) {
                    // Not a fault after all - the device sent an error status
                    // with a non-fault body. Report it as a transport failure.
                    Ok(_) => SoapError::Network(format!("HTTP {status}")),
                    Err(err) => err,
                },
                Err(_) => SoapError::Network(format!("HTTP {status}: {body}")),
            },
            Err(e) => SoapError::Network(format!("HTTP {status}: failed to read body: {e}")),
        },
        ureq::Error::Transport(transport) => SoapError::Network(transport.to_string()),
    }
}

/// Map a `ureq` error from a SUBSCRIBE/UNSUBSCRIBE request into a [`SoapError`].
///
/// Subscription endpoints do not return SOAP envelopes, so there is no fault to
/// parse. The HTTP status is still the actionable detail (e.g. `412 Precondition
/// Failed` for an expired SID), so it is preserved in the message rather than
/// collapsed into an opaque `ureq` string.
fn map_subscription_error(operation: &str, error: ureq::Error) -> SoapError {
    match error {
        ureq::Error::Status(status, _) => {
            SoapError::Network(format!("{operation} failed: HTTP {status}"))
        }
        ureq::Error::Transport(transport) => {
            SoapError::Network(format!("{operation} failed: {transport}"))
        }
    }
}

/// Extract the action response element from a SOAP envelope, or the fault it carries.
fn extract_response(xml: &Element, action: &str) -> Result<Element, SoapError> {
    let body = xml
        .get_child("Body")
        .ok_or_else(|| SoapError::Parse("Missing SOAP Body".to_string()))?;

    // Check for SOAP fault first
    if let Some(fault) = body.get_child("Fault") {
        return Err(parse_fault(fault));
    }

    // Extract the action response
    let response_name = format!("{action}Response");
    body.get_child(response_name.as_str())
        .cloned()
        .ok_or_else(|| SoapError::Parse(format!("Missing {response_name} element")))
}

/// Parse a `<s:Fault>` element into [`SoapError::Fault`].
fn parse_fault(fault: &Element) -> SoapError {
    let upnp_error = fault.get_child("detail").and_then(|detail| {
        detail
            .get_child(UPNP_ERROR_ELEMENT)
            .or_else(|| detail.get_child(UPNP_ERROR_ELEMENT_LEGACY))
    });

    let code = upnp_error
        .and_then(|e| e.get_child("errorCode"))
        .and_then(|c| c.get_text())
        .and_then(|t| t.trim().parse::<u16>().ok())
        .unwrap_or(UNKNOWN_FAULT_CODE);

    let description = upnp_error
        .and_then(|e| e.get_child("errorDescription"))
        .and_then(|d| d.get_text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

    SoapError::Fault { code, description }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soap_client_creation() {
        // Test singleton pattern
        let _client = SoapClient::get();

        // Test that the client can be created without panicking
        // and that it has the expected timeout configuration
        // We can't easily test the timeout values directly, but we can verify
        // the client was created successfully
        let _default_client = SoapClient::default();

        // Test that cloning works efficiently
        let _cloned_client = SoapClient::get().clone();
    }

    #[test]
    fn test_singleton_pattern_consistency() {
        // Test that multiple calls to get() return references to the same instance
        let client1 = SoapClient::get();
        let client2 = SoapClient::get();

        // Both should point to the same static instance
        assert!(std::ptr::eq(client1, client2));

        // Clones should have the same Arc reference count
        let cloned1 = client1.clone();
        let cloned2 = client2.clone();

        // All clones should share the same underlying agent
        assert!(Arc::ptr_eq(&cloned1.agent, &cloned2.agent));
    }

    #[test]
    fn test_extract_response_with_valid_response() {
        let client = SoapClient::get();

        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <u:PlayResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
                    </u:PlayResponse>
                </s:Body>
            </s:Envelope>
        "#;

        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "PlayResponse");
    }

    #[test]
    fn test_extract_response_with_soap_fault() {
        let client = SoapClient::get();

        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <faultcode>s:Client</faultcode>
                        <faultstring>UPnPError</faultstring>
                        <detail>
                            <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
                                <errorCode>401</errorCode>
                                <errorDescription>Invalid Action</errorDescription>
                            </UPnPError>
                        </detail>
                    </s:Fault>
                </s:Body>
            </s:Envelope>
        "#;

        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");

        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Fault { code, description } => {
                assert_eq!(code, 401);
                assert_eq!(description.as_deref(), Some("Invalid Action"));
            }
            other => panic!("Expected SoapError::Fault, got {other:?}"),
        }
    }

    /// Regression: devices spelling the element the legacy way must still work.
    #[test]
    fn test_extract_response_with_legacy_upnperror_spelling() {
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <faultcode>s:Client</faultcode>
                        <faultstring>UPnPError</faultstring>
                        <detail>
                            <UpnPError xmlns="urn:schemas-upnp-org:control-1-0">
                                <errorCode>701</errorCode>
                                <errorDescription>Transition not available</errorDescription>
                            </UpnPError>
                        </detail>
                    </s:Fault>
                </s:Body>
            </s:Envelope>
        "#;

        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        match extract_response(&xml, "Play").unwrap_err() {
            SoapError::Fault { code, description } => {
                assert_eq!(code, 701);
                assert_eq!(description.as_deref(), Some("Transition not available"));
            }
            other => panic!("Expected SoapError::Fault, got {other:?}"),
        }
    }

    /// Regression: `ureq` reports UPnP faults as `Error::Status(500, ..)`. The
    /// fault body must be read so callers see `Fault`, not `Network`.
    #[test]
    fn test_status_500_with_fault_body_yields_fault() {
        let fault_body = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <faultcode>s:Client</faultcode>
                        <faultstring>UPnPError</faultstring>
                        <detail>
                            <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
                                <errorCode>402</errorCode>
                                <errorDescription>Invalid Args</errorDescription>
                            </UPnPError>
                        </detail>
                    </s:Fault>
                </s:Body>
            </s:Envelope>"#;

        let response = ureq::Response::new(500, "Internal Server Error", fault_body).unwrap();
        let error = ureq::Error::Status(500, response);

        match map_ureq_error(error, "SetVolume") {
            SoapError::Fault { code, description } => {
                assert_eq!(code, 402);
                assert_eq!(description.as_deref(), Some("Invalid Args"));
            }
            other => panic!("Expected SoapError::Fault, got {other:?}"),
        }
    }

    /// An error status with a non-SOAP body has no fault to report, so it stays
    /// a transport-level failure.
    #[test]
    fn test_status_error_without_fault_body_yields_network() {
        let response = ureq::Response::new(503, "Service Unavailable", "device busy").unwrap();
        let error = ureq::Error::Status(503, response);

        match map_ureq_error(error, "Play") {
            SoapError::Network(msg) => assert!(msg.contains("503"), "unexpected message: {msg}"),
            other => panic!("Expected SoapError::Network, got {other:?}"),
        }
    }

    #[test]
    fn test_is_success_accepts_non_200_success_codes() {
        // ureq errors on >= 400, so these checks only ever see 1xx-3xx.
        assert!(is_success(200));
        assert!(is_success(201));
        assert!(!is_success(302));
    }

    #[test]
    fn test_extract_response_missing_body() {
        let client = SoapClient::get();

        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
            </s:Envelope>
        "#;

        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");

        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Parse(msg) => assert!(msg.contains("Missing SOAP Body")),
            _ => panic!("Expected SoapError::Parse"),
        }
    }

    #[test]
    fn test_extract_response_missing_action_response() {
        let client = SoapClient::get();

        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                </s:Body>
            </s:Envelope>
        "#;

        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");

        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Parse(msg) => assert!(msg.contains("Missing PlayResponse element")),
            _ => panic!("Expected SoapError::Parse"),
        }
    }

    #[test]
    fn test_soap_fault_with_default_error_code() {
        let client = SoapClient::get();

        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <faultcode>s:Server</faultcode>
                        <faultstring>Internal Error</faultstring>
                    </s:Fault>
                </s:Body>
            </s:Envelope>
        "#;

        let xml = Element::parse(xml_str.as_bytes()).unwrap();
        let result = client.extract_response(&xml, "Play");

        assert!(result.is_err());
        match result.unwrap_err() {
            SoapError::Fault { code, description } => {
                assert_eq!(code, UNKNOWN_FAULT_CODE);
                assert_eq!(description, None);
            }
            other => panic!("Expected SoapError::Fault, got {other:?}"),
        }
    }

    #[test]
    fn test_subscription_error_preserves_http_status() {
        let response = ureq::Response::new(412, "Precondition Failed", "").unwrap();
        let error = ureq::Error::Status(412, response);

        match map_subscription_error("SUBSCRIBE", error) {
            SoapError::Network(msg) => {
                assert!(msg.contains("SUBSCRIBE"), "unexpected message: {msg}");
                assert!(msg.contains("412"), "unexpected message: {msg}");
            }
            other => panic!("Expected SoapError::Network, got {other:?}"),
        }
    }
}
