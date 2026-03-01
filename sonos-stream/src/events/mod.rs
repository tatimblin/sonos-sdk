//! Event processing and streaming
//!
//! This module handles event processing, enrichment, and provides the iterator interface
//! for consuming events. It supports both UPnP events and synthetic polling events,
//! providing transparent switching between event sources.

pub mod iterator;
pub mod processor;
pub mod types;

pub use iterator::{EventIterator, SyncEventIterator};
pub use processor::EventProcessor;
pub use types::{
    // Re-export sonos-api state types for convenience
    AVTransportState,
    DevicePropertiesEvent,
    EnrichedEvent,
    EventData,
    EventSource,
    GroupManagementState,
    GroupRenderingControlState,
    NetworkInfo,
    RenderingControlState,
    SatelliteInfo,
    // Re-export topology sub-types
    ZoneGroupInfo,
    ZoneGroupMemberInfo,
    ZoneGroupTopologyState,
};
