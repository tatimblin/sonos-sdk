//! Enhanced operation framework with composability and validation support
//!
//! This module provides the core framework for UPnP operations with advanced features:
//! - Composable operations that can be chained, batched, or made conditional
//! - Dual validation strategy (boundary vs comprehensive)
//! - Fluent builder pattern for operation construction
//! - Strong type safety with minimal boilerplate

mod builder;
pub mod macros;

pub use builder::*;

// Legacy SonosOperation trait for backward compatibility
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::ApiError;
use crate::service::Service;

/// Base trait for all Sonos API operations (LEGACY)
///
/// This trait defines the common interface that all Sonos UPnP operations must implement.
/// It provides type safety through associated types and ensures consistent patterns
/// for request/response handling across all operations.
///
/// **Note**: This is the legacy trait. New code should use `UPnPOperation` instead.
pub trait SonosOperation {
    /// The request type for this operation, must be serializable
    type Request: Serialize;

    /// The response type for this operation, must be deserializable
    type Response: for<'de> Deserialize<'de>;

    /// The UPnP service this operation belongs to
    const SERVICE: Service;

    /// The SOAP action name for this operation
    const ACTION: &'static str;

    /// Build the SOAP payload from the request data
    ///
    /// This method should construct the XML payload that goes inside the SOAP envelope.
    /// The payload should contain all the parameters needed for the UPnP action.
    ///
    /// # Arguments
    /// * `request` - The typed request data
    ///
    /// # Returns
    /// A string containing the XML payload (without SOAP envelope)
    fn build_payload(request: &Self::Request) -> String;

    /// Parse the SOAP response XML into the typed response
    ///
    /// This method extracts the relevant data from the SOAP response XML and
    /// converts it into the strongly-typed response structure.
    ///
    /// # Arguments
    /// * `xml` - The raw SOAP response body
    ///
    /// # Returns
    /// The typed response data or an error if parsing fails
    fn parse_response(xml: &str) -> Result<Self::Response, ApiError>;
}

/// Validation error types
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Parameter '{parameter}' value '{value}' is out of range ({min}..={max})")]
    RangeError {
        parameter: String,
        value: String,
        min: String,
        max: String,
    },

    #[error("Parameter '{parameter}' value '{value}' is invalid: {reason}")]
    InvalidValue {
        parameter: String,
        value: String,
        reason: String,
    },

    #[error("Required parameter '{parameter}' is missing")]
    MissingParameter { parameter: String },

    #[error("Parameter '{parameter}' failed validation: {message}")]
    Custom { parameter: String, message: String },
}

impl ValidationError {
    pub fn range_error(
        parameter: &str,
        min: impl std::fmt::Display,
        max: impl std::fmt::Display,
        value: impl std::fmt::Display,
    ) -> Self {
        Self::RangeError {
            parameter: parameter.to_string(),
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        }
    }

    pub fn invalid_value(parameter: &str, value: impl std::fmt::Display) -> Self {
        Self::InvalidValue {
            parameter: parameter.to_string(),
            value: value.to_string(),
            reason: "invalid format or content".to_string(),
        }
    }
}

/// Validation levels for operation parameters
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ValidationLevel {
    /// No validation - maximum performance
    None,
    /// Basic validation - type and range checks
    #[default]
    Basic,
}

/// Trait for types that can be validated
pub trait Validate {
    /// Perform basic validation
    ///
    /// This should include type checks and range validation
    /// to fail fast on obviously invalid input.
    fn validate_basic(&self) -> Result<(), ValidationError> {
        Ok(()) // Default: no validation
    }

    /// Validate with the specified level
    fn validate(&self, level: ValidationLevel) -> Result<(), ValidationError> {
        match level {
            ValidationLevel::None => Ok(()),
            ValidationLevel::Basic => self.validate_basic(),
        }
    }
}

/// Enhanced UPnP operation trait with composability support
///
/// This trait extends the original SonosOperation concept with:
/// - Composability: operations can be chained, batched, or made conditional
/// - Validation: flexible validation strategy with boundary and comprehensive levels
/// - Dependencies: operations can declare dependencies on other operations
/// - Batching: operations can indicate whether they can be batched with others
pub trait UPnPOperation {
    /// The request type for this operation, must be serializable and validatable
    type Request: Serialize + Validate;

    /// The response type for this operation, must be deserializable
    type Response: for<'de> Deserialize<'de>;

    /// The UPnP service this operation belongs to
    const SERVICE: Service;

    /// The SOAP action name for this operation
    const ACTION: &'static str;

