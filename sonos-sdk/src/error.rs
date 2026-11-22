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

impl SdkError {
    /// Whether a speaker could not be reached: a connection failure, timeout
    /// or unresolvable address, as opposed to a speaker that answered with an
    /// error.
    ///
    /// The underlying `ApiError::NetworkError` can arrive directly or wrapped
    /// in a state error, and `sonos-api` is not re-exported from this crate,
    /// so this saves callers from matching either path or the message text.
    pub fn is_network_error(&self) -> bool {
        matches!(
            self,
            Self::ApiError(sonos_api::ApiError::NetworkError(_))
                | Self::StateError(sonos_state::StateError::Api(
                    sonos_api::ApiError::NetworkError(_)
                ))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_api::ApiError;

    #[test]
    fn test_is_network_error() {
        let network = || ApiError::NetworkError("connection refused".to_string());
        assert!(SdkError::ApiError(network()).is_network_error());
        assert!(SdkError::StateError(sonos_state::StateError::Api(network())).is_network_error());

        assert!(!SdkError::ApiError(ApiError::SoapFault(701)).is_network_error());
        assert!(!SdkError::DiscoveryFailed("no speakers".to_string()).is_network_error());
    }
}
