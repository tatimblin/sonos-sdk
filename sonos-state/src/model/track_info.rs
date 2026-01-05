//! Track information type

use serde::{Deserialize, Serialize};

/// Information about a track (song/episode/etc)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackInfo {
    /// Track title
    pub title: Option<String>,
    /// Artist name
    pub artist: Option<String>,
    /// Album name
    pub album: Option<String>,
    /// Track duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Track URI
    pub uri: Option<String>,
    /// Album art URI
    pub album_art_uri: Option<String>,
}

impl TrackInfo {
    /// Create a new empty TrackInfo
    pub fn new() -> Self {
        Self::default()
    }

    /// Create TrackInfo with a title
    pub fn with_title(title: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Default::default()
        }
    }

    /// Check if track info has any meaningful content
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.uri.is_none()
    }
}

impl PartialEq for TrackInfo {
    fn eq(&self, other: &Self) -> bool {
        // Compare by URI first (most reliable), then by title/artist
        if self.uri.is_some() && other.uri.is_some() {
            self.uri == other.uri
        } else {
            self.title == other.title && self.artist == other.artist && self.album == other.album
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let info = TrackInfo::new();
        assert!(info.title.is_none());
        assert!(info.is_empty());
    }

    #[test]
    fn test_with_title() {
        let info = TrackInfo::with_title("Test Song");
        assert_eq!(info.title, Some("Test Song".to_string()));
        assert!(!info.is_empty());
    }

    #[test]
    fn test_equality_by_uri() {
        let info1 = TrackInfo {
            uri: Some("x-sonos://123".to_string()),
            title: Some("Title 1".to_string()),
            ..Default::default()
        };
        let info2 = TrackInfo {
            uri: Some("x-sonos://123".to_string()),
            title: Some("Title 2".to_string()),
            ..Default::default()
        };
        assert_eq!(info1, info2);
    }

    #[test]
    fn test_equality_by_title() {
        let info1 = TrackInfo {
            title: Some("Same Title".to_string()),
            artist: Some("Same Artist".to_string()),
            ..Default::default()
        };
        let info2 = TrackInfo {
            title: Some("Same Title".to_string()),
            artist: Some("Same Artist".to_string()),
            ..Default::default()
        };
        assert_eq!(info1, info2);
    }
}
