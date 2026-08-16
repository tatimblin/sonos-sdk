//! Error types for the SOAP client

use thiserror::Error;

/// Errors that can occur during SOAP communication
#[derive(Debug, Error)]
pub enum SoapError {
    /// Network or HTTP communication error
    #[error("Network/HTTP error: {0}")]
    Network(String),

    /// XML parsing error
    #[error("XML parsing error: {0}")]
    Parse(String),

    /// SOAP fault returned by the device
    ///
    /// UPnP devices report action failures as an HTTP 500 response whose body is
    /// a `<s:Fault>` envelope. The `code` is the standardized UPnP error code
    /// (e.g. 402 = Invalid Args, 701 = Transition Not Available) and
    /// `description` is the device's human-readable reason, when it supplies one.
    #[error("SOAP fault: error code {code}{}", .description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default())]
    Fault {
        /// UPnP error code from `<errorCode>`
        code: u16,
        /// Device-supplied reason from `<errorDescription>`, if present
        description: Option<String>,
    },
}

impl SoapError {
    /// Construct a fault with no device-supplied description.
    ///
    /// Convenience for tests and for callers that only have a code.
    pub fn fault(code: u16) -> Self {
        Self::Fault {
            code,
            description: None,
        }
    }
}
