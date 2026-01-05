//! Group identity type

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a zone group
///
/// This is typically the group ID from ZoneGroupTopology,
/// which usually has the format "RINCON_xxxxx:n".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(String);

impl GroupId {
    /// Creates a new GroupId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for GroupId {
    fn from(s: &str) -> Self {
        GroupId::new(s)
    }
}

impl From<String> for GroupId {
    fn from(s: String) -> Self {
        GroupId::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let id = GroupId::new("RINCON_123:0");
        assert_eq!(id.as_str(), "RINCON_123:0");
    }

    #[test]
    fn test_equality() {
        let id1 = GroupId::new("group1");
        let id2 = GroupId::new("group1");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_display() {
        let id = GroupId::new("RINCON_123:0");
        assert_eq!(format!("{}", id), "RINCON_123:0");
    }
}
