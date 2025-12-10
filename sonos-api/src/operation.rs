use serde::{Deserialize, Serialize};
use xmltree::Element;

use crate::error::ApiError;
use crate::service::Service;

/// Base trait for all Sonos API operations
/// 
/// This trait defines the common interface that all Sonos UPnP operations must implement.
/// It provides type safety through associated types and ensures consistent patterns
/// for request/response handling across all operations.
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
    /// * `xml` - The parsed XML element containing the response data
    /// 
    /// # Returns
    /// The typed response data or an error if parsing fails
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError>;
}