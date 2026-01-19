//! AVTransport service operations
//!
//! This module contains all UPnP operations for the AVTransport service,
//! which controls playback, queue management, and transport settings.

use crate::{define_operation_with_response, define_upnp_operation, Validate};
use paste::paste;

// =============================================================================
// BASIC PLAYBACK CONTROL
// =============================================================================

define_upnp_operation! {
    operation: PlayOperation,
    action: "Play",
    service: AVTransport,
    request: {
        speed: String,
    },
    response: (),
    payload: |req| {
        format!("<InstanceID>{}</InstanceID><Speed>{}</Speed>", req.instance_id, req.speed)
    },
    parse: |_xml| Ok(()),
}

impl Validate for PlayOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        if self.speed.is_empty() {
            return Err(crate::operation::ValidationError::invalid_value(
                "speed",
                &self.speed,
            ));
        }
        match self.speed.as_str() {
            "1" | "0" => Ok(()),
            other => {
                if other.parse::<f32>().is_ok() {
                    Ok(())
                } else {
                    Err(crate::operation::ValidationError::Custom {
                        parameter: "speed".to_string(),
                        message: "Speed must be '1', '0', or a numeric value".to_string(),
                    })
                }
            }
        }
    }
}

define_upnp_operation! {
    operation: PauseOperation,
    action: "Pause",
    service: AVTransport,
    request: {},
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

impl Validate for PauseOperationRequest {}

define_upnp_operation! {
    operation: StopOperation,
    action: "Stop",
    service: AVTransport,
    request: {},
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

impl Validate for StopOperationRequest {}

define_upnp_operation! {
    operation: NextOperation,
    action: "Next",
    service: AVTransport,
    request: {},
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

impl Validate for NextOperationRequest {}

define_upnp_operation! {
    operation: PreviousOperation,
    action: "Previous",
    service: AVTransport,
    request: {},
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

impl Validate for PreviousOperationRequest {}

// =============================================================================
// SEEK AND POSITION
// =============================================================================

define_upnp_operation! {
    operation: SeekOperation,
    action: "Seek",
    service: AVTransport,
    request: {
        unit: String,
        target: String,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><Unit>{}</Unit><Target>{}</Target>",
            req.instance_id, req.unit, req.target
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SeekOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.unit.as_str() {
            "TRACK_NR" | "REL_TIME" | "TIME_DELTA" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "unit".to_string(),
                message: format!(
                    "Invalid unit '{}'. Must be 'TRACK_NR', 'REL_TIME', or 'TIME_DELTA'",
                    other
                ),
            }),
        }
    }
}

define_operation_with_response! {
    operation: GetPositionInfoOperation,
    action: "GetPositionInfo",
    service: AVTransport,
    request: {},
    response: GetPositionInfoResponse {
        track: u32,
        track_duration: String,
        track_meta_data: String,
        track_uri: String,
        rel_time: String,
        abs_time: String,
        rel_count: i32,
        abs_count: i32,
    },
    xml_mapping: {
        track: "Track",
        track_duration: "TrackDuration",
        track_meta_data: "TrackMetaData",
        track_uri: "TrackURI",
        rel_time: "RelTime",
        abs_time: "AbsTime",
        rel_count: "RelCount",
        abs_count: "AbsCount",
    },
}

impl Validate for GetPositionInfoOperationRequest {}

// =============================================================================
// TRANSPORT INFO AND SETTINGS
// =============================================================================

define_operation_with_response! {
    operation: GetTransportInfoOperation,
    action: "GetTransportInfo",
    service: AVTransport,
    request: {},
    response: GetTransportInfoResponse {
        current_transport_state: String,
        current_transport_status: String,
        current_speed: String,
    },
    xml_mapping: {
        current_transport_state: "CurrentTransportState",
        current_transport_status: "CurrentTransportStatus",
        current_speed: "CurrentSpeed",
    },
}

impl Validate for GetTransportInfoOperationRequest {}

define_operation_with_response! {
    operation: GetTransportSettingsOperation,
    action: "GetTransportSettings",
    service: AVTransport,
    request: {},
    response: GetTransportSettingsResponse {
        play_mode: String,
        rec_quality_mode: String,
    },
    xml_mapping: {
        play_mode: "PlayMode",
        rec_quality_mode: "RecQualityMode",
    },
}

impl Validate for GetTransportSettingsOperationRequest {}

define_operation_with_response! {
    operation: GetCurrentTransportActionsOperation,
    action: "GetCurrentTransportActions",
    service: AVTransport,
    request: {},
    response: GetCurrentTransportActionsResponse {
        actions: String,
    },
    xml_mapping: {
        actions: "Actions",
    },
}

impl Validate for GetCurrentTransportActionsOperationRequest {}

define_operation_with_response! {
    operation: GetDeviceCapabilitiesOperation,
    action: "GetDeviceCapabilities",
    service: AVTransport,
    request: {},
    response: GetDeviceCapabilitiesResponse {
        play_media: String,
        rec_media: String,
        rec_quality_modes: String,
    },
    xml_mapping: {
        play_media: "PlayMedia",
        rec_media: "RecMedia",
        rec_quality_modes: "RecQualityModes",
    },
}

impl Validate for GetDeviceCapabilitiesOperationRequest {}

// =============================================================================
// MEDIA INFO AND URI SETTING
// =============================================================================

define_operation_with_response! {
    operation: GetMediaInfoOperation,
    action: "GetMediaInfo",
    service: AVTransport,
    request: {},
    response: GetMediaInfoResponse {
        nr_tracks: u32,
        media_duration: String,
        current_uri: String,
        current_uri_meta_data: String,
        next_uri: String,
        next_uri_meta_data: String,
        play_medium: String,
        record_medium: String,
        write_status: String,
    },
    xml_mapping: {
        nr_tracks: "NrTracks",
        media_duration: "MediaDuration",
        current_uri: "CurrentURI",
        current_uri_meta_data: "CurrentURIMetaData",
        next_uri: "NextURI",
        next_uri_meta_data: "NextURIMetaData",
        play_medium: "PlayMedium",
        record_medium: "RecordMedium",
        write_status: "WriteStatus",
    },
}

impl Validate for GetMediaInfoOperationRequest {}

define_upnp_operation! {
    operation: SetAVTransportURIOperation,
    action: "SetAVTransportURI",
    service: AVTransport,
    request: {
        current_uri: String,
        current_uri_meta_data: String,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
            req.instance_id, req.current_uri, req.current_uri_meta_data
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetAVTransportURIOperationRequest {}

define_upnp_operation! {
    operation: SetNextAVTransportURIOperation,
    action: "SetNextAVTransportURI",
    service: AVTransport,
    request: {
        next_uri: String,
        next_uri_meta_data: String,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><NextURI>{}</NextURI><NextURIMetaData>{}</NextURIMetaData>",
            req.instance_id, req.next_uri, req.next_uri_meta_data
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetNextAVTransportURIOperationRequest {}

// =============================================================================
// CROSSFADE AND PLAY MODE
// =============================================================================

define_operation_with_response! {
    operation: GetCrossfadeModeOperation,
    action: "GetCrossfadeMode",
    service: AVTransport,
    request: {},
    response: GetCrossfadeModeResponse {
        crossfade_mode: String,
    },
    xml_mapping: {
        crossfade_mode: "CrossfadeMode",
    },
}

impl Validate for GetCrossfadeModeOperationRequest {}

define_upnp_operation! {
    operation: SetCrossfadeModeOperation,
    action: "SetCrossfadeMode",
    service: AVTransport,
    request: {
        crossfade_mode: bool,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><CrossfadeMode>{}</CrossfadeMode>",
            req.instance_id,
            if req.crossfade_mode { "1" } else { "0" }
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetCrossfadeModeOperationRequest {}

define_upnp_operation! {
    operation: SetPlayModeOperation,
    action: "SetPlayMode",
    service: AVTransport,
    request: {
        new_play_mode: String,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><NewPlayMode>{}</NewPlayMode>",
            req.instance_id, req.new_play_mode
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SetPlayModeOperationRequest {
    fn validate_basic(&self) -> Result<(), crate::operation::ValidationError> {
        match self.new_play_mode.as_str() {
            "NORMAL" | "REPEAT_ALL" | "REPEAT_ONE" | "SHUFFLE_NOREPEAT" | "SHUFFLE"
            | "SHUFFLE_REPEAT_ONE" => Ok(()),
            other => Err(crate::operation::ValidationError::Custom {
                parameter: "new_play_mode".to_string(),
                message: format!(
                    "Invalid play mode '{}'. Must be NORMAL, REPEAT_ALL, REPEAT_ONE, SHUFFLE_NOREPEAT, SHUFFLE, or SHUFFLE_REPEAT_ONE",
                    other
                ),
            }),
        }
    }
}

// =============================================================================
// SLEEP TIMER
// =============================================================================

define_upnp_operation! {
    operation: ConfigureSleepTimerOperation,
    action: "ConfigureSleepTimer",
    service: AVTransport,
    request: {
        new_sleep_timer_duration: String,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><NewSleepTimerDuration>{}</NewSleepTimerDuration>",
            req.instance_id, req.new_sleep_timer_duration
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for ConfigureSleepTimerOperationRequest {}

define_operation_with_response! {
    operation: GetRemainingSleepTimerDurationOperation,
    action: "GetRemainingSleepTimerDuration",
    service: AVTransport,
    request: {},
    response: GetRemainingSleepTimerDurationResponse {
        remaining_sleep_timer_duration: String,
        current_sleep_timer_generation: u32,
    },
    xml_mapping: {
        remaining_sleep_timer_duration: "RemainingSleepTimerDuration",
        current_sleep_timer_generation: "CurrentSleepTimerGeneration",
    },
}

impl Validate for GetRemainingSleepTimerDurationOperationRequest {}

// =============================================================================
// QUEUE OPERATIONS
// =============================================================================

// AddURIToQueue - manually defined because it has a boolean parameter
// and returns a response, which the macros don't handle together
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddURIToQueueOperationRequest {
    pub instance_id: u32,
    pub enqueued_uri: String,
    pub enqueued_uri_meta_data: String,
    pub desired_first_track_number_enqueued: u32,
    pub enqueue_as_next: bool,
}

impl Validate for AddURIToQueueOperationRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AddURIToQueueResponse {
    pub first_track_number_enqueued: u32,
    pub num_tracks_added: u32,
    pub new_queue_length: u32,
}

pub struct AddURIToQueueOperation;

impl crate::operation::UPnPOperation for AddURIToQueueOperation {
    type Request = AddURIToQueueOperationRequest;
    type Response = AddURIToQueueResponse;

    const SERVICE: crate::service::Service = crate::service::Service::AVTransport;
    const ACTION: &'static str = "AddURIToQueue";

    fn build_payload(
        request: &Self::Request,
    ) -> Result<String, crate::operation::ValidationError> {
        <Self::Request as Validate>::validate(request, crate::operation::ValidationLevel::Basic)?;
        Ok(format!(
            "<InstanceID>{}</InstanceID><EnqueuedURI>{}</EnqueuedURI><EnqueuedURIMetaData>{}</EnqueuedURIMetaData><DesiredFirstTrackNumberEnqueued>{}</DesiredFirstTrackNumberEnqueued><EnqueueAsNext>{}</EnqueueAsNext>",
            request.instance_id,
            request.enqueued_uri,
            request.enqueued_uri_meta_data,
            request.desired_first_track_number_enqueued,
            if request.enqueue_as_next { "1" } else { "0" }
        ))
    }

    fn parse_response(
        xml: &xmltree::Element,
    ) -> Result<Self::Response, crate::error::ApiError> {
        Ok(AddURIToQueueResponse {
            first_track_number_enqueued: xml
                .get_child("FirstTrackNumberEnqueued")
                .and_then(|e| e.get_text())
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            num_tracks_added: xml
                .get_child("NumTracksAdded")
                .and_then(|e| e.get_text())
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            new_queue_length: xml
                .get_child("NewQueueLength")
                .and_then(|e| e.get_text())
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
        })
    }
}

pub fn add_uri_to_queue_operation(
    enqueued_uri: String,
    enqueued_uri_meta_data: String,
    desired_first_track_number_enqueued: u32,
    enqueue_as_next: bool,
) -> crate::operation::OperationBuilder<AddURIToQueueOperation> {
    let request = AddURIToQueueOperationRequest {
        instance_id: 0,
        enqueued_uri,
        enqueued_uri_meta_data,
        desired_first_track_number_enqueued,
        enqueue_as_next,
    };
    crate::operation::OperationBuilder::new(request)
}

define_upnp_operation! {
    operation: RemoveTrackFromQueueOperation,
    action: "RemoveTrackFromQueue",
    service: AVTransport,
    request: {
        object_id: String,
        update_id: u32,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><ObjectID>{}</ObjectID><UpdateID>{}</UpdateID>",
            req.instance_id, req.object_id, req.update_id
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for RemoveTrackFromQueueOperationRequest {}

define_operation_with_response! {
    operation: RemoveTrackRangeFromQueueOperation,
    action: "RemoveTrackRangeFromQueue",
    service: AVTransport,
    request: {
        update_id: u32,
        starting_index: u32,
        number_of_tracks: u32,
    },
    response: RemoveTrackRangeFromQueueResponse {
        new_update_id: u32,
    },
    xml_mapping: {
        new_update_id: "NewUpdateID",
    },
}

impl Validate for RemoveTrackRangeFromQueueOperationRequest {}

define_upnp_operation! {
    operation: RemoveAllTracksFromQueueOperation,
    action: "RemoveAllTracksFromQueue",
    service: AVTransport,
    request: {},
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

impl Validate for RemoveAllTracksFromQueueOperationRequest {}

define_operation_with_response! {
    operation: SaveQueueOperation,
    action: "SaveQueue",
    service: AVTransport,
    request: {
        title: String,
        object_id: String,
    },
    response: SaveQueueResponse {
        assigned_object_id: String,
    },
    xml_mapping: {
        assigned_object_id: "AssignedObjectID",
    },
}

impl Validate for SaveQueueOperationRequest {}

define_operation_with_response! {
    operation: CreateSavedQueueOperation,
    action: "CreateSavedQueue",
    service: AVTransport,
    request: {
        title: String,
        enqueued_uri: String,
        enqueued_uri_meta_data: String,
    },
    response: CreateSavedQueueResponse {
        num_tracks_added: u32,
        new_queue_length: u32,
        assigned_object_id: String,
        new_update_id: u32,
    },
    xml_mapping: {
        num_tracks_added: "NumTracksAdded",
        new_queue_length: "NewQueueLength",
        assigned_object_id: "AssignedObjectID",
        new_update_id: "NewUpdateID",
    },
}

impl Validate for CreateSavedQueueOperationRequest {}

define_upnp_operation! {
    operation: BackupQueueOperation,
    action: "BackupQueue",
    service: AVTransport,
    request: {},
    response: (),
    payload: |req| format!("<InstanceID>{}</InstanceID>", req.instance_id),
    parse: |_xml| Ok(()),
}

impl Validate for BackupQueueOperationRequest {}

// =============================================================================
// GROUP COORDINATION
// =============================================================================

define_operation_with_response! {
    operation: BecomeCoordinatorOfStandaloneGroupOperation,
    action: "BecomeCoordinatorOfStandaloneGroup",
    service: AVTransport,
    request: {},
    response: BecomeCoordinatorOfStandaloneGroupResponse {
        delegated_group_coordinator_id: String,
        new_group_id: String,
    },
    xml_mapping: {
        delegated_group_coordinator_id: "DelegatedGroupCoordinatorID",
        new_group_id: "NewGroupID",
    },
}

impl Validate for BecomeCoordinatorOfStandaloneGroupOperationRequest {}

define_upnp_operation! {
    operation: DelegateGroupCoordinationToOperation,
    action: "DelegateGroupCoordinationTo",
    service: AVTransport,
    request: {
        new_coordinator: String,
        rejoin_group: bool,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><NewCoordinator>{}</NewCoordinator><RejoinGroup>{}</RejoinGroup>",
            req.instance_id, req.new_coordinator, if req.rejoin_group { "true" } else { "false" }
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for DelegateGroupCoordinationToOperationRequest {}

// =============================================================================
// ALARMS
// =============================================================================

define_upnp_operation! {
    operation: SnoozeAlarmOperation,
    action: "SnoozeAlarm",
    service: AVTransport,
    request: {
        duration: String,
    },
    response: (),
    payload: |req| {
        format!(
            "<InstanceID>{}</InstanceID><Duration>{}</Duration>",
            req.instance_id, req.duration
        )
    },
    parse: |_xml| Ok(()),
}

impl Validate for SnoozeAlarmOperationRequest {}

define_operation_with_response! {
    operation: GetRunningAlarmPropertiesOperation,
    action: "GetRunningAlarmProperties",
    service: AVTransport,
    request: {},
    response: GetRunningAlarmPropertiesResponse {
        alarm_id: u32,
        group_id: String,
        logged_start_time: String,
    },
    xml_mapping: {
        alarm_id: "AlarmID",
        group_id: "GroupID",
        logged_start_time: "LoggedStartTime",
    },
}

impl Validate for GetRunningAlarmPropertiesOperationRequest {}

// =============================================================================
// LEGACY ALIASES
// =============================================================================

// Basic playback
pub use play_operation as play;
pub use pause_operation as pause;
pub use stop_operation as stop;
pub use next_operation as next;
pub use previous_operation as previous;

// Seek and position
pub use seek_operation as seek;
pub use get_position_info_operation as get_position_info;

// Transport info and settings
pub use get_transport_info_operation as get_transport_info;
pub use get_transport_settings_operation as get_transport_settings;
pub use get_current_transport_actions_operation as get_current_transport_actions;
pub use get_device_capabilities_operation as get_device_capabilities;

// Media info and URI
pub use get_media_info_operation as get_media_info;
pub use set_a_v_transport_u_r_i_operation as set_av_transport_uri;
pub use set_next_a_v_transport_u_r_i_operation as set_next_av_transport_uri;

// Crossfade and play mode
pub use get_crossfade_mode_operation as get_crossfade_mode;
pub use set_crossfade_mode_operation as set_crossfade_mode;
pub use set_play_mode_operation as set_play_mode;

// Sleep timer
pub use configure_sleep_timer_operation as configure_sleep_timer;
pub use get_remaining_sleep_timer_duration_operation as get_remaining_sleep_timer_duration;

// Queue operations
pub use add_uri_to_queue_operation as add_uri_to_queue;
pub use remove_track_from_queue_operation as remove_track_from_queue;
pub use remove_track_range_from_queue_operation as remove_track_range_from_queue;
pub use remove_all_tracks_from_queue_operation as remove_all_tracks_from_queue;
pub use save_queue_operation as save_queue;
pub use create_saved_queue_operation as create_saved_queue;
pub use backup_queue_operation as backup_queue;

// Group coordination
pub use become_coordinator_of_standalone_group_operation as become_coordinator_of_standalone_group;
pub use delegate_group_coordination_to_operation as delegate_group_coordination_to;

// Alarms
pub use snooze_alarm_operation as snooze_alarm;
pub use get_running_alarm_properties_operation as get_running_alarm_properties;

// =============================================================================
// SERVICE CONSTANT AND SUBSCRIPTION HELPERS
// =============================================================================

/// Service identifier for AVTransport
pub const SERVICE: crate::Service = crate::Service::AVTransport;

/// Subscribe to AVTransport events
pub fn subscribe(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe(ip, SERVICE, callback_url)
}

/// Subscribe to AVTransport events with custom timeout
pub fn subscribe_with_timeout(
    client: &crate::SonosClient,
    ip: &str,
    callback_url: &str,
    timeout_seconds: u32,
) -> crate::Result<crate::ManagedSubscription> {
    client.subscribe_with_timeout(ip, SERVICE, callback_url, timeout_seconds)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::UPnPOperation;

    // --- Basic Playback Tests ---

    #[test]
    fn test_play_operation_builder() {
        let op = play_operation("1".to_string()).build().unwrap();
        assert_eq!(op.request().speed, "1");
        assert_eq!(op.metadata().action, "Play");
    }

    #[test]
    fn test_play_validation() {
        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "".to_string(),
        };
        assert!(request.validate_basic().is_err());

        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "1".to_string(),
        };
        assert!(request.validate_basic().is_ok());
    }

    #[test]
    fn test_play_payload() {
        let request = PlayOperationRequest {
            instance_id: 0,
            speed: "1".to_string(),
        };
        let payload = PlayOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<InstanceID>0</InstanceID>"));
        assert!(payload.contains("<Speed>1</Speed>"));
    }

    #[test]
    fn test_pause_operation_builder() {
        let op = pause_operation().build().unwrap();
        assert_eq!(op.metadata().action, "Pause");
    }

    #[test]
    fn test_stop_operation_builder() {
        let op = stop_operation().build().unwrap();
        assert_eq!(op.metadata().action, "Stop");
    }

    #[test]
    fn test_next_operation_builder() {
        let op = next_operation().build().unwrap();
        assert_eq!(op.metadata().action, "Next");
    }

    #[test]
    fn test_previous_operation_builder() {
        let op = previous_operation().build().unwrap();
        assert_eq!(op.metadata().action, "Previous");
    }

    // --- Seek Tests ---

    #[test]
    fn test_seek_operation_builder() {
        let op = seek_operation("TRACK_NR".to_string(), "5".to_string())
            .build()
            .unwrap();
        assert_eq!(op.request().unit, "TRACK_NR");
        assert_eq!(op.request().target, "5");
        assert_eq!(op.metadata().action, "Seek");
    }

    #[test]
    fn test_seek_validation() {
        let request = SeekOperationRequest {
            instance_id: 0,
            unit: "INVALID".to_string(),
            target: "5".to_string(),
        };
        assert!(request.validate_basic().is_err());

        let request = SeekOperationRequest {
            instance_id: 0,
            unit: "REL_TIME".to_string(),
            target: "0:01:30".to_string(),
        };
        assert!(request.validate_basic().is_ok());
    }

    #[test]
    fn test_seek_payload() {
        let request = SeekOperationRequest {
            instance_id: 0,
            unit: "TRACK_NR".to_string(),
            target: "3".to_string(),
        };
        let payload = SeekOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<Unit>TRACK_NR</Unit>"));
        assert!(payload.contains("<Target>3</Target>"));
    }

    // --- Transport Info Tests ---

    #[test]
    fn test_get_transport_info_builder() {
        let op = get_transport_info_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetTransportInfo");
    }

    #[test]
    fn test_get_position_info_builder() {
        let op = get_position_info_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetPositionInfo");
    }

    #[test]
    fn test_get_media_info_builder() {
        let op = get_media_info_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetMediaInfo");
    }

    #[test]
    fn test_get_transport_settings_builder() {
        let op = get_transport_settings_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetTransportSettings");
    }

    // --- Crossfade and Play Mode Tests ---

    #[test]
    fn test_get_crossfade_mode_builder() {
        let op = get_crossfade_mode_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetCrossfadeMode");
    }

    #[test]
    fn test_set_crossfade_mode_builder() {
        let op = set_crossfade_mode_operation(true).build().unwrap();
        assert_eq!(op.request().crossfade_mode, true);
        assert_eq!(op.metadata().action, "SetCrossfadeMode");
    }

    #[test]
    fn test_set_crossfade_mode_payload() {
        let request = SetCrossfadeModeOperationRequest {
            instance_id: 0,
            crossfade_mode: true,
        };
        let payload = SetCrossfadeModeOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<CrossfadeMode>1</CrossfadeMode>"));

        let request = SetCrossfadeModeOperationRequest {
            instance_id: 0,
            crossfade_mode: false,
        };
        let payload = SetCrossfadeModeOperation::build_payload(&request).unwrap();
        assert!(payload.contains("<CrossfadeMode>0</CrossfadeMode>"));
    }

    #[test]
    fn test_set_play_mode_builder() {
        let op = set_play_mode_operation("SHUFFLE".to_string()).build().unwrap();
        assert_eq!(op.request().new_play_mode, "SHUFFLE");
        assert_eq!(op.metadata().action, "SetPlayMode");
    }

    #[test]
    fn test_set_play_mode_validation() {
        let request = SetPlayModeOperationRequest {
            instance_id: 0,
            new_play_mode: "INVALID".to_string(),
        };
        assert!(request.validate_basic().is_err());

        let request = SetPlayModeOperationRequest {
            instance_id: 0,
            new_play_mode: "REPEAT_ALL".to_string(),
        };
        assert!(request.validate_basic().is_ok());
    }

    // --- Sleep Timer Tests ---

    #[test]
    fn test_configure_sleep_timer_builder() {
        let op = configure_sleep_timer_operation("0:30:00".to_string())
            .build()
            .unwrap();
        assert_eq!(op.request().new_sleep_timer_duration, "0:30:00");
        assert_eq!(op.metadata().action, "ConfigureSleepTimer");
    }

    #[test]
    fn test_get_remaining_sleep_timer_duration_builder() {
        let op = get_remaining_sleep_timer_duration_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetRemainingSleepTimerDuration");
    }

    // --- Queue Tests ---

    #[test]
    fn test_remove_all_tracks_from_queue_builder() {
        let op = remove_all_tracks_from_queue_operation().build().unwrap();
        assert_eq!(op.metadata().action, "RemoveAllTracksFromQueue");
    }

    #[test]
    fn test_backup_queue_builder() {
        let op = backup_queue_operation().build().unwrap();
        assert_eq!(op.metadata().action, "BackupQueue");
    }

    // --- Group Coordination Tests ---

    #[test]
    fn test_become_coordinator_of_standalone_group_builder() {
        let op = become_coordinator_of_standalone_group_operation()
            .build()
            .unwrap();
        assert_eq!(op.metadata().action, "BecomeCoordinatorOfStandaloneGroup");
    }

    // --- Alarm Tests ---

    #[test]
    fn test_snooze_alarm_builder() {
        let op = snooze_alarm_operation("0:10:00".to_string()).build().unwrap();
        assert_eq!(op.request().duration, "0:10:00");
        assert_eq!(op.metadata().action, "SnoozeAlarm");
    }

    #[test]
    fn test_get_running_alarm_properties_builder() {
        let op = get_running_alarm_properties_operation().build().unwrap();
        assert_eq!(op.metadata().action, "GetRunningAlarmProperties");
    }

    // --- Service Tests ---

    #[test]
    fn test_service_constant() {
        assert_eq!(SERVICE, crate::Service::AVTransport);
    }

    #[test]
    fn test_subscription_helpers() {
        let client = crate::SonosClient::new();
        let _subscribe_fn = || subscribe(&client, "192.168.1.100", "http://callback.url");
        let _subscribe_timeout_fn =
            || subscribe_with_timeout(&client, "192.168.1.100", "http://callback.url", 3600);
        assert!(true);
    }
}
