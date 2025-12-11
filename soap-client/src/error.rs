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
    
    /// SOAP fault returned by the server
    #[error("SOAP fault: error code {0}")]
    Fault(u16),
}