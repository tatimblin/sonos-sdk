//! UPnP service definitions and configuration

/// UPnP services available on Sonos devices
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    AVTransport,
    RenderingControl,
    GroupRenderingControl,
    ZoneGroupTopology,
    DeviceProperties,
    AlarmClock,
    MusicServices,
}

/// Service configuration information
pub struct ServiceInfo {
    pub endpoint: &'static str,
    pub service_uri: &'static str,
}

impl Service {
    /// Get service configuration information
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
}