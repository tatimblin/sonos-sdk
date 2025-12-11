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
    
    /// DeviceProperties service - Provides device information and properties
    DeviceProperties,
    
    /// AlarmClock service - Manages alarms and sleep timers
    AlarmClock,
    
    /// MusicServices service - Manages music service configurations
    MusicServices,
}

/// Contains the endpoint and service URI information for a UPnP service
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInfo {
    /// The HTTP endpoint path for this service (relative to device base URL)
    pub endpoint: &'static str,
    
    /// The UPnP service URI used in SOAP requests
    pub service_uri: &'static str,
}

impl Service {
    /// Get the service information (endpoint and URI) for this service
    /// 
    /// # Returns
    /// A `ServiceInfo` struct containing the endpoint path and service URI
    pub fn info(&self) -> ServiceInfo {
        match self {
            Service::AVTransport => ServiceInfo {
                endpoint: "MediaRenderer/AVTransport/Control",
                service_uri: "urn:schemas-upnp-org:service:AVTransport:1",
            },
            Service::RenderingControl => ServiceInfo {
                endpoint: "MediaRenderer/RenderingControl/Control",
                service_uri: "urn:schemas-upnp-org:service:RenderingControl:1",
            },
            Service::GroupRenderingControl => ServiceInfo {
                endpoint: "MediaRenderer/GroupRenderingControl/Control",
                service_uri: "urn:schemas-upnp-org:service:GroupRenderingControl:1",
            },
            Service::ZoneGroupTopology => ServiceInfo {
                endpoint: "ZoneGroupTopology/Control",
                service_uri: "urn:schemas-upnp-org:service:ZoneGroupTopology:1",
            },
            Service::DeviceProperties => ServiceInfo {
                endpoint: "DeviceProperties/Control",
                service_uri: "urn:schemas-upnp-org:service:DeviceProperties:1",
            },
            Service::AlarmClock => ServiceInfo {
                endpoint: "AlarmClock/Control",
                service_uri: "urn:schemas-upnp-org:service:AlarmClock:1",
            },
            Service::MusicServices => ServiceInfo {
                endpoint: "MusicServices/Control",
                service_uri: "urn:schemas-upnp-org:service:MusicServices:1",
            },
        }
    }
    
    /// Get the endpoint path for this service
    /// 
    /// # Returns
    /// The HTTP endpoint path as a string slice
    pub fn endpoint(&self) -> &'static str {
        self.info().endpoint
    }
    
    /// Get the service URI for this service
    /// 
    /// # Returns
    /// The UPnP service URI as a string slice
    pub fn service_uri(&self) -> &'static str {
        self.info().service_uri
    }
}