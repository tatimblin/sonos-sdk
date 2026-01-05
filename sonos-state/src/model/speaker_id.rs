//! Speaker identity type

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a Sonos speaker
///
/// This is typically the UUID from the UPnP device description,
/// normalized to strip the "uuid:" prefix if present.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpeakerId(String);

impl SpeakerId {
    /// Creates a new SpeakerId, normalizing the format
    ///
    /// Strips the "uuid:" prefix if present for consistent comparison.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let normalized = id.strip_prefix("uuid:").unwrap_or(&id);
        Self(normalized.to_string())
    }

    /// Get the ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SpeakerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for SpeakerId {
    fn from(s: &str) -> Self {
        SpeakerId::new(s)
    }
}

impl From<String> for SpeakerId {
    fn from(s: String) -> Self {
        SpeakerId::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_strips_uuid_prefix() {
        let id = SpeakerId::new("uuid:RINCON_123456789");
        assert_eq!(id.as_str(), "RINCON_123456789");
    }

    #[test]
    fn test_new_without_prefix() {
        let id = SpeakerId::new("RINCON_123456789");
        assert_eq!(id.as_str(), "RINCON_123456789");
    }

    #[test]
    fn test_equality() {
        let id1 = SpeakerId::new("uuid:RINCON_123");
        let id2 = SpeakerId::new("RINCON_123");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_display() {
        let id = SpeakerId::new("RINCON_123");
        assert_eq!(format!("{}", id), "RINCON_123");
    }
}
