//! Event decoders for different UPnP services
//!
//! Each decoder handles events from a specific service type and converts
//! them into property updates.

mod rendering;
mod topology;
mod transport;

pub use rendering::RenderingControlDecoder;
pub use topology::TopologyDecoder;
pub use transport::AVTransportDecoder;

use crate::decoder::EventDecoder;

/// Create the default set of decoders for processing Sonos events
pub fn default_decoders() -> Vec<Box<dyn EventDecoder>> {
    vec![
        Box::new(RenderingControlDecoder),
        Box::new(AVTransportDecoder),
        Box::new(TopologyDecoder),
    ]
}
