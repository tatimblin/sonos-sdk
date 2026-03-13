use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SdkError {
    #[error("state management error: {0}")]
    StateError(#[from] sonos_state::StateError),

    #[error("api error: {0}")]
    ApiError(#[from] sonos_api::ApiError),

    #[error("event manager error: {0}")]
    EventManager(String),

    #[error("speaker not found: {0}")]
    SpeakerNotFound(String),

    #[error("invalid ip address")]
    InvalidIpAddress,

    #[error("property watcher closed")]
    WatcherClosed,

    #[error("property fetch failed: {0}")]
    FetchFailed(String),

    #[error("validation failed: {0}")]
    ValidationFailed(#[from] sonos_api::operation::ValidationError),

    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    #[error("discovery failed: {0}")]
    DiscoveryFailed(String),

    #[error("internal lock poisoned")]
    LockPoisoned,
}
