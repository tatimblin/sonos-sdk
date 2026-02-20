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

    /// GroupManagement service - Manages speaker group membership operations
    GroupManagement,
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

/// Defines the subscription scope for UPnP services
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceScope {
    /// Per-speaker service - allows independent subscriptions on each speaker
    PerSpeaker,
    /// Per-network service - only one subscription should exist across entire network
    PerNetwork,
    /// Per-coordinator service - only run on speakers that are the coordinator for a group
    PerCoordinator,
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
            Service::GroupManagement => "GroupManagement",
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
            Service::GroupManagement => ServiceInfo {
                endpoint: "GroupManagement/Control",
                service_uri: "urn:schemas-upnp-org:service:GroupManagement:1",
                event_endpoint: "GroupManagement/Event",
            },
        }
    }

    /// Get the subscription scope for this service
    ///
    /// # Returns
    /// A `ServiceScope` indicating whether this service should have per-speaker,
    /// per-network, or per-coordinator subscriptions
    pub fn scope(&self) -> ServiceScope {
        match self {
            Service::AVTransport => ServiceScope::PerSpeaker,
            Service::RenderingControl => ServiceScope::PerSpeaker,
            Service::GroupRenderingControl => ServiceScope::PerCoordinator,
            Service::ZoneGroupTopology => ServiceScope::PerNetwork,
            Service::GroupManagement => ServiceScope::PerCoordinator,
        }
    }

}

#[cfg(test)]
mod scope_tests {
    use super::*;

    #[test]
    fn test_service_scopes() {
        assert_eq!(Service::AVTransport.scope(), ServiceScope::PerSpeaker);
        assert_eq!(Service::RenderingControl.scope(), ServiceScope::PerSpeaker);
        assert_eq!(Service::GroupRenderingControl.scope(), ServiceScope::PerCoordinator);
        assert_eq!(Service::ZoneGroupTopology.scope(), ServiceScope::PerNetwork);
        assert_eq!(Service::GroupManagement.scope(), ServiceScope::PerCoordinator);
    }

    #[test]
    fn test_all_services_have_scope() {
        // Ensure new services added to enum get scope assignments
        let services = [
            Service::AVTransport,
            Service::RenderingControl,
            Service::GroupRenderingControl,
            Service::ZoneGroupTopology,
            Service::GroupManagement,
        ];

        for service in services {
            let _scope = service.scope(); // Should not panic
        }
    }
}