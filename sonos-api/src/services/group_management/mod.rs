//! GroupManagement service for speaker group membership operations
//!
//! This service handles group membership operations (add/remove members, track buffering)
//! for Sonos speaker groups. Operations should only be sent to the group coordinator.
//!
//! # Control Operations
//! ```rust,ignore
//! use sonos_api::services::group_management;
//!
//! let add_op = group_management::add_member("RINCON_123".to_string(), 1).build()?;
//! client.execute("192.168.1.100", add_op)?;
//! ```
//!
//! # Event Subscriptions
//! ```rust,ignore
//! let subscription = group_management::subscribe(&client, "192.168.1.100", "http://callback")?;
//! ```
//!
//! # Event Handling
//! ```rust,ignore
//! use sonos_api::services::group_management::events::{GroupManagementEventParser, create_enriched_event};
//! use sonos_api::events::EventSource;
//!
//! let parser = GroupManagementEventParser;
//! let event_data = parser.parse_upnp_event(xml_content)?;
//! let enriched = create_enriched_event(speaker_ip, event_source, event_data);
//! ```
//!
//! # Important Notes
//! - Operations should only be sent to the group coordinator

pub mod operations;
pub mod events;

// Re-export operations for convenience
pub use operations::*;

// Re-export event types and parsers
pub use events::{
    GroupManagementEvent, GroupManagementEventParser, 
    create_enriched_event, create_enriched_event_with_registration_id
};

/// Service constant for GroupManagement
pub const SERVICE: crate::Service = crate::Service::GroupManagement;

/// Subscribe to GroupManagement events
pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}

/// Subscribe to GroupManagement events with custom timeout
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
    fn test_module_service_constant() {
        assert_eq!(SERVICE, crate::Service::GroupManagement);
    }

    #[test]
    fn test_subscribe_function_exists() {
        // Verify subscribe function signature compiles correctly
        // This is a compile-time check - the function exists with correct types
        let _: fn(&crate::SonosClient, &str, &str) -> crate::Result<crate::ManagedSubscription> = subscribe;
    }

    #[test]
    fn test_subscribe_with_timeout_function_exists() {
        // Verify subscribe_with_timeout function signature compiles correctly
        let _: fn(&crate::SonosClient, &str, &str, u32) -> crate::Result<crate::ManagedSubscription> = subscribe_with_timeout;
    }
}
