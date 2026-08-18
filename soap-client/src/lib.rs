//! Internal implementation detail of [`sonos-sdk`](https://crates.io/crates/sonos-sdk). Not intended for direct use.
//!
//! Private SOAP client for UPnP device communication
//!
//! This crate provides a minimal SOAP client specifically designed for
//! communicating with UPnP devices like Sonos speakers. It also supports
//! UPnP event subscriptions using SUBSCRIBE/UNSUBSCRIBE methods.

// This workspace contains no `unsafe` code. Asserted here so a future
// addition is a hard compile error, not a silent change in guarantees.
#![forbid(unsafe_code)]

mod error;

pub use error::SoapError;

use quick_xml::events::Event;
use quick_xml::Reader;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

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

    /// Send a SOAP request and return the raw response body.
    ///
    /// The body is handed back as text rather than a parsed DOM: response *shape*
    /// is service-specific and belongs to `sonos-api`, while this crate is only
    /// responsible for transport and for distinguishing a device refusal (a SOAP
    /// fault) from a successful answer. That check still happens here, so a
    /// returned `Ok(String)` is guaranteed to be an envelope carrying
    /// `<{action}Response>` and no `<Fault>`.
    pub fn call(
        &self,
        ip: &str,
        endpoint: &str,
        service_uri: &str,
        action: &str,
        payload: &str,
    ) -> Result<String, SoapError> {
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

        // Reject faults and malformed envelopes before handing the body upstream.
        check_response(&xml_text, action)?;

        Ok(xml_text)
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

        require_success("SUBSCRIBE", &response)?;

        // Extract SID from response headers
        let sid = response
            .header("SID")
            .ok_or_else(|| {
                SoapError::Parse("Missing SID header in SUBSCRIBE response".to_string())
            })?
            .to_string();

        Ok(SubscriptionResponse {
            sid,
            timeout_seconds: granted_timeout(&response, timeout_seconds),
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

        require_success("SUBSCRIBE renewal", &response)?;

        Ok(granted_timeout(&response, timeout_seconds))
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

        require_success("UNSUBSCRIBE", &response)?;

        Ok(())
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

/// Reject a subscription response whose status is not a success.
///
/// The message keeps the same `"{operation} failed: HTTP {status}"` shape as
/// [`map_subscription_error`], so a status surfaced by `ureq` as an error and one
/// that arrived on the success path read identically to callers.
fn require_success(operation: &str, response: &ureq::Response) -> Result<(), SoapError> {
    if is_success(response.status()) {
        Ok(())
    } else {
        Err(SoapError::Network(format!(
            "{operation} failed: HTTP {}",
            response.status()
        )))
    }
}

/// Read the timeout the device actually granted from the `TIMEOUT` header.
///
/// UPnP spells this `Second-1800`. Devices may grant less than was asked for, so
/// the header wins when it parses; anything else (absent, `infinite`, malformed)
/// falls back to `requested` rather than failing, since a non-compliant header is
/// not a reason to abandon a working subscription.
fn granted_timeout(response: &ureq::Response, requested: u32) -> u32 {
    response
        .header("TIMEOUT")
        .and_then(|s| s.strip_prefix("Second-")?.parse::<u32>().ok())
        .unwrap_or(requested)
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
            // A parseable envelope tells us why the device refused.
            Ok(body) => match check_response(&body, action) {
                // Not a fault after all - the device sent an error status with a
                // non-fault body. Report it as a transport failure.
                Ok(()) => SoapError::Network(format!("HTTP {status}")),
                Err(SoapError::Fault { code, description }) => {
                    SoapError::Fault { code, description }
                }
                // The body was not a usable SOAP envelope at all. Keep it in the
                // message: for an error status it is the only diagnostic there is.
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

/// What a SOAP response body turned out to be.
///
/// Mirrors the outcomes of the previous DOM-based `extract_response`, kept as a
/// distinct type because the success path and the HTTP-error path map the same
/// outcomes to different [`SoapError`]s (see [`map_ureq_error`]).
#[derive(Debug)]
enum EnvelopeScan {
    /// `<{action}Response>` is present and there is no fault.
    Response,
    /// The envelope carries a `<Fault>`.
    Fault {
        code: u16,
        description: Option<String>,
    },
    /// No `<Body>` child on the root element.
    MissingBody,
    /// `<Body>` is present but carries no `<{action}Response>`.
    MissingResponse,
    /// Not a usable XML document: a syntax error, or no element at all.
    Malformed(String),
}

/// The two fields read out of a UPnP fault detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultField {
    Code,
    Description,
}

/// Fault fields collected for one spelling of the UPnP error element.
#[derive(Debug, Default)]
struct FaultFields {
    code: Option<String>,
    description: Option<String>,
}

impl FaultFields {
    /// Append to a field rather than overwrite it, so an element split across
    /// several text/CDATA nodes reads the same as `xmltree`'s `get_text`, which
    /// concatenated them.
    fn push(&mut self, field: FaultField, text: &str) {
        let slot = match field {
            FaultField::Code => &mut self.code,
            FaultField::Description => &mut self.description,
        };
        slot.get_or_insert_with(String::new).push_str(text);
    }

    fn is_empty(&self) -> bool {
        self.code.is_none() && self.description.is_none()
    }

    /// Apply the same normalization the DOM implementation did: trim, parse the
    /// code with [`UNKNOWN_FAULT_CODE`] as fallback, and treat a blank
    /// description as absent.
    fn into_scan(self) -> EnvelopeScan {
        EnvelopeScan::Fault {
            code: self
                .code
                .as_deref()
                .and_then(|t| t.trim().parse::<u16>().ok())
                .unwrap_or(UNKNOWN_FAULT_CODE),
            description: self
                .description
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty()),
        }
    }
}

/// Verify that a SOAP response body is a successful `<{action}Response>` envelope.
///
/// Returns `Ok(())` for a usable response, [`SoapError::Fault`] when the device
/// refused the action, and [`SoapError::Parse`] when the body is not a response
/// envelope for `action` at all.
fn check_response(xml: &str, action: &str) -> Result<(), SoapError> {
    match scan_envelope(xml, action) {
        EnvelopeScan::Response => Ok(()),
        scan => Err(scan_to_error(scan, action)),
    }
}

/// Convert a non-success [`EnvelopeScan`] into the [`SoapError`] it represents.
///
/// # Panics
///
/// Panics if given [`EnvelopeScan::Response`], which is not an error.
fn scan_to_error(scan: EnvelopeScan, action: &str) -> SoapError {
    match scan {
        EnvelopeScan::Fault { code, description } => SoapError::Fault { code, description },
        EnvelopeScan::MissingBody => SoapError::Parse("Missing SOAP Body".to_string()),
        EnvelopeScan::MissingResponse => {
            SoapError::Parse(format!("Missing {action}Response element"))
        }
        EnvelopeScan::Malformed(msg) => SoapError::Parse(msg),
        EnvelopeScan::Response => unreachable!("Response is not an error"),
    }
}

/// Whether the current element path, ignoring the root envelope element, is
/// exactly `expected`.
fn at_path(path: &[String], expected: &[&str]) -> bool {
    path.len() == expected.len() + 1 && path[1..].iter().zip(expected).all(|(a, b)| a == b)
}

/// The fault field the current element path points at, if any.
///
/// The `bool` is `true` for the spec spelling of the UPnP error element and
/// `false` for the tolerated legacy misspelling.
fn fault_field_at(path: &[String]) -> Option<(bool, FaultField)> {
    for (is_spec, element) in [
        (true, UPNP_ERROR_ELEMENT),
        (false, UPNP_ERROR_ELEMENT_LEGACY),
    ] {
        for (field, name) in [
            (FaultField::Code, "errorCode"),
            (FaultField::Description, "errorDescription"),
        ] {
            if at_path(path, &["Body", "Fault", "detail", element, name]) {
                return Some((is_spec, field));
            }
        }
    }
    None
}

/// Pick the [`FaultFields`] accumulator for the spelling the device used.
fn fields_for<'a>(
    is_spec: bool,
    spec: &'a mut FaultFields,
    legacy: &'a mut FaultFields,
) -> &'a mut FaultFields {
    if is_spec {
        spec
    } else {
        legacy
    }
}

/// Scan a SOAP envelope for the action response or the fault it carries.
///
/// Single-pass over the document with `quick-xml`. Element matching is on the
/// **local** name (so `s:Body` matches `Body`) and on the **exact child path**,
/// which is what the previous `xmltree::Element::get_child` chain did: `Body` had
/// to be a direct child of the root, `Fault` of `Body`, `detail` of `Fault`, and so
/// on. A looser "anywhere in the document" match would let a `<Fault>` nested in
/// unrelated response content turn a successful call into an error.
fn scan_envelope(xml: &str, action: &str) -> EnvelopeScan {
    let response_name = format!("{action}Response");

    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut saw_root = false;
    let mut saw_body = false;
    let mut saw_fault = false;
    let mut saw_response = false;
    // Collected per spelling so the spec spelling wins wholesale if a device
    // somehow emits both, matching the old `get_child(..).or_else(..)` which
    // picked one element and read both fields from it.
    let mut spec_fields = FaultFields::default();
    let mut legacy_fields = FaultFields::default();
    // Set while inside an `<errorCode>`/`<errorDescription>` leaf.
    let mut collecting: Option<(bool, FaultField)> = None;

    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(e) => return EnvelopeScan::Malformed(e.to_string()),
        };

        match event {
            Event::Eof => break,

            Event::Start(start) => {
                path.push(String::from_utf8_lossy(start.local_name().as_ref()).into_owned());
                saw_root = true;

                if at_path(&path, &["Body"]) {
                    saw_body = true;
                } else if at_path(&path, &["Body", "Fault"]) {
                    saw_fault = true;
                } else if path.len() == 3 && path[1] == "Body" && path[2] == response_name {
                    saw_response = true;
                } else {
                    collecting = fault_field_at(&path);
                }
            }

            // A self-closing element has no children and no text, so it only
            // affects the "did we see it" flags - never `collecting`.
            Event::Empty(start) => {
                path.push(String::from_utf8_lossy(start.local_name().as_ref()).into_owned());
                saw_root = true;

                if at_path(&path, &["Body"]) {
                    saw_body = true;
                } else if at_path(&path, &["Body", "Fault"]) {
                    saw_fault = true;
                } else if path.len() == 3 && path[1] == "Body" && path[2] == response_name {
                    saw_response = true;
                }
                path.pop();
            }

            Event::Text(text) => {
                if let Some((is_spec, field)) = collecting {
                    let decoded = match text.unescape() {
                        Ok(decoded) => decoded,
                        Err(e) => return EnvelopeScan::Malformed(e.to_string()),
                    };
                    fields_for(is_spec, &mut spec_fields, &mut legacy_fields).push(field, &decoded);
                }
            }

            Event::CData(cdata) => {
                if let Some((is_spec, field)) = collecting {
                    let decoded = String::from_utf8_lossy(&cdata).into_owned();
                    fields_for(is_spec, &mut spec_fields, &mut legacy_fields).push(field, &decoded);
                }
            }

            Event::End(_) => {
                path.pop();
                collecting = fault_field_at(&path);
            }

            _ => {}
        }
    }

    if !saw_root {
        return EnvelopeScan::Malformed("response body is not an XML document".to_string());
    }
    // `quick-xml` reports `Eof` for a document whose tags are still open rather
    // than erroring, so a response truncated mid-transfer would otherwise scan as
    // a perfectly good `<{action}Response>`. `xmltree::Element::parse` rejected
    // those, and it should stay rejected: a half-received body is not an answer.
    if !path.is_empty() {
        return EnvelopeScan::Malformed(format!(
            "unexpected end of XML: <{}> was never closed",
            path[path.len() - 1]
        ));
    }
    if !saw_body {
        return EnvelopeScan::MissingBody;
    }
    if saw_fault {
        return if spec_fields.is_empty() {
            legacy_fields.into_scan()
        } else {
            spec_fields.into_scan()
        };
    }
    if !saw_response {
        return EnvelopeScan::MissingResponse;
    }
    EnvelopeScan::Response
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
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <u:PlayResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
                    </u:PlayResponse>
                </s:Body>
            </s:Envelope>
        "#;

        assert!(check_response(xml_str, "Play").is_ok());
    }

    /// A response for a *different* action must not be accepted just because the
    /// envelope is well-formed.
    #[test]
    fn test_extract_response_rejects_other_action_response() {
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <u:PauseResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
                    </u:PauseResponse>
                </s:Body>
            </s:Envelope>
        "#;

        match check_response(xml_str, "Play").unwrap_err() {
            SoapError::Parse(msg) => assert!(msg.contains("Missing PlayResponse element")),
            other => panic!("Expected SoapError::Parse, got {other:?}"),
        }
    }

    /// Self-closing response elements are spec-legal and carry the same meaning as
    /// an empty open/close pair.
    #[test]
    fn test_extract_response_accepts_self_closing_response() {
        let xml_str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body><u:PlayResponse xmlns:u="urn:x"/></s:Body>
            </s:Envelope>"#;

        assert!(check_response(xml_str, "Play").is_ok());
    }

    /// A `<Fault>` nested inside unrelated response content is not a device
    /// refusal. Only `Body > Fault` is, which is what the old `get_child` chain
    /// matched.
    #[test]
    fn test_extract_response_ignores_non_toplevel_fault() {
        let xml_str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <u:PlayResponse xmlns:u="urn:x">
                        <Detail><Fault>not a soap fault</Fault></Detail>
                    </u:PlayResponse>
                </s:Body>
            </s:Envelope>"#;

        assert!(check_response(xml_str, "Play").is_ok());
    }

    /// Garbage that is not XML at all is a parse failure, not a missing Body.
    #[test]
    fn test_extract_response_with_non_xml_body() {
        match check_response("device busy", "Play").unwrap_err() {
            SoapError::Parse(_) => {}
            other => panic!("Expected SoapError::Parse, got {other:?}"),
        }
    }

    /// A body truncated mid-transfer must not scan as a valid response. `quick-xml`
    /// reports `Eof` rather than an error for unclosed tags, so this is checked
    /// explicitly; `xmltree::Element::parse` used to reject it for us.
    #[test]
    fn test_extract_response_rejects_truncated_envelope() {
        let truncated = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body><u:PlayResponse xmlns:u="urn:x">"#;

        match check_response(truncated, "Play").unwrap_err() {
            SoapError::Parse(msg) => {
                assert!(msg.contains("never closed"), "unexpected message: {msg}");
            }
            other => panic!("Expected SoapError::Parse, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_response_with_soap_fault() {
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

        match check_response(xml_str, "Play").unwrap_err() {
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

        match check_response(xml_str, "Play").unwrap_err() {
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
    fn test_require_success_preserves_status_and_operation() {
        let ok = ureq::Response::new(200, "OK", "").unwrap();
        assert!(require_success("SUBSCRIBE", &ok).is_ok());

        let redirect = ureq::Response::new(302, "Found", "").unwrap();
        match require_success("UNSUBSCRIBE", &redirect).unwrap_err() {
            SoapError::Network(msg) => {
                assert!(msg.contains("UNSUBSCRIBE"), "unexpected message: {msg}");
                assert!(msg.contains("302"), "unexpected message: {msg}");
            }
            other => panic!("Expected SoapError::Network, got {other:?}"),
        }
    }

    /// The device's granted timeout wins when the header parses; every other
    /// shape falls back to what was requested rather than failing.
    #[test]
    fn test_granted_timeout_parsing_and_fallback() {
        let with = |header: Option<&str>| {
            let mut raw = "HTTP/1.1 200 OK\r\n".to_string();
            if let Some(value) = header {
                raw.push_str(&format!("TIMEOUT: {value}\r\n"));
            }
            raw.push_str("\r\n");
            let response: ureq::Response = raw.parse().unwrap();
            granted_timeout(&response, 1800)
        };

        assert_eq!(with(Some("Second-600")), 600);
        assert_eq!(with(Some("Second-infinite")), 1800);
        assert_eq!(with(Some("600")), 1800);
        assert_eq!(with(None), 1800);
    }

    #[test]
    fn test_extract_response_missing_body() {
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
            </s:Envelope>
        "#;

        match check_response(xml_str, "Play").unwrap_err() {
            SoapError::Parse(msg) => assert!(msg.contains("Missing SOAP Body")),
            _ => panic!("Expected SoapError::Parse"),
        }
    }

    #[test]
    fn test_extract_response_missing_action_response() {
        let xml_str = r#"
            <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                </s:Body>
            </s:Envelope>
        "#;

        match check_response(xml_str, "Play").unwrap_err() {
            SoapError::Parse(msg) => assert!(msg.contains("Missing PlayResponse element")),
            _ => panic!("Expected SoapError::Parse"),
        }
    }

    #[test]
    fn test_soap_fault_with_default_error_code() {
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

        match check_response(xml_str, "Play").unwrap_err() {
            SoapError::Fault { code, description } => {
                assert_eq!(code, UNKNOWN_FAULT_CODE);
                assert_eq!(description, None);
            }
            other => panic!("Expected SoapError::Fault, got {other:?}"),
        }
    }

    /// A blank `<errorDescription>` is normalized to `None`, matching the old
    /// `.filter(|t| !t.is_empty())`.
    #[test]
    fn test_soap_fault_blank_description_is_none() {
        let xml_str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <detail>
                            <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
                                <errorCode>402</errorCode>
                                <errorDescription>   </errorDescription>
                            </UPnPError>
                        </detail>
                    </s:Fault>
                </s:Body>
            </s:Envelope>"#;

        match check_response(xml_str, "Play").unwrap_err() {
            SoapError::Fault { code, description } => {
                assert_eq!(code, 402);
                assert_eq!(description, None);
            }
            other => panic!("Expected SoapError::Fault, got {other:?}"),
        }
    }

    /// A non-numeric `<errorCode>` falls back to [`UNKNOWN_FAULT_CODE`] rather
    /// than degrading the fault into a parse error.
    #[test]
    fn test_soap_fault_unparseable_code_falls_back() {
        let xml_str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <detail>
                            <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
                                <errorCode>not-a-number</errorCode>
                                <errorDescription>Invalid Args</errorDescription>
                            </UPnPError>
                        </detail>
                    </s:Fault>
                </s:Body>
            </s:Envelope>"#;

        match check_response(xml_str, "Play").unwrap_err() {
            SoapError::Fault { code, description } => {
                assert_eq!(code, UNKNOWN_FAULT_CODE);
                assert_eq!(description.as_deref(), Some("Invalid Args"));
            }
            other => panic!("Expected SoapError::Fault, got {other:?}"),
        }
    }

    /// Escaped entities in a fault description are decoded, as `xmltree` did.
    #[test]
    fn test_soap_fault_description_is_unescaped() {
        let xml_str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
                <s:Body>
                    <s:Fault>
                        <detail>
                            <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
                                <errorCode>402</errorCode>
                                <errorDescription>Bad &amp; wrong</errorDescription>
                            </UPnPError>
                        </detail>
                    </s:Fault>
                </s:Body>
            </s:Envelope>"#;

        match check_response(xml_str, "Play").unwrap_err() {
            SoapError::Fault { description, .. } => {
                assert_eq!(description.as_deref(), Some("Bad & wrong"));
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
