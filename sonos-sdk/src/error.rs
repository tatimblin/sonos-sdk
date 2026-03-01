use thiserror::Error;

/// Errors returned by the sonos-sdk API.
#[derive(Error, Debug)]
pub enum SdkError {
    /// The internal state manager encountered an error (e.g., failed to start event processing).
    #[error("State management error: {0}")]
    StateError(#[from] sonos_state::StateError),

    /// A UPnP SOAP call to the speaker failed (network error, malformed response, etc.).
    #[error("API error: {0}")]
    ApiError(#[from] sonos_api::ApiError),

    /// The event manager failed to initialize or manage UPnP subscriptions.
    #[error("Event manager error: {0}")]
    EventManager(String),

    /// No speaker with the given name or ID was found in the system.
    #[error("Speaker not found: {0}")]
    SpeakerNotFound(String),

    /// A speaker's IP address could not be parsed from the discovery response.
    #[error("Invalid IP address")]
    InvalidIpAddress,

    /// A property watcher's channel was closed unexpectedly.
    #[error("Property watcher closed")]
    WatcherClosed,

    /// A `fetch()` call failed to retrieve the property value from the speaker.
    #[error("Property fetch failed: {0}")]
    FetchFailed(String),

    /// An operation's parameters failed validation (e.g., volume > 100, bass out of range).
    #[error("Validation failed: {0}")]
    ValidationFailed(#[from] sonos_api::operation::ValidationError),

    /// The requested operation is not valid in the current state (e.g., removing a coordinator from its own group).
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}
