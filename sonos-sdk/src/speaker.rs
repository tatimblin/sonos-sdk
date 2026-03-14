//! Speaker handle with property accessors
//!
//! Provides a DOM-like interface for accessing speaker properties.
//!
//! ## Write Operations and State Cache
//!
//! Write methods (e.g., `play()`, `set_volume()`) update the state cache
//! optimistically after the SOAP call succeeds. This means `speaker.volume.get()`
//! reflects the written value immediately. However, if the speaker rejects the
//! command silently, the cache may be stale until the next UPnP event corrects it.
//! Use `speaker.volume.watch()` for authoritative real-time state.

use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::SonosClient;
use sonos_discovery::Device;
use sonos_state::{Bass, Loudness, Mute, PlaybackState, SpeakerId, StateManager, Treble, Volume};

use crate::Group;

use sonos_api::operation::{ComposableOperation, UPnPOperation, ValidationError};
use sonos_api::services::{
    av_transport::{
        self, AddURIToQueueResponse, BecomeCoordinatorOfStandaloneGroupResponse,
        CreateSavedQueueResponse, GetCrossfadeModeResponse, GetCurrentTransportActionsResponse,
        GetDeviceCapabilitiesResponse, GetMediaInfoResponse,
        GetRemainingSleepTimerDurationResponse, GetRunningAlarmPropertiesResponse,
        GetTransportSettingsResponse, RemoveTrackRangeFromQueueResponse, SaveQueueResponse,
    },
    rendering_control::{self, SetRelativeVolumeResponse},
};

use crate::SdkError;

/// Seek target for the `seek()` method
///
/// Combines the seek unit and target value into a single type-safe enum,
/// preventing mismatched unit/target combinations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeekTarget {
    /// Seek to a track number (1-based)
    Track(u32),
    /// Seek to an absolute time position (e.g., `"0:02:30"`)
    Time(String),
    /// Seek by a time delta (e.g., `"+0:00:30"` or `"-0:00:10"`)
    Delta(String),
}

impl SeekTarget {
    /// Returns the UPnP seek unit string
    fn unit(&self) -> &str {
        match self {
            SeekTarget::Track(_) => "TRACK_NR",
            SeekTarget::Time(_) => "REL_TIME",
            SeekTarget::Delta(_) => "TIME_DELTA",
        }
    }

    /// Returns the UPnP seek target string
    fn target(&self) -> String {
        match self {
            SeekTarget::Track(n) => n.to_string(),
            SeekTarget::Time(t) => t.clone(),
            SeekTarget::Delta(d) => d.clone(),
        }
    }
}

/// Play mode for the `set_play_mode()` method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    /// Normal sequential playback
    Normal,
    /// Repeat all tracks
    RepeatAll,
    /// Repeat current track
    RepeatOne,
    /// Shuffle without repeat
    ShuffleNoRepeat,
    /// Shuffle with repeat
    Shuffle,
    /// Shuffle and repeat current track
    ShuffleRepeatOne,
}

impl std::fmt::Display for PlayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayMode::Normal => write!(f, "NORMAL"),
            PlayMode::RepeatAll => write!(f, "REPEAT_ALL"),
            PlayMode::RepeatOne => write!(f, "REPEAT_ONE"),
            PlayMode::ShuffleNoRepeat => write!(f, "SHUFFLE_NOREPEAT"),
            PlayMode::Shuffle => write!(f, "SHUFFLE"),
            PlayMode::ShuffleRepeatOne => write!(f, "SHUFFLE_REPEAT_ONE"),
        }
    }
}

use crate::property::{
    BassHandle, CurrentTrackHandle, EventInitFn, GroupMembershipHandle, LoudnessHandle, MuteHandle,
    PlaybackStateHandle, PositionHandle, PropertyHandle, SpeakerContext, TrebleHandle,
    VolumeHandle,
};

/// Speaker handle with property access
///
/// Provides direct access to speaker properties through property handles.
/// Each property handle provides `get()`, `fetch()`, `watch()`, and `unwatch()` methods.
///
/// # Example
///
/// ```rust,ignore
/// // Get cached volume
/// let volume = speaker.volume.get();
///
/// // Fetch fresh volume from device
/// let fresh_volume = speaker.volume.fetch()?;
///
/// // Watch for volume changes
/// speaker.volume.watch()?;
///
/// // Stop watching
/// speaker.volume.unwatch();
/// ```
#[derive(Clone)]
pub struct Speaker {
    /// Unique speaker identifier
    pub id: SpeakerId,
    /// Friendly name of the speaker
    pub name: String,
    /// IP address of the speaker
    pub ip: IpAddr,
    /// Model name of the speaker (e.g., "Sonos One", "Sonos Beam")
    pub model_name: String,

