//! Identity types for speakers and groups

use serde::{Deserialize, Serialize};
use std::fmt;

/// Macro to generate common ID type implementations
macro_rules! impl_id_type {
    ($name:ident) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name::new(s)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                $name::new(s)
            }
        }
    };
}

/// Unique identifier for a Sonos speaker
///
/// Typically the UUID from the UPnP device description,
/// normalized to strip the "uuid:" prefix if present.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpeakerId(String);

impl SpeakerId {
    /// Creates a new SpeakerId, normalizing the format
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        let normalized = id.strip_prefix("uuid:").unwrap_or(&id);
        Self(normalized.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_id_type!(SpeakerId);

/// Unique identifier for a zone group
///
/// Typically has the format "RINCON_xxxxx:n".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(String);

impl GroupId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl_id_type!(GroupId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speaker_id_strips_uuid_prefix() {
        let id = SpeakerId::new("uuid:RINCON_123456789");
        assert_eq!(id.as_str(), "RINCON_123456789");
    }

    #[test]
    fn test_speaker_id_without_prefix() {
        let id = SpeakerId::new("RINCON_123456789");
        assert_eq!(id.as_str(), "RINCON_123456789");
    }

    #[test]
    fn test_speaker_id_equality() {
        let id1 = SpeakerId::new("uuid:RINCON_123");
        let id2 = SpeakerId::new("RINCON_123");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_group_id() {
        let id = GroupId::new("RINCON_123:0");
        assert_eq!(id.as_str(), "RINCON_123:0");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", SpeakerId::new("RINCON_123")), "RINCON_123");
        assert_eq!(format!("{}", GroupId::new("RINCON_123:0")), "RINCON_123:0");
    }
}
