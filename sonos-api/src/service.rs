/// Represents the different UPnP services exposed by Sonos devices
/// 
/// Each service provides a specific set of operations for controlling different
/// aspects of the Sonos device functionality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    /// AVTransport service - Controls playback (play, pause, stop, seek, etc.)
    AVTransport,
    
    /// RenderingControl service - Controls audio rendering (volume, mute, etc.)
    RenderingControl,
    
    /// GroupRenderingControl service - Controls group-wide audio settings
    GroupRenderingControl,

    /// ZoneGroupTopology service - Manages speaker grouping and topology
    ZoneGroupTopology,
}

/// Contains the endpoint and service URI information for a UPnP service
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInfo {
    /// The HTTP endpoint path for this service (relative to device base URL)
    pub endpoint: &'static str,
    
    /// The UPnP service URI used in SOAP requests
    pub service_uri: &'static str,
    
    /// The HTTP event endpoint path for UPnP event subscriptions
    pub event_endpoint: &'static str,
}

impl Service {
    /// Get the name of this service as a string
    ///
    /// # Returns
    /// The service name as a string slice
    pub fn name(&self) -> &'static str {
        match self {
            Service::AVTransport => "AVTransport",
            Service::RenderingControl => "RenderingControl",
            Service::GroupRenderingControl => "GroupRenderingControl",
            Service::ZoneGroupTopology => "ZoneGroupTopology",
        }
    }

    /// Get the service information (endpoint and URI) for this service
    ///
    /// # Returns
    /// A `ServiceInfo` struct containing the endpoint path, service URI, and event endpoint
    pub fn info(&self) -> ServiceInfo {
        match self {
            Service::AVTransport => ServiceInfo {
                endpoint: "MediaRenderer/AVTransport/Control",
                service_uri: "urn:schemas-upnp-org:service:AVTransport:1",
                event_endpoint: "MediaRenderer/AVTransport/Event",
            },
            Service::RenderingControl => ServiceInfo {
                endpoint: "MediaRenderer/RenderingControl/Control",
                service_uri: "urn:schemas-upnp-org:service:RenderingControl:1",
                event_endpoint: "MediaRenderer/RenderingControl/Event",
            },
            Service::GroupRenderingControl => ServiceInfo {
                endpoint: "MediaRenderer/GroupRenderingControl/Control",
                service_uri: "urn:schemas-upnp-org:service:GroupRenderingControl:1",
                event_endpoint: "MediaRenderer/GroupRenderingControl/Event",
            },
            Service::ZoneGroupTopology => ServiceInfo {
                endpoint: "ZoneGroupTopology/Control",
                service_uri: "urn:schemas-upnp-org:service:ZoneGroupTopology:1",
                event_endpoint: "ZoneGroupTopology/Event",
            },
        }
    }
    
}