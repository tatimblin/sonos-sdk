//! Error types for XML parsing operations

use thiserror::Error;

/// Errors that can occur during XML parsing operations
#[derive(Error, Debug)]
pub enum ParseError {
    /// XML deserialization failed
    #[error("XML deserialization failed: {0}")]
    XmlDeserializationFailed(String),
    
    /// Invalid XML structure
    #[error("Invalid XML structure: {0}")]
    InvalidXmlStructure(String),
    
    /// Missing required element
    #[error("Missing required element: {0}")]
    MissingRequiredElement(String),
    
    /// Namespace processing failed
    #[error("Namespace processing failed: {0}")]
    NamespaceProcessingFailed(String),
}

/// Result type alias for parsing operations
pub type ParseResult<T> = Result<T, ParseError>;