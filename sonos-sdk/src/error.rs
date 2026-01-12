use thiserror::Error;

#[derive(Error, Debug)]
pub enum SdkError {
    #[error("State management error: {0}")]
    StateError(#[from] sonos_state::StateError),

    #[error("API error: {0}")]
    ApiError(#[from] sonos_api::ApiError),

    #[error("Speaker not found: {0}")]
    SpeakerNotFound(String),

    #[error("Invalid IP address")]
    InvalidIpAddress,

    #[error("Property watcher closed")]
    WatcherClosed,
}