    /// Build the SOAP payload from the request data with validation
    ///
    /// This method validates the request according to the validation level
    /// and then constructs the XML payload for the SOAP envelope.
    ///
    /// # Arguments
    /// * `request` - The typed request data
    ///
    /// # Returns
    /// A string containing the XML payload or a validation error
    fn build_payload(request: &Self::Request) -> Result<String, ValidationError>;

    /// Parse the SOAP response XML into the typed response
    ///
    /// This method extracts the relevant data from the SOAP response XML and
    /// converts it into the strongly-typed response structure.
    ///
    /// # Arguments
    /// * `xml` - The raw SOAP response body
    ///
    /// # Returns
    /// The typed response data or an error if parsing fails
    fn parse_response(xml: &str) -> Result<Self::Response, ApiError>;

    /// Get the list of operations this operation depends on
    ///
    /// This is used for operation ordering and dependency resolution
    /// in batch and sequence operations.
    ///
    /// # Returns
    /// A slice of action names that must be executed before this operation
    fn dependencies() -> &'static [&'static str] {
        &[]
    }

    /// Check if this operation can be batched with another operation
    ///
    /// Some operations may have conflicts or dependencies that prevent
    /// them from being executed in parallel.
    ///
    /// # Type Parameters
    /// * `T` - Another UPnP operation type to check compatibility with
    ///
    /// # Returns
    /// True if the operations can be safely executed in parallel
    fn can_batch_with<T: UPnPOperation>() -> bool {
        true // Default: most operations can be batched
    }

    /// Get human-readable operation metadata
    ///
    /// This is useful for debugging, logging, and SDK development
    fn metadata() -> OperationMetadata {
        OperationMetadata {
            service: Self::SERVICE.name(),
            action: Self::ACTION,
            dependencies: Self::dependencies(),
        }
    }
}

/// Metadata about a UPnP operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationMetadata {
    /// The service name (e.g., "AVTransport")
    pub service: &'static str,
    /// The action name (e.g., "Play")
    pub action: &'static str,
    /// List of operations this operation depends on
    pub dependencies: &'static [&'static str],
}

/// Read the text content of a named argument out of a SOAP response body.
///
/// UPnP action responses are flat: every out-argument is a leaf element directly
/// under `<{action}Response>`, which is itself under `<Body>`. Rather than build a
/// DOM, this walks the document once with `quick-xml` and returns the text of the
/// first element whose **local** name matches `name`, so namespace prefixes
/// (`u:CurrentVolume`) match unprefixed lookups. Text split across several nodes is
/// concatenated, and entities are decoded.
///
/// Returns `None` when the element is absent, and `Some("")` when it is present but
/// empty — the same distinction `xmltree`'s `get_child(..).get_text()` drew, except
/// that `get_text()` also returned `None` for a childless element. Callers here all
/// funnel through `unwrap_or_default()`/`parse().ok()`, so the two are equivalent
/// downstream.
pub fn response_text(xml: &str, name: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut depth_in_target: Option<usize> = None;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,

            Ok(Event::Start(start)) => {
                match depth_in_target {
                    // Nested markup inside the argument. Its text is *not*
                    // collected (only direct text children count, as with
                    // `xmltree`'s `get_text`), but the depth must be tracked so
                    // its end tag does not terminate the search early.
                    Some(depth) => depth_in_target = Some(depth + 1),
                    None => {
                        if start.local_name().as_ref() == name.as_bytes() {
                            depth_in_target = Some(0);
                        }
                    }
                }
            }

            Ok(Event::Empty(empty)) => {
                // A self-closing match has no text content.
                if depth_in_target.is_none() && empty.local_name().as_ref() == name.as_bytes() {
                    return Some(String::new());
                }
            }

            Ok(Event::Text(raw)) => {
                if depth_in_target == Some(0) {
                    if let Ok(decoded) = raw.unescape() {
                        text.push_str(&decoded);
                    }
                }
            }

            Ok(Event::CData(raw)) => {
                if depth_in_target == Some(0) {
                    text.push_str(&String::from_utf8_lossy(&raw));
                }
            }

            Ok(Event::End(_)) => match depth_in_target {
                Some(0) => return Some(text),
                Some(depth) => depth_in_target = Some(depth - 1),
                None => {}
            },

            _ => {}
        }
    }

    None
}

/// Read a named response argument and parse it, falling back to the type's
/// default when the argument is absent or unparseable.
///
/// This is the exact behavior of the old
/// `get_child(..).get_text().parse().ok().unwrap_or_default()` chain that every
/// macro-generated `parse_response` used, kept in one place.
pub fn response_field<T: FromStr + Default>(xml: &str, name: &str) -> T {
    response_text(xml, name)
        .and_then(|s| s.parse().ok())
        .unwrap_or_default()
}