    // ========================================================================
    // RenderingControl properties
    // ========================================================================
    /// Volume property (0-100)
    pub volume: VolumeHandle,
    /// Mute state (true = muted)
    pub mute: MuteHandle,
    /// Bass EQ setting (-10 to +10)
    pub bass: BassHandle,
    /// Treble EQ setting (-10 to +10)
    pub treble: TrebleHandle,
    /// Loudness compensation setting
    pub loudness: LoudnessHandle,

    // ========================================================================
    // AVTransport properties
    // ========================================================================
    /// Playback state (Playing/Paused/Stopped/Transitioning)
    pub playback_state: PlaybackStateHandle,
    /// Current playback position and duration
    pub position: PositionHandle,
    /// Current track information (title, artist, album, etc.)
    pub current_track: CurrentTrackHandle,

    // ========================================================================
    // ZoneGroupTopology properties
    // ========================================================================
    /// Group membership information (group_id, is_coordinator)
    pub group_membership: GroupMembershipHandle,

    // Internal context shared with property handles
    context: Arc<SpeakerContext>,
}

impl Speaker {
    /// Create a Speaker from a discovered Device
    ///
    /// This is the preferred way to create a Speaker when you have a Device
    /// from discovery. It handles IP address parsing and extracts all relevant
    /// fields from the Device struct.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let devices = sonos_discovery::get();
    /// for device in devices {
    ///     let speaker = Speaker::from_device(&device, state_manager.clone(), api_client.clone())?;
    ///     println!("Created speaker: {}", speaker.name);
    /// }
    /// ```
    pub fn from_device(
        device: &Device,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Result<Self, SdkError> {
        let ip: IpAddr = device
            .ip_address
            .parse()
            .map_err(|_| SdkError::InvalidIpAddress)?;

        let name = if device.room_name.is_empty() || device.room_name == "Unknown" {
            device.name.clone()
        } else {
            device.room_name.clone()
        };

        Ok(Self::new(
            SpeakerId::new(&device.id),
            name,
            ip,
            device.model_name.clone(),
            state_manager,
            api_client,
        ))
    }

    /// Create a new Speaker handle
    ///
    /// For most use cases, prefer [`Speaker::from_device()`] which handles
    /// IP parsing and extracts fields from a Device struct.
    pub fn new(
        id: SpeakerId,
        name: String,
        ip: IpAddr,
        model_name: String,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
    ) -> Self {
        Self::new_with_event_init(id, name, ip, model_name, state_manager, api_client, None)
    }

    /// Create a new Speaker handle with an optional event init closure
    ///
    /// When `event_init` is provided, calling `watch()` on any property will
    /// trigger lazy event manager initialization on first use.
    pub(crate) fn new_with_event_init(
        id: SpeakerId,
        name: String,
        ip: IpAddr,
        model_name: String,
        state_manager: Arc<StateManager>,
        api_client: SonosClient,
        event_init: Option<EventInitFn>,
    ) -> Self {
        let context = match event_init {
            Some(init) => {
                SpeakerContext::with_event_init(id.clone(), ip, state_manager, api_client, init)
            }
            None => SpeakerContext::new(id.clone(), ip, state_manager, api_client),
        };

        Self {
            id,
            name,
            ip,
            model_name,
            // RenderingControl properties
            volume: PropertyHandle::new(Arc::clone(&context)),
            mute: PropertyHandle::new(Arc::clone(&context)),
            bass: PropertyHandle::new(Arc::clone(&context)),
            treble: PropertyHandle::new(Arc::clone(&context)),
            loudness: PropertyHandle::new(Arc::clone(&context)),
            // AVTransport properties
            playback_state: PropertyHandle::new(Arc::clone(&context)),
            position: PropertyHandle::new(Arc::clone(&context)),
            current_track: PropertyHandle::new(Arc::clone(&context)),
            // ZoneGroupTopology properties
            group_membership: PropertyHandle::new(Arc::clone(&context)),
            // Internal
            context,
        }
    }

    // ========================================================================
    // Navigation
    // ========================================================================

    /// Get the group this speaker belongs to (sync, no network call)
    ///
    /// Reads from the state store's topology data. Returns `None` if
    /// topology has not been loaded yet.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let kitchen = sonos.speaker("Kitchen").unwrap();
    /// if let Some(group) = kitchen.group() {
    ///     println!("Kitchen is in group {}", group.id);
    /// }
    /// ```
    pub fn group(&self) -> Option<Group> {
        let info = self
            .context
            .state_manager
            .get_group_for_speaker(&self.context.speaker_id)?;
        Group::from_info(
            info,
            Arc::clone(&self.context.state_manager),
            self.context.api_client.clone(),
        )
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Execute a UPnP operation against this speaker
    fn exec<Op: UPnPOperation>(
        &self,
        operation: Result<ComposableOperation<Op>, ValidationError>,
    ) -> Result<Op::Response, SdkError> {
        let op = operation?;
        self.context
            .api_client
            .execute_enhanced(&self.context.speaker_ip.to_string(), op)
            .map_err(SdkError::ApiError)
    }

    // ========================================================================
    // AVTransport — Basic playback
    // ========================================================================

    /// Start or resume playback
    ///
    /// Updates the state cache to `PlaybackState::Playing` on success.
    pub fn play(&self) -> Result<(), SdkError> {
        self.exec(av_transport::play("1".to_string()).build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, PlaybackState::Playing);
        Ok(())
    }

    /// Pause playback
    ///
    /// Updates the state cache to `PlaybackState::Paused` on success.
    pub fn pause(&self) -> Result<(), SdkError> {
        self.exec(av_transport::pause().build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, PlaybackState::Paused);
        Ok(())
    }

    /// Stop playback
    ///
    /// Updates the state cache to `PlaybackState::Stopped` on success.
    pub fn stop(&self) -> Result<(), SdkError> {
        self.exec(av_transport::stop().build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, PlaybackState::Stopped);
        Ok(())
    }

    /// Skip to next track
    pub fn next(&self) -> Result<(), SdkError> {
        self.exec(av_transport::next().build())?;
        Ok(())
    }

    /// Skip to previous track
    pub fn previous(&self) -> Result<(), SdkError> {
        self.exec(av_transport::previous().build())?;
        Ok(())
    }

    // ========================================================================
    // AVTransport — Seek
    // ========================================================================

    /// Seek to a position
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// speaker.seek(SeekTarget::Time("0:02:30".into()))?;  // Seek to 2:30
    /// speaker.seek(SeekTarget::Track(3))?;                 // Seek to track 3
    /// speaker.seek(SeekTarget::Delta("+0:00:30".into()))?; // Skip forward 30s
    /// ```
    pub fn seek(&self, target: SeekTarget) -> Result<(), SdkError> {
        self.exec(av_transport::seek(target.unit().to_string(), target.target()).build())?;
        Ok(())
    }

    // ========================================================================
    // AVTransport — URI setting
    // ========================================================================

    /// Set the current transport URI
    pub fn set_av_transport_uri(&self, uri: &str, metadata: &str) -> Result<(), SdkError> {
        self.exec(
            av_transport::set_av_transport_uri(uri.to_string(), metadata.to_string()).build(),
        )?;
        Ok(())
    }

    /// Set the next transport URI (for gapless playback)
    pub fn set_next_av_transport_uri(&self, uri: &str, metadata: &str) -> Result<(), SdkError> {
        self.exec(
            av_transport::set_next_av_transport_uri(uri.to_string(), metadata.to_string()).build(),
        )?;
        Ok(())
    }

    // ========================================================================
    // AVTransport — Info queries
    // ========================================================================

    /// Get media info (number of tracks, duration, URI, etc.)
    pub fn get_media_info(&self) -> Result<GetMediaInfoResponse, SdkError> {
        self.exec(av_transport::get_media_info().build())
    }

    /// Get transport settings (play mode, recording quality)
    pub fn get_transport_settings(&self) -> Result<GetTransportSettingsResponse, SdkError> {
        self.exec(av_transport::get_transport_settings().build())
    }

    /// Get currently available transport actions
    pub fn get_current_transport_actions(
        &self,
    ) -> Result<GetCurrentTransportActionsResponse, SdkError> {
        self.exec(av_transport::get_current_transport_actions().build())
    }

    // ========================================================================
    // AVTransport — Play mode / crossfade
    // ========================================================================

    /// Set play mode
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// speaker.set_play_mode(PlayMode::Shuffle)?;
    /// speaker.set_play_mode(PlayMode::RepeatAll)?;
    /// ```
    pub fn set_play_mode(&self, mode: PlayMode) -> Result<(), SdkError> {
        self.exec(av_transport::set_play_mode(mode.to_string()).build())?;
        Ok(())
    }

    /// Get crossfade mode
    pub fn get_crossfade_mode(&self) -> Result<GetCrossfadeModeResponse, SdkError> {
        self.exec(av_transport::get_crossfade_mode().build())
    }

    /// Set crossfade mode
    pub fn set_crossfade_mode(&self, enabled: bool) -> Result<(), SdkError> {
        self.exec(av_transport::set_crossfade_mode(enabled).build())?;
        Ok(())
    }

    // ========================================================================
    // AVTransport — Sleep timer
    // ========================================================================

    /// Configure sleep timer (e.g., `"01:00:00"` for 1 hour, `""` to cancel)
    pub fn configure_sleep_timer(&self, duration: &str) -> Result<(), SdkError> {
        self.exec(av_transport::configure_sleep_timer(duration.to_string()).build())?;
        Ok(())
    }

    /// Cancel an active sleep timer
    pub fn cancel_sleep_timer(&self) -> Result<(), SdkError> {
        self.configure_sleep_timer("")
    }

    /// Get remaining sleep timer duration
    pub fn get_remaining_sleep_timer(
        &self,
    ) -> Result<GetRemainingSleepTimerDurationResponse, SdkError> {
        self.exec(av_transport::get_remaining_sleep_timer_duration().build())
    }

    // ========================================================================
    // AVTransport — Queue operations
    // ========================================================================

    /// Add a URI to the queue
    pub fn add_uri_to_queue(
        &self,
        uri: &str,
        metadata: &str,
        position: u32,
        enqueue_as_next: bool,
    ) -> Result<AddURIToQueueResponse, SdkError> {
        self.exec(
            av_transport::add_uri_to_queue(
                uri.to_string(),
                metadata.to_string(),
                position,
                enqueue_as_next,
            )
            .build(),
        )
    }

    /// Remove a track from the queue
    pub fn remove_track_from_queue(&self, object_id: &str, update_id: u32) -> Result<(), SdkError> {
        self.exec(av_transport::remove_track_from_queue(object_id.to_string(), update_id).build())?;
        Ok(())
    }

    /// Remove all tracks from the queue
    pub fn remove_all_tracks_from_queue(&self) -> Result<(), SdkError> {
        self.exec(av_transport::remove_all_tracks_from_queue().build())?;
        Ok(())
    }

    /// Save the current queue as a Sonos playlist
    pub fn save_queue(&self, title: &str, object_id: &str) -> Result<SaveQueueResponse, SdkError> {
        self.exec(av_transport::save_queue(title.to_string(), object_id.to_string()).build())
    }

    /// Create a new saved queue (playlist) with a URI
    pub fn create_saved_queue(
        &self,
        title: &str,
        uri: &str,
        metadata: &str,
    ) -> Result<CreateSavedQueueResponse, SdkError> {
        self.exec(
            av_transport::create_saved_queue(
                title.to_string(),
                uri.to_string(),
                metadata.to_string(),
            )
            .build(),
        )
    }

    /// Remove a range of tracks from the queue
    pub fn remove_track_range_from_queue(
        &self,
        update_id: u32,
        starting_index: u32,
        number_of_tracks: u32,
    ) -> Result<RemoveTrackRangeFromQueueResponse, SdkError> {
        self.exec(
            av_transport::remove_track_range_from_queue(
                update_id,
                starting_index,
                number_of_tracks,
            )
            .build(),
        )
    }

    /// Backup the current queue
    pub fn backup_queue(&self) -> Result<(), SdkError> {
        self.exec(av_transport::backup_queue().build())?;
        Ok(())
    }

    // ========================================================================
    // AVTransport — Device capabilities
    // ========================================================================

    /// Get device capabilities (supported media formats)
    pub fn get_device_capabilities(&self) -> Result<GetDeviceCapabilitiesResponse, SdkError> {
        self.exec(av_transport::get_device_capabilities().build())
    }

    // ========================================================================
    // AVTransport — Alarm operations
    // ========================================================================

    /// Snooze the currently running alarm
    pub fn snooze_alarm(&self, duration: &str) -> Result<(), SdkError> {
        self.exec(av_transport::snooze_alarm(duration.to_string()).build())?;
        Ok(())
    }

    /// Get properties of the currently running alarm
    pub fn get_running_alarm_properties(
        &self,
    ) -> Result<GetRunningAlarmPropertiesResponse, SdkError> {
        self.exec(av_transport::get_running_alarm_properties().build())
    }

    // ========================================================================
    // AVTransport — Group coordination
    // ========================================================================

    /// Leave current group and become a standalone player
    pub fn become_standalone(
        &self,
    ) -> Result<BecomeCoordinatorOfStandaloneGroupResponse, SdkError> {
        self.exec(av_transport::become_coordinator_of_standalone_group().build())
    }

    /// Delegate group coordination to another speaker
    pub fn delegate_coordination_to(
        &self,
        new_coordinator: &SpeakerId,
        rejoin_group: bool,
    ) -> Result<(), SdkError> {
        self.exec(
            av_transport::delegate_group_coordination_to(
                new_coordinator.as_str().to_string(),
                rejoin_group,
            )
            .build(),
        )?;
        Ok(())
    }

    /// Join a group (convenience wrapper for `group.add_speaker(self)`)
    ///
    /// Adds this speaker to the specified group. After calling this,
    /// re-fetch groups via `system.groups()` to see updated membership.
    pub fn join_group(&self, group: &Group) -> Result<(), SdkError> {
        group.add_speaker(self)
    }

    /// Leave current group and become a standalone player
    ///
    /// Semantic alias for [`become_standalone()`](Self::become_standalone).
    /// After calling this, the speaker forms its own group of one.
    pub fn leave_group(&self) -> Result<BecomeCoordinatorOfStandaloneGroupResponse, SdkError> {
        self.become_standalone()
    }

    // ========================================================================
    // RenderingControl — Volume and EQ
    // ========================================================================

    /// Set speaker volume (0-100)
    ///
    /// Updates the state cache to the new `Volume` on success.
    pub fn set_volume(&self, volume: u8) -> Result<(), SdkError> {
        self.exec(rendering_control::set_volume("Master".to_string(), volume).build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, Volume(volume));
        Ok(())
    }

    /// Adjust volume relative to current level
    ///
    /// Returns the new absolute volume.
    pub fn set_relative_volume(
        &self,
        adjustment: i8,
    ) -> Result<SetRelativeVolumeResponse, SdkError> {
        let response = self.exec(
            rendering_control::set_relative_volume("Master".to_string(), adjustment).build(),
        )?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, Volume(response.new_volume));
        Ok(response)
    }

    /// Set mute state
    ///
    /// Updates the state cache to the new `Mute` value on success.
    pub fn set_mute(&self, muted: bool) -> Result<(), SdkError> {
        self.exec(rendering_control::set_mute("Master".to_string(), muted).build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, Mute(muted));
        Ok(())
    }

    /// Set bass EQ level (-10 to +10)
    pub fn set_bass(&self, level: i8) -> Result<(), SdkError> {
        self.exec(rendering_control::set_bass(level).build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, Bass(level));
        Ok(())
    }

    /// Set treble EQ level (-10 to +10)
    pub fn set_treble(&self, level: i8) -> Result<(), SdkError> {
        self.exec(rendering_control::set_treble(level).build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, Treble(level));
        Ok(())
    }

    /// Set loudness compensation
    pub fn set_loudness(&self, enabled: bool) -> Result<(), SdkError> {
        self.exec(rendering_control::set_loudness("Master".to_string(), enabled).build())?;
        self.context
            .state_manager
            .set_property(&self.context.speaker_id, Loudness(enabled));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonos_discovery::Device;

    fn create_test_speaker() -> Speaker {
        let manager = StateManager::new().unwrap();
        let devices = vec![Device {
            id: "RINCON_TEST123".to_string(),
            name: "Test Speaker".to_string(),
            room_name: "Test Room".to_string(),
            ip_address: "192.168.1.100".to_string(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        manager.add_devices(devices).unwrap();
        let state_manager = Arc::new(manager);
        let api_client = SonosClient::new();

        Speaker::new(
            SpeakerId::new("RINCON_TEST123"),
            "Test Speaker".to_string(),
            "192.168.1.100".parse().unwrap(),
            "Sonos One".to_string(),
            state_manager,
            api_client,
        )
    }

    #[test]
    fn test_set_volume_rejects_invalid() {
        let speaker = create_test_speaker();
        let result = speaker.set_volume(150);
        assert!(matches!(result, Err(SdkError::ValidationFailed(_))));
    }

    #[test]
    fn test_set_bass_rejects_invalid() {
        let speaker = create_test_speaker();
        let result = speaker.set_bass(15);
        assert!(matches!(result, Err(SdkError::ValidationFailed(_))));
    }

    #[test]
    fn test_set_treble_rejects_invalid() {
        let speaker = create_test_speaker();
        let result = speaker.set_treble(-15);
        assert!(matches!(result, Err(SdkError::ValidationFailed(_))));
    }

    #[test]
    fn test_speaker_action_methods_exist() {
        // Compile-time assertion that all method signatures are correct
        fn assert_void(_r: Result<(), SdkError>) {}
        fn assert_response<T>(_r: Result<T, SdkError>) {}

        let speaker = create_test_speaker();

        // AVTransport — these will fail at network level but prove signatures compile
        assert_void(speaker.play());
        assert_void(speaker.pause());
        assert_void(speaker.stop());
        assert_void(speaker.next());
        assert_void(speaker.previous());
        assert_void(speaker.seek(SeekTarget::Time("0:00:00".into())));
        assert_void(speaker.set_av_transport_uri("", ""));
        assert_void(speaker.set_next_av_transport_uri("", ""));
        assert_response::<GetMediaInfoResponse>(speaker.get_media_info());
        assert_response::<GetTransportSettingsResponse>(speaker.get_transport_settings());
        assert_response::<GetCurrentTransportActionsResponse>(
            speaker.get_current_transport_actions(),
        );
        assert_void(speaker.set_play_mode(PlayMode::Normal));
        assert_response::<GetCrossfadeModeResponse>(speaker.get_crossfade_mode());
        assert_void(speaker.set_crossfade_mode(true));
        assert_void(speaker.configure_sleep_timer(""));
        assert_void(speaker.cancel_sleep_timer());
        assert_response::<GetRemainingSleepTimerDurationResponse>(
            speaker.get_remaining_sleep_timer(),
        );
        assert_response::<AddURIToQueueResponse>(speaker.add_uri_to_queue("", "", 0, false));
        assert_void(speaker.remove_track_from_queue("", 0));
        assert_void(speaker.remove_all_tracks_from_queue());
        assert_response::<SaveQueueResponse>(speaker.save_queue("", ""));
        assert_response::<CreateSavedQueueResponse>(speaker.create_saved_queue("", "", ""));
        assert_response::<RemoveTrackRangeFromQueueResponse>(
            speaker.remove_track_range_from_queue(0, 0, 1),
        );
        assert_void(speaker.backup_queue());
        assert_response::<GetDeviceCapabilitiesResponse>(speaker.get_device_capabilities());
        assert_void(speaker.snooze_alarm("00:10:00"));
        assert_response::<GetRunningAlarmPropertiesResponse>(
            speaker.get_running_alarm_properties(),
        );
        assert_response::<BecomeCoordinatorOfStandaloneGroupResponse>(speaker.become_standalone());
        assert_void(speaker.delegate_coordination_to(&SpeakerId::new("RINCON_OTHER"), false));

        // RenderingControl
        assert_void(speaker.set_volume(50));
        assert_response::<SetRelativeVolumeResponse>(speaker.set_relative_volume(5));
        assert_void(speaker.set_mute(true));
        assert_void(speaker.set_bass(0));
        assert_void(speaker.set_treble(0));
        assert_void(speaker.set_loudness(true));

        // Group convenience methods
        let group = create_test_group_for_speaker(&speaker);
        assert_void(speaker.join_group(&group));
        assert_response::<BecomeCoordinatorOfStandaloneGroupResponse>(speaker.leave_group());
    }

    fn create_test_group_for_speaker(speaker: &Speaker) -> crate::Group {
        use sonos_state::{GroupId, GroupInfo};
        let state_manager = Arc::new(StateManager::new().unwrap());
        let devices = vec![Device {
            id: speaker.id.as_str().to_string(),
            name: speaker.name.clone(),
            room_name: speaker.name.clone(),
            ip_address: speaker.ip.to_string(),
            port: 1400,
            model_name: speaker.model_name.clone(),
        }];
        state_manager.add_devices(devices).unwrap();

        let group_info = GroupInfo::new(
            GroupId::new(format!("{}:1", speaker.id.as_str())),
            speaker.id.clone(),
            vec![speaker.id.clone()],
        );

        crate::Group::from_info(group_info, state_manager, SonosClient::new()).unwrap()
    }
}
