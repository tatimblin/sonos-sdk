//! ZoneGroupTopology service operations and events
//!
//! This service manages the topology of zone groups in a Sonos household.
//! It tracks which speakers are grouped together, coordinator relationships,
//! and network topology information.

use crate::{define_operation_with_response, Validate};
use paste::paste;

// Get the current zone group topology
define_operation_with_response! {
    operation: GetZoneGroupStateOperation,
    action: "GetZoneGroupState",
    service: ZoneGroupTopology,
    request: {},
    response: GetZoneGroupStateResponse {
        zone_group_state: String,
    },
    xml_mapping: {
        zone_group_state: "ZoneGroupState",
    },
}

// Default Validate implementation for GetZoneGroupState operation (no parameters)
impl Validate for GetZoneGroupStateOperationRequest {
    // No validation needed for parameterless operation
}

// Convenience function
pub use get_zone_group_state_operation as get_zone_group_state;

/// Service identifier for ZoneGroupTopology
pub const SERVICE: crate::Service = crate::Service::ZoneGroupTopology;

/// Subscribe to ZoneGroupTopology events
///
/// This is a convenience function that subscribes to ZoneGroupTopology service events.
/// Events include speakers joining/leaving groups, coordinator changes, etc.
///
/// # Arguments
/// * `client` - The SonosClient to use for the subscription
/// * `ip` - The IP address of the Sonos device
/// * `callback_url` - URL where the device will send event notifications
///
/// # Returns
/// A managed subscription for ZoneGroupTopology events
pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}

/// Subscribe to ZoneGroupTopology events with custom timeout
pub fn subscribe_with_timeout(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
    timeout_seconds: u32,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe_with_timeout(ip, SERVICE, callback_url, timeout_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_zone_group_state_operation() {
        let op = get_zone_group_state_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetZoneGroupState");
    }

    #[test]
    fn test_service_constant() {
        assert_eq!(SERVICE, crate::Service::ZoneGroupTopology);
    }

    #[test]
    fn test_subscription_helpers() {
        let client = crate::SonosClient::new();

        // Test that functions exist with correct signatures
        let _subscribe_fn = || {
            subscribe(&client, "192.168.1.100", "http://callback.url")
        };

        let _subscribe_timeout_fn = || {
            subscribe_with_timeout(&client, "192.168.1.100", "http://callback.url", 3600)
        };
    }
}