/// Read a named response argument as an owned string, defaulting to empty.
pub fn response_string(xml: &str, name: &str) -> String {
    response_text(xml, name).unwrap_or_default()
}

/// Parse a Sonos UPnP boolean argument out of a SOAP response body.
///
/// Sonos devices return "0"/"1" for booleans, but Rust's `bool::parse()` only
/// handles "true"/"false". This helper correctly parses "0", "1", "true", "false",
/// and handles whitespace-padded variants.
///
/// Returns `false` if the argument is missing or empty.
pub fn parse_sonos_bool(xml: &str, name: &str) -> bool {
    response_text(xml, name)
        .map(|s| s.trim() == "1" || s.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Escape XML special characters in a string for safe SOAP payload interpolation.
///
/// Replaces `&`, `<`, `>`, `"`, and `'` with their XML entity equivalents.
///
/// Delegates to `quick_xml::escape::escape`, whose predicate is exactly those five
/// characters. Notably **not** `partial_escape`, which leaves `"` and `'` alone and
/// would therefore be unsafe for values interpolated in attribute position.
/// Whitespace is left verbatim: `escape`'s predicate never matches space or tab, so
/// the numeric-reference arms inside quick-xml's shared `_escape` helper (which exist
/// for `xs:list` delimiters) are unreachable from here. That matters because SOAP
/// payloads carry track titles and URIs where `&#32;` would corrupt the value.
pub fn xml_escape(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

/// Capitalize the first character of a snake_case field name.
///
/// Used by `define_operation_with_response!` to derive the UPnP element name for
/// single-word request arguments (`channel` -> `Channel`).
pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

/// Compile-time guard rejecting request field names whose UPnP element name cannot be
/// derived by capitalizing the first character.
///
/// UPnP argument names come from each device's SCPD and use casing that snake_case
/// does not preserve (`object_id` -> `ObjectID`, `enqueued_uri` -> `EnqueuedURI`).
/// Capitalizing only the first character would emit `<Object_id>`, which devices
/// reject. Multi-word request fields must therefore declare their element name via
/// the `request_xml_mapping:` block; this function makes forgetting a compile error
/// rather than a malformed request discovered at runtime.
///
/// # Panics
///
/// Panics (at compile time, when used in a `const` context) if `name` contains `_`.
pub const fn assert_derivable_arg_name(name: &str) {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        assert!(
            bytes[i] != b'_',
            "multi-word request field needs an explicit `request_xml_mapping:` entry: \
             UPnP element casing cannot be derived from snake_case"
        );
        i += 1;
    }
}

/// Validate a RenderingControl channel parameter.
///
/// Sonos speakers accept "Master", "LF" (left front), and "RF" (right front) channels.
pub fn validate_channel(channel: &str) -> Result<(), ValidationError> {
    match channel {
        "Master" | "LF" | "RF" => Ok(()),
        other => Err(ValidationError::Custom {
            parameter: "channel".to_string(),
            message: format!("Invalid channel '{other}'. Must be 'Master', 'LF', or 'RF'"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error_creation() {
        let error = ValidationError::range_error("volume", 0, 100, 150);
        assert!(error.to_string().contains("volume"));
        assert!(error.to_string().contains("150"));
        assert!(error.to_string().contains("0..=100"));
    }

    #[test]
    fn test_validation_level_default() {
        assert_eq!(ValidationLevel::default(), ValidationLevel::Basic);
    }

    // Mock validation implementation for testing
    struct TestRequest {
        value: i32,
    }

    impl Validate for TestRequest {
        fn validate_basic(&self) -> Result<(), ValidationError> {
            if self.value < 0 || self.value > 100 {
                Err(ValidationError::range_error("value", 0, 100, self.value))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn test_validation_levels() {
        let valid_request = TestRequest { value: 50 };
        assert!(valid_request.validate(ValidationLevel::None).is_ok());
        assert!(valid_request.validate(ValidationLevel::Basic).is_ok());

        let invalid_request = TestRequest { value: 150 };
        assert!(invalid_request.validate(ValidationLevel::None).is_ok());
        assert!(invalid_request.validate(ValidationLevel::Basic).is_err());

        let negative_request = TestRequest { value: -10 };
        assert!(negative_request.validate(ValidationLevel::None).is_ok());
        assert!(negative_request.validate(ValidationLevel::Basic).is_err());
    }

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("hello"), "hello");
        assert_eq!(xml_escape("<script>"), "&lt;script&gt;");
        assert_eq!(xml_escape("a&b"), "a&amp;b");
        assert_eq!(xml_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
        assert_eq!(
            xml_escape("</CurrentURI><Injected>"),
            "&lt;/CurrentURI&gt;&lt;Injected&gt;"
        );
        assert_eq!(xml_escape(""), "");
    }

    /// A realistic SOAP envelope: the argument is nested two levels deep and the
    /// response element is namespace-prefixed.
    const ENVELOPE: &str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
            <s:Body>
                <u:GetVolumeResponse xmlns:u="urn:schemas-upnp-org:service:RenderingControl:1">
                    <CurrentVolume>42</CurrentVolume>
                </u:GetVolumeResponse>
            </s:Body>
        </s:Envelope>"#;

    #[test]
    fn test_response_text_reads_nested_argument() {
        assert_eq!(
            response_text(ENVELOPE, "CurrentVolume").as_deref(),
            Some("42")
        );
        assert_eq!(response_text(ENVELOPE, "NoSuchArgument"), None);
    }

    /// Namespace prefixes on the argument itself must not defeat the lookup;
    /// matching is on the local name, as `xmltree`'s `get_child` was.
    #[test]
    fn test_response_text_ignores_namespace_prefix() {
        let xml = r#"<s:Body><u:GetVolumeResponse><u:CurrentVolume>7</u:CurrentVolume></u:GetVolumeResponse></s:Body>"#;
        assert_eq!(response_text(xml, "CurrentVolume").as_deref(), Some("7"));
    }

    /// An empty element is present-but-blank, distinct from absent.
    #[test]
    fn test_response_text_distinguishes_empty_from_absent() {
        assert_eq!(response_text("<A><B></B></A>", "B").as_deref(), Some(""));
        assert_eq!(response_text("<A><B/></A>", "B").as_deref(), Some(""));
        assert_eq!(response_text("<A></A>", "B"), None);
    }

    /// Escaped entities are decoded: streaming URIs routinely arrive with `&amp;`.
    #[test]
    fn test_response_text_unescapes_entities() {
        let xml = "<A><CurrentURI>x-sonosapi-stream:s1?sid=254&amp;flags=32</CurrentURI></A>";
        assert_eq!(
            response_text(xml, "CurrentURI").as_deref(),
            Some("x-sonosapi-stream:s1?sid=254&flags=32")
        );
    }

    /// CDATA is text too - devices wrap DIDL metadata this way.
    #[test]
    fn test_response_text_reads_cdata() {
        let xml = "<A><Meta><![CDATA[<DIDL-Lite/>]]></Meta></A>";
        assert_eq!(response_text(xml, "Meta").as_deref(), Some("<DIDL-Lite/>"));
    }

    /// The parse-with-default chain the macros generate.
    #[test]
    fn test_response_field_defaults_on_missing_or_unparseable() {
        assert_eq!(response_field::<u8>(ENVELOPE, "CurrentVolume"), 42);
        assert_eq!(response_field::<u8>(ENVELOPE, "Absent"), 0);
        assert_eq!(response_field::<u8>("<A><B>not-a-number</B></A>", "B"), 0);
        assert_eq!(response_field::<i8>("<A><B>-5</B></A>", "B"), -5);
    }

    #[test]
    fn test_parse_sonos_bool_accepts_sonos_and_rust_spellings() {
        assert!(parse_sonos_bool("<A><M>1</M></A>", "M"));
        assert!(parse_sonos_bool("<A><M>true</M></A>", "M"));
        assert!(parse_sonos_bool("<A><M> TRUE </M></A>", "M"));
        assert!(!parse_sonos_bool("<A><M>0</M></A>", "M"));
        assert!(!parse_sonos_bool("<A><M>false</M></A>", "M"));
        // Absent or blank is false, not an error.
        assert!(!parse_sonos_bool("<A></A>", "M"));
        assert!(!parse_sonos_bool("<A><M></M></A>", "M"));
    }

    /// Malformed XML yields `None` rather than panicking; callers then fall back
    /// to defaults exactly as they did when `xmltree` failed to build a DOM.
    #[test]
    fn test_response_text_on_malformed_xml() {
        assert_eq!(response_text("<A><B>unclosed", "B"), None);
        assert_eq!(response_text("", "B"), None);
    }

    /// `quick_xml::escape::escape` shares an internal helper with an `xs:list`
    /// variant that maps space and tab to `&#32;`/`&#9;`. Those arms must stay
    /// unreachable here: SOAP payloads carry track titles and URIs where escaped
    /// whitespace would corrupt the value the device receives.
    #[test]
    fn test_xml_escape_leaves_whitespace_verbatim() {
        assert_eq!(
            xml_escape("Bohemian Rhapsody\t(Remastered 2011)"),
            "Bohemian Rhapsody\t(Remastered 2011)"
        );
    }
}
