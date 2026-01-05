//! Thread-safe state cache with change detection

use crate::{
    Group, GroupId, PlaybackState, Speaker, SpeakerId, SpeakerState, StateChange, TrackInfo,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::StateSnapshot;

/// Thread-safe cache for speaker and group state
///
/// Uses `Arc<RwLock<HashMap<...>>>` for concurrent access.
/// Update methods return `Option<StateChange>` for change detection.
pub struct StateCache {
    speakers: Arc<RwLock<HashMap<SpeakerId, SpeakerState>>>,
    groups: Arc<RwLock<HashMap<GroupId, Group>>>,
}

impl StateCache {
    /// Create a new empty state cache
    pub fn new() -> Self {
        Self {
            speakers: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize the cache with speakers and groups
    pub fn initialize(&self, speakers: Vec<Speaker>, groups: Vec<Group>) {
        // Initialize speakers
        if let Ok(mut speaker_cache) = self.speakers.write() {
            for speaker in speakers {
                let id = speaker.get_id().clone();
                speaker_cache.insert(id, SpeakerState::new(speaker));
            }
        }

        // Initialize groups
        if let Ok(mut group_cache) = self.groups.write() {
            for group in groups {
                group_cache.insert(group.get_id().clone(), group);
            }
        }

        // Update speaker group assignments
        self.update_speaker_group_assignments();
    }

    /// Update speaker group assignments based on current groups
    fn update_speaker_group_assignments(&self) {
        let groups = match self.groups.read() {
            Ok(g) => g.clone(),
            Err(_) => return,
        };

        if let Ok(mut speakers) = self.speakers.write() {
            // Clear all group assignments
            for speaker_state in speakers.values_mut() {
                speaker_state.group_id = None;
                speaker_state.is_coordinator = false;
            }

            // Set group assignments based on current groups
            for group in groups.values() {
                for member in group.get_members() {
                    if let Some(speaker_state) = speakers.get_mut(member.get_id()) {
                        speaker_state.group_id = Some(group.get_id().clone());
                        speaker_state.is_coordinator =
                            member.get_id() == group.get_coordinator_id();
                    }

                    // Also update satellite states
                    for satellite_id in member.get_satellites() {
                        if let Some(satellite_state) = speakers.get_mut(satellite_id) {
                            satellite_state.group_id = Some(group.get_id().clone());
                            satellite_state.is_coordinator = false;
                        }
                    }
                }
            }
        }
    }

    // ==================== Update methods with change detection ====================

    /// Update volume for a speaker, returning a StateChange if the value changed
    pub fn update_volume(&self, id: &SpeakerId, volume: u8) -> Option<StateChange> {
        let mut speakers = self.speakers.write().ok()?;
        let state = speakers.get_mut(id)?;
        let old_volume = state.volume;

        if old_volume != volume {
            state.volume = volume;
            Some(StateChange::VolumeChanged {
                speaker_id: id.clone(),
                old_volume,
                new_volume: volume,
            })
        } else {
            None
        }
    }

    /// Update mute state for a speaker, returning a StateChange if the value changed
    pub fn update_mute(&self, id: &SpeakerId, muted: bool) -> Option<StateChange> {
        let mut speakers = self.speakers.write().ok()?;
        let state = speakers.get_mut(id)?;

        if state.muted != muted {
            state.muted = muted;
            Some(StateChange::MuteChanged {
                speaker_id: id.clone(),
                muted,
            })
        } else {
            None
        }
    }

    /// Update playback state for a speaker, returning a StateChange if the value changed
    pub fn update_playback_state(
        &self,
        id: &SpeakerId,
        playback_state: PlaybackState,
    ) -> Option<StateChange> {
        let mut speakers = self.speakers.write().ok()?;
        let state = speakers.get_mut(id)?;
        let old_state = state.playback_state;

        if old_state != playback_state {
            state.playback_state = playback_state;
            Some(StateChange::PlaybackStateChanged {
                speaker_id: id.clone(),
                old_state,
                new_state: playback_state,
            })
        } else {
            None
        }
    }

    /// Update position for a speaker, returning a StateChange if the value changed
    pub fn update_position(
        &self,
        id: &SpeakerId,
        position_ms: u64,
        duration_ms: u64,
    ) -> Option<StateChange> {
        let mut speakers = self.speakers.write().ok()?;
        let state = speakers.get_mut(id)?;

        // Only emit change if position changed significantly (more than 1 second)
        let significant_change = (state.position_ms as i64 - position_ms as i64).unsigned_abs() > 1000
            || state.duration_ms != duration_ms;

        if significant_change {
            state.position_ms = position_ms;
            state.duration_ms = duration_ms;
            Some(StateChange::PositionChanged {
                speaker_id: id.clone(),
                position_ms,
                duration_ms,
            })
        } else {
            // Still update position even if not emitting change
            state.position_ms = position_ms;
            None
        }
    }

    /// Update current track for a speaker, returning a StateChange if the track changed
    pub fn update_track(&self, id: &SpeakerId, track: Option<TrackInfo>) -> Option<StateChange> {
        let mut speakers = self.speakers.write().ok()?;
        let state = speakers.get_mut(id)?;
        let old_track = state.current_track.clone();

        // Check if track actually changed
        let changed = match (&old_track, &track) {
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
            (Some(old), Some(new)) => old != new,
        };

        if changed {
            state.current_track = track.clone();
            Some(StateChange::TrackChanged {
                speaker_id: id.clone(),
                old_track,
                new_track: track,
            })
        } else {
            None
        }
    }

    /// Set groups, returning a StateChange with old and new groups
    pub fn set_groups(&self, groups: Vec<Group>) -> Option<StateChange> {
        let old_groups: Vec<Group> = self
            .groups
            .read()
            .ok()?
            .values()
            .cloned()
            .collect();

        {
            let mut group_cache = self.groups.write().ok()?;
            group_cache.clear();
            for group in &groups {
                group_cache.insert(group.get_id().clone(), group.clone());
            }
        }

        // Update speaker group assignments
        self.update_speaker_group_assignments();

        Some(StateChange::GroupsChanged {
            old_groups,
            new_groups: groups,
        })
    }

    /// Add a new speaker to the cache
    pub fn add_speaker(&self, speaker: Speaker) -> Option<StateChange> {
        let speaker_id = speaker.get_id().clone();
        let mut speakers = self.speakers.write().ok()?;

        if speakers.contains_key(&speaker_id) {
            return None;
        }

        speakers.insert(speaker_id.clone(), SpeakerState::new(speaker));
        Some(StateChange::SpeakerAdded { speaker_id })
    }

    /// Remove a speaker from the cache
    pub fn remove_speaker(&self, id: &SpeakerId) -> Option<StateChange> {
        let mut speakers = self.speakers.write().ok()?;

        if speakers.remove(id).is_some() {
            Some(StateChange::SpeakerRemoved {
                speaker_id: id.clone(),
            })
        } else {
            None
        }
    }

    // ==================== Query methods ====================

    /// Get a speaker by ID
    pub fn get_speaker(&self, id: &SpeakerId) -> Option<SpeakerState> {
        self.speakers.read().ok()?.get(id).cloned()
    }

    /// Get all speakers
    pub fn get_all_speakers(&self) -> Vec<SpeakerState> {
        self.speakers
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get speakers by room name
    pub fn get_by_room(&self, room_name: &str) -> Vec<SpeakerState> {
        self.speakers
            .read()
            .map(|s| {
                s.values()
                    .filter(|speaker| speaker.speaker.room_name == room_name)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get speaker by name
    pub fn get_by_name(&self, name: &str) -> Option<SpeakerState> {
        self.speakers
            .read()
            .ok()?
            .values()
            .find(|s| s.speaker.name == name)
            .cloned()
    }

    /// Get a group by ID
    pub fn get_group(&self, id: &GroupId) -> Option<Group> {
        self.groups.read().ok()?.get(id).cloned()
    }

    /// Get all groups
    pub fn get_groups(&self) -> HashMap<GroupId, Group> {
        self.groups.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Get speakers in a specific group
    pub fn get_speakers_by_group(&self, group_id: &GroupId) -> Vec<SpeakerState> {
        let groups = match self.groups.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let Some(group) = groups.get(group_id) else {
            return Vec::new();
        };

        let speakers = match self.speakers.read() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        group
            .get_members()
            .iter()
            .filter_map(|member| speakers.get(member.get_id()).cloned())
            .collect()
    }

    /// Get a snapshot of the current state
    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            speakers: self.speakers.read().map(|s| s.clone()).unwrap_or_default(),
            groups: self.groups.read().map(|g| g.clone()).unwrap_or_default(),
        }
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.speakers
            .read()
            .map(|s| s.is_empty())
            .unwrap_or(true)
    }

    /// Get total number of speakers
    pub fn speaker_count(&self) -> usize {
        self.speakers.read().map(|s| s.len()).unwrap_or(0)
    }

    /// Get total number of groups
    pub fn group_count(&self) -> usize {
        self.groups.read().map(|g| g.len()).unwrap_or(0)
    }
}

impl Clone for StateCache {
    fn clone(&self) -> Self {
        Self {
            speakers: self.speakers.clone(),
            groups: self.groups.clone(),
        }
    }
}

impl Default for StateCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn create_test_speaker(id: &str, name: &str, ip: &str) -> Speaker {
        Speaker {
            id: SpeakerId::new(id),
            name: name.to_string(),
            room_name: name.to_string(),
            ip_address: ip.parse::<IpAddr>().unwrap(),
            port: 1400,
            model_name: "Sonos One".to_string(),
            software_version: "56.0".to_string(),
            satellites: vec![],
        }
    }

    #[test]
    fn test_new_empty() {
        let cache = StateCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.speaker_count(), 0);
    }

    #[test]
    fn test_initialize() {
        let cache = StateCache::new();
        let speakers = vec![
            create_test_speaker("RINCON_1", "Living Room", "192.168.1.100"),
            create_test_speaker("RINCON_2", "Kitchen", "192.168.1.101"),
        ];
        cache.initialize(speakers, vec![]);

        assert_eq!(cache.speaker_count(), 2);
        assert!(cache.get_speaker(&SpeakerId::new("RINCON_1")).is_some());
    }

    #[test]
    fn test_update_volume_change_detection() {
        let cache = StateCache::new();
        cache.initialize(
            vec![create_test_speaker("RINCON_1", "Test", "192.168.1.100")],
            vec![],
        );

        let id = SpeakerId::new("RINCON_1");

        // First update should emit change
        let change = cache.update_volume(&id, 50);
        assert!(change.is_some());
        if let Some(StateChange::VolumeChanged {
            old_volume,
            new_volume,
            ..
        }) = change
        {
            assert_eq!(old_volume, 0);
            assert_eq!(new_volume, 50);
        }

        // Same value should not emit change
        let no_change = cache.update_volume(&id, 50);
        assert!(no_change.is_none());

        // Different value should emit change
        let change2 = cache.update_volume(&id, 75);
        assert!(change2.is_some());
    }

    #[test]
    fn test_update_playback_state_change_detection() {
        let cache = StateCache::new();
        cache.initialize(
            vec![create_test_speaker("RINCON_1", "Test", "192.168.1.100")],
            vec![],
        );

        let id = SpeakerId::new("RINCON_1");

        let change = cache.update_playback_state(&id, PlaybackState::Playing);
        assert!(change.is_some());
        if let Some(StateChange::PlaybackStateChanged {
            old_state,
            new_state,
            ..
        }) = change
        {
            assert_eq!(old_state, PlaybackState::Stopped);
            assert_eq!(new_state, PlaybackState::Playing);
        }
    }

    #[test]
    fn test_clone_shares_data() {
        let cache = StateCache::new();
        cache.initialize(
            vec![create_test_speaker("RINCON_1", "Test", "192.168.1.100")],
            vec![],
        );

        let cloned = cache.clone();

        // Update through original
        cache.update_volume(&SpeakerId::new("RINCON_1"), 50);

        // Should be visible through clone
        let speaker = cloned.get_speaker(&SpeakerId::new("RINCON_1")).unwrap();
        assert_eq!(speaker.volume, 50);
    }
}
