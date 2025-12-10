//! Base operation trait for Sonos API operations

use serde::{Deserialize, Serialize};
use xmltree::Element;
use crate::{ApiError, Service};

/// Base trait that all Sonos API operations must implement
pub trait SonosOperation {
    /// Request type for this operation
    type Request: Serialize;
    
    /// Response type for this operation
    type Response: for<'de> Deserialize<'de>;
    
    /// The UPnP service this operation belongs to
    const SERVICE: Service;
    
    /// The SOAP action name for this operation
    const ACTION: &'static str;
    
    /// Build the SOAP payload for the request
    fn build_payload(request: &Self::Request) -> String;
    
    /// Parse the XML response into the response type
    fn parse_response(xml: &Element) -> Result<Self::Response, ApiError>;
}