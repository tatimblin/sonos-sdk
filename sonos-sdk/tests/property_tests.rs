//! Property-based tests for the Sonos SDK
//!
//! These tests validate correctness properties using property-based testing.

use proptest::prelude::*;
use std::net::IpAddr;
use std::sync::Arc;

use sonos_api::SonosClient;
use sonos_discovery::Device;
use sonos_sdk::property::SpeakerContext;
use sonos_sdk::PropertyHandle;
use sonos_state::{Property, SpeakerId, StateManager, Volume};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test StateManager with a speaker
fn create_test_state_manager(
    speaker_id: impl Into<String>,
    ip: impl Into<String>,
) -> Arc<StateManager> {
    let speaker_id = speaker_id.into();
    let ip = ip.into();
    let manager = StateManager::new().unwrap();
    let devices = vec![Device {
        id: speaker_id.clone(),
        name: format!("Test Speaker {}", speaker_id),
        room_name: "Test Room".to_string(),
        ip_address: ip,
        port: 1400,
        model_name: "Sonos One".to_string(),
    }];
    manager.add_devices(devices).unwrap();
    Arc::new(manager)
}

/// Strategy for generating valid speaker IDs
fn speaker_id_strategy() -> impl Strategy<Value = String> {
    // Generate RINCON-style IDs like real Sonos devices
    "[A-Z0-9]{12,20}".prop_map(|s| format!("RINCON_{}", s))
}

/// Strategy for generating valid IP addresses
fn ip_strategy() -> impl Strategy<Value = String> {
    (1u8..255, 0u8..255, 0u8..255, 1u8..255).prop_map(|(a, b, c, d)| format!("{}.{}.{}.{}", a, b, c, d))
}

/// Strategy for generating volume values (0-100)
fn volume_strategy() -> impl Strategy<Value = u8> {
    0u8..=100
}

/// Create a test SpeakerContext
fn create_test_context(
    speaker_id: impl AsRef<str>,
    ip: impl AsRef<str>,
    state_manager: Arc<StateManager>,
) -> Arc<SpeakerContext> {
    let speaker_id_obj = SpeakerId::new(speaker_id.as_ref());
    let speaker_ip: IpAddr = ip.as_ref().parse().unwrap();
    let api_client = SonosClient::new();
    SpeakerContext::new(speaker_id_obj, speaker_ip, state_manager, api_client)
}

// ============================================================================
// Property 3: Watch Registers Property
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* speaker and any property type, after calling `property_handle.watch()`,
    /// the property SHALL be marked as watched (i.e., `property_handle.is_watched()` returns true).
    #[test]
    fn prop_watch_registers_property(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let context = create_test_context(&speaker_id, &ip, state_manager);

        let handle: PropertyHandle<Volume> = PropertyHandle::new(context);

        // Initially not watched
        prop_assert!(!handle.is_watched(), "Property should not be watched initially");

        // After watch(), should be watched
        handle.watch().unwrap();
        prop_assert!(handle.is_watched(), "Property should be watched after watch() is called");
    }
}

// ============================================================================
// Property 4: Unwatch Unregisters Property
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* speaker and any property type that is currently watched,
    /// after calling `property_handle.unwatch()`, the property SHALL no longer
    /// be marked as watched (i.e., `property_handle.is_watched()` returns false).
    #[test]
    fn prop_unwatch_unregisters_property(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let context = create_test_context(&speaker_id, &ip, state_manager);

        let handle: PropertyHandle<Volume> = PropertyHandle::new(context);

        // Watch first
        handle.watch().unwrap();
        prop_assert!(handle.is_watched(), "Property should be watched after watch()");

        // After unwatch(), should not be watched
        handle.unwatch();
        prop_assert!(!handle.is_watched(), "Property should not be watched after unwatch()");
    }

    /// *For any* speaker and property, multiple watch/unwatch cycles should
    /// correctly toggle the watched state.
    #[test]
    fn prop_watch_unwatch_round_trip(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        cycles in 1usize..5,
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let context = create_test_context(&speaker_id, &ip, state_manager);

        let handle: PropertyHandle<Volume> = PropertyHandle::new(context);

        // Initially not watched
        prop_assert!(!handle.is_watched());

        for i in 0..cycles {
            // Watch
            handle.watch().unwrap();
            prop_assert!(handle.is_watched(), "Cycle {}: should be watched after watch()", i);

            // Unwatch
            handle.unwatch();
            prop_assert!(!handle.is_watched(), "Cycle {}: should not be watched after unwatch()", i);
        }
    }
}


// ============================================================================
// Property 2: Fetch Updates Cache
// ============================================================================

// Note: Property 2 tests that after fetch() succeeds, get() returns the same value.
// Since fetch() requires a real network call, we test the underlying mechanism:
// the state manager's set_property() which fetch() uses to update the cache.
// This validates the core property that cache updates work correctly.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* speaker and any property type, after calling `property_handle.fetch()`
    /// successfully, a subsequent call to `property_handle.get()` SHALL return the
    /// same value that `fetch()` returned.
    ///
    /// This test validates the cache update mechanism that fetch() relies on.
    /// We test that set_property() (called by fetch() internally) correctly updates
    /// the cache so that get() returns the updated value.
    #[test]
    fn prop_fetch_updates_cache_mechanism(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        volume_value in 0u8..=100,
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);
        let context = create_test_context(&speaker_id, &ip, Arc::clone(&state_manager));

        let handle: PropertyHandle<Volume> = PropertyHandle::new(context);

        // Initially no value cached
        prop_assert!(handle.get().is_none(), "Cache should be empty initially");

        // Simulate what fetch() does internally: update the cache via state_manager
        let fetched_value = Volume::new(volume_value);
        state_manager.set_property(&speaker_id_obj, fetched_value.clone());

        // After cache update, get() should return the same value
        let cached_value = handle.get();
        prop_assert!(cached_value.is_some(), "Cache should have a value after update");
        prop_assert_eq!(
            cached_value.unwrap(),
            fetched_value,
            "get() should return the same value that was set (simulating fetch())"
        );
    }

    /// *For any* sequence of fetch operations, the cache should always reflect
    /// the most recent fetched value.
    #[test]
    fn prop_multiple_fetch_updates_cache(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        volume_values in proptest::collection::vec(0u8..=100, 1..5),
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);
        let context = create_test_context(&speaker_id, &ip, Arc::clone(&state_manager));

        let handle: PropertyHandle<Volume> = PropertyHandle::new(context);

        // Simulate multiple fetch operations
        for volume_value in volume_values {
            let fetched_value = Volume::new(volume_value);
            state_manager.set_property(&speaker_id_obj, fetched_value.clone());

            // After each update, get() should return the latest value
            let cached_value = handle.get();
            prop_assert!(cached_value.is_some());
            prop_assert_eq!(
                cached_value.unwrap(),
                fetched_value,
                "Cache should always reflect the most recent fetched value"
            );
        }
    }
}


// ============================================================================
// Property 5: Speaker Clone Equivalence
// ============================================================================

use sonos_sdk::Speaker;
use sonos_state::{Bass, Loudness, Mute, PlaybackState, Treble};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* Speaker handle, cloning it SHALL produce a handle that provides
    /// access to the same property values - i.e., for any property P,
    /// `original.P.get() == clone.P.get()`.
    #[test]
    fn prop_speaker_clone_equivalence(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        volume_value in volume_strategy(),
        mute_value in proptest::bool::ANY,
        bass_value in -10i8..=10,
        treble_value in -10i8..=10,
        loudness_value in proptest::bool::ANY,
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);
        let speaker_ip: IpAddr = ip.parse().unwrap();
        let api_client = SonosClient::new();

        // Create a speaker
        let speaker = Speaker::new(
            speaker_id_obj.clone(),
            format!("Test Speaker {}", speaker_id),
            speaker_ip,
            "Sonos One".to_string(),
            Arc::clone(&state_manager),
            api_client,
        );

        // Set various property values in the state manager
        state_manager.set_property(&speaker_id_obj, Volume::new(volume_value));
        state_manager.set_property(&speaker_id_obj, Mute::new(mute_value));
        state_manager.set_property(&speaker_id_obj, Bass::new(bass_value));
        state_manager.set_property(&speaker_id_obj, Treble::new(treble_value));
        state_manager.set_property(&speaker_id_obj, Loudness::new(loudness_value));
        state_manager.set_property(&speaker_id_obj, PlaybackState::Stopped);

        // Clone the speaker
        let cloned = speaker.clone();

        // Verify all property handles return the same values
        prop_assert_eq!(
            speaker.volume.get(),
            cloned.volume.get(),
            "Volume should be equal after clone"
        );
        prop_assert_eq!(
            speaker.mute.get(),
            cloned.mute.get(),
            "Mute should be equal after clone"
        );
        prop_assert_eq!(
            speaker.bass.get(),
            cloned.bass.get(),
            "Bass should be equal after clone"
        );
        prop_assert_eq!(
            speaker.treble.get(),
            cloned.treble.get(),
            "Treble should be equal after clone"
        );
        prop_assert_eq!(
            speaker.loudness.get(),
            cloned.loudness.get(),
            "Loudness should be equal after clone"
        );
        prop_assert_eq!(
            speaker.playback_state.get(),
            cloned.playback_state.get(),
            "PlaybackState should be equal after clone"
        );

        // Verify metadata is also equal
        prop_assert_eq!(speaker.id, cloned.id, "Speaker ID should be equal after clone");
        prop_assert_eq!(speaker.name, cloned.name, "Speaker name should be equal after clone");
        prop_assert_eq!(speaker.ip, cloned.ip, "Speaker IP should be equal after clone");
        prop_assert_eq!(speaker.model_name, cloned.model_name, "Speaker model_name should be equal after clone");
    }


    /// *For any* Speaker handle, after cloning, changes to the underlying state
    /// should be visible through both the original and cloned handles.
    #[test]
    fn prop_speaker_clone_shares_state(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        initial_volume in volume_strategy(),
        updated_volume in volume_strategy(),
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);
        let speaker_ip: IpAddr = ip.parse().unwrap();
        let api_client = SonosClient::new();

        // Create a speaker and set initial volume
        let speaker = Speaker::new(
            speaker_id_obj.clone(),
            format!("Test Speaker {}", speaker_id),
            speaker_ip,
            "Sonos One".to_string(),
            Arc::clone(&state_manager),
            api_client,
        );
        state_manager.set_property(&speaker_id_obj, Volume::new(initial_volume));

        // Clone the speaker
        let cloned = speaker.clone();

        // Both should see the initial value
        prop_assert_eq!(speaker.volume.get(), cloned.volume.get());

        // Update the state through the state manager (simulating an event update)
        state_manager.set_property(&speaker_id_obj, Volume::new(updated_volume));

        // Both original and clone should see the updated value
        prop_assert_eq!(
            speaker.volume.get(),
            Some(Volume::new(updated_volume)),
            "Original should see updated value"
        );
        prop_assert_eq!(
            cloned.volume.get(),
            Some(Volume::new(updated_volume)),
            "Clone should see updated value"
        );
        prop_assert_eq!(
            speaker.volume.get(),
            cloned.volume.get(),
            "Both should see the same updated value"
        );
    }
}


// ============================================================================
// Watched Property Changes Emit Events
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* property that has been watched, when its value changes via `set_property()`,
    /// a ChangeEvent SHALL be emitted to the iterator.
    #[test]
    fn prop_watched_property_changes_emit_events(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        initial_volume in volume_strategy(),
        updated_volume in volume_strategy(),
    ) {
        // Skip if volumes are the same (no change = no event)
        prop_assume!(initial_volume != updated_volume);

        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);

        // Set initial value
        state_manager.set_property(&speaker_id_obj, Volume::new(initial_volume));

        // Register watch for the property
        state_manager.register_watch(&speaker_id_obj, Volume::KEY);

        // Change the property value
        state_manager.set_property(&speaker_id_obj, Volume::new(updated_volume));

        // Get the iterator and check for the event
        let iter = state_manager.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));

        prop_assert!(
            event.is_some(),
            "A ChangeEvent should be emitted when a watched property changes"
        );
    }
}

// ============================================================================
// Change Event Contains Correct Data
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* ChangeEvent emitted when a watched property changes, the event's
    /// `speaker_id` SHALL match the speaker whose property changed, and the event's
    /// `property_key` SHALL match the property's KEY constant.
    #[test]
    fn prop_change_event_contains_correct_data(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        initial_volume in volume_strategy(),
        updated_volume in volume_strategy(),
    ) {
        // Skip if volumes are the same (no change = no event)
        prop_assume!(initial_volume != updated_volume);

        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);

        // Set initial value
        state_manager.set_property(&speaker_id_obj, Volume::new(initial_volume));

        // Register watch for the property
        state_manager.register_watch(&speaker_id_obj, Volume::KEY);

        // Change the property value
        state_manager.set_property(&speaker_id_obj, Volume::new(updated_volume));

        // Get the iterator and check the event data
        let iter = state_manager.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(100));

        prop_assert!(event.is_some(), "Event should be emitted");
        let event = event.unwrap();

        // Verify speaker_id matches
        prop_assert_eq!(
            event.speaker_id.as_str(),
            speaker_id_obj.as_str(),
            "ChangeEvent speaker_id should match the speaker whose property changed"
        );

        // Verify property_key matches
        prop_assert_eq!(
            event.property_key,
            Volume::KEY,
            "ChangeEvent property_key should match the property's KEY constant"
        );

        // Verify service matches
        prop_assert_eq!(
            event.service,
            sonos_api::Service::RenderingControl,
            "ChangeEvent service should match the property's SERVICE constant"
        );
    }

    /// *For any* set of watched properties on a speaker, when each changes,
    /// the emitted events should contain the correct speaker_id and property_key.
    #[test]
    fn prop_multiple_properties_emit_correct_events(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        volume_value in volume_strategy(),
        mute_value in proptest::bool::ANY,
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);

        // Register watches for multiple properties
        state_manager.register_watch(&speaker_id_obj, Volume::KEY);
        state_manager.register_watch(&speaker_id_obj, Mute::KEY);

        // Change volume
        state_manager.set_property(&speaker_id_obj, Volume::new(volume_value));

        // Change mute
        state_manager.set_property(&speaker_id_obj, Mute::new(mute_value));

        // Get the iterator and collect events
        let iter = state_manager.iter();
        
        // First event should be for volume
        let volume_event = iter.recv_timeout(std::time::Duration::from_millis(100));
        prop_assert!(volume_event.is_some(), "Volume event should be emitted");
        let volume_event = volume_event.unwrap();
        prop_assert_eq!(volume_event.speaker_id.as_str(), speaker_id_obj.as_str());
        prop_assert_eq!(volume_event.property_key, Volume::KEY);

        // Second event should be for mute
        let mute_event = iter.recv_timeout(std::time::Duration::from_millis(100));
        prop_assert!(mute_event.is_some(), "Mute event should be emitted");
        let mute_event = mute_event.unwrap();
        prop_assert_eq!(mute_event.speaker_id.as_str(), speaker_id_obj.as_str());
        prop_assert_eq!(mute_event.property_key, Mute::KEY);
    }
}


// ============================================================================
// Unwatched Properties Do Not Emit Events
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* property that has NOT been watched (or has been unwatched),
    /// when its value changes via `set_property()`, NO ChangeEvent SHALL be
    /// emitted to the iterator.
    #[test]
    fn prop_unwatched_properties_do_not_emit_events(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        initial_volume in volume_strategy(),
        updated_volume in volume_strategy(),
    ) {
        // Skip if volumes are the same (no change anyway)
        prop_assume!(initial_volume != updated_volume);

        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);

        // Set initial value (NOT watched)
        state_manager.set_property(&speaker_id_obj, Volume::new(initial_volume));

        // Change the property value WITHOUT watching it
        state_manager.set_property(&speaker_id_obj, Volume::new(updated_volume));

        // Get the iterator and check that NO event was emitted
        let iter = state_manager.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(50));

        prop_assert!(
            event.is_none(),
            "No ChangeEvent should be emitted when an unwatched property changes"
        );
    }

    /// *For any* property that was watched and then unwatched, when its value
    /// changes via `set_property()`, NO ChangeEvent SHALL be emitted.
    #[test]
    fn prop_unwatched_after_unwatch_does_not_emit(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        initial_volume in volume_strategy(),
        updated_volume in volume_strategy(),
    ) {
        // Skip if volumes are the same
        prop_assume!(initial_volume != updated_volume);

        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);

        // Set initial value
        state_manager.set_property(&speaker_id_obj, Volume::new(initial_volume));

        // Watch the property
        state_manager.register_watch(&speaker_id_obj, Volume::KEY);

        // Unwatch the property
        state_manager.unregister_watch(&speaker_id_obj, Volume::KEY);

        // Change the property value (now unwatched)
        state_manager.set_property(&speaker_id_obj, Volume::new(updated_volume));

        // Get the iterator and check that NO event was emitted
        let iter = state_manager.iter();
        let event = iter.recv_timeout(std::time::Duration::from_millis(50));

        prop_assert!(
            event.is_none(),
            "No ChangeEvent should be emitted after property is unwatched"
        );
    }

    /// *For any* set of properties where only some are watched, only the watched
    /// properties should emit events when changed.
    #[test]
    fn prop_only_watched_properties_emit(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
        volume_value in volume_strategy(),
        mute_value in proptest::bool::ANY,
    ) {
        let state_manager = create_test_state_manager(&speaker_id, &ip);
        let speaker_id_obj = SpeakerId::new(&speaker_id);

        // Only watch volume, NOT mute
        state_manager.register_watch(&speaker_id_obj, Volume::KEY);

        // Change both properties
        state_manager.set_property(&speaker_id_obj, Volume::new(volume_value));
        state_manager.set_property(&speaker_id_obj, Mute::new(mute_value));

        // Get the iterator
        let iter = state_manager.iter();

        // Should get exactly one event (for volume)
        let first_event = iter.recv_timeout(std::time::Duration::from_millis(100));
        prop_assert!(first_event.is_some(), "Volume event should be emitted");
        let first_event = first_event.unwrap();
        prop_assert_eq!(first_event.property_key, Volume::KEY, "First event should be for volume");

        // Should NOT get a second event (mute is not watched)
        let second_event = iter.recv_timeout(std::time::Duration::from_millis(50));
        prop_assert!(
            second_event.is_none(),
            "No event should be emitted for unwatched mute property"
        );
    }
}


// ============================================================================
// Property 6: Get Speaker By Name Round-Trip
// ============================================================================

use sonos_sdk::SonosSystem;

/// Strategy for generating valid speaker names
fn speaker_name_strategy() -> impl Strategy<Value = String> {
    "[A-Za-z ]{3,20}".prop_map(|s| s.trim().to_string())
        .prop_filter("Name must not be empty", |s| !s.is_empty())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* speaker added to the system, calling `system.get_speaker_by_name(speaker.name)`
    /// SHALL return a speaker with the same ID as the original.
    #[test]
    fn prop_get_speaker_by_name_round_trip(
        speaker_id in speaker_id_strategy(),
        speaker_name in speaker_name_strategy(),
        ip in ip_strategy(),
    ) {
        let devices = vec![Device {
            id: speaker_id.clone(),
            name: speaker_name.clone(),
            room_name: "Test Room".to_string(),
            ip_address: ip.clone(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = SonosSystem::from_discovered_devices(devices).unwrap();

        // Look up by name
        let found_speaker = system.get_speaker_by_name(&speaker_name);

        prop_assert!(
            found_speaker.is_some(),
            "Speaker should be found by name '{}'", speaker_name
        );

        let found_speaker = found_speaker.unwrap();
        let expected_id = SpeakerId::new(&speaker_id);

        prop_assert_eq!(
            found_speaker.id,
            expected_id,
            "Found speaker ID should match the original speaker ID"
        );
    }


    /// *For any* name that was not added to the system, `get_speaker_by_name()` SHALL return None.
    #[test]
    fn prop_get_speaker_by_name_returns_none_for_unknown(
        speaker_id in speaker_id_strategy(),
        speaker_name in speaker_name_strategy(),
        unknown_name in speaker_name_strategy(),
        ip in ip_strategy(),
    ) {
        // Ensure the unknown name is different from the actual speaker name
        prop_assume!(speaker_name != unknown_name);

        let devices = vec![Device {
            id: speaker_id.clone(),
            name: speaker_name.clone(),
            room_name: "Test Room".to_string(),
            ip_address: ip.clone(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = SonosSystem::from_discovered_devices(devices).unwrap();

        // Look up by unknown name
        let found_speaker = system.get_speaker_by_name(&unknown_name);

        prop_assert!(
            found_speaker.is_none(),
            "Speaker should NOT be found for unknown name '{}' (actual name: '{}')",
            unknown_name, speaker_name
        );
    }
}

// ============================================================================
// Property 7: Get Speaker By ID Round-Trip
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* speaker added to the system, calling `system.get_speaker_by_id(&speaker.id)`
    /// SHALL return a speaker with the same name as the original.
    #[test]
    fn prop_get_speaker_by_id_round_trip(
        speaker_id in speaker_id_strategy(),
        speaker_name in speaker_name_strategy(),
        ip in ip_strategy(),
    ) {
        let devices = vec![Device {
            id: speaker_id.clone(),
            name: speaker_name.clone(),
            room_name: "Test Room".to_string(),
            ip_address: ip.clone(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = SonosSystem::from_discovered_devices(devices).unwrap();

        // Look up by ID
        let lookup_id = SpeakerId::new(&speaker_id);
        let found_speaker = system.get_speaker_by_id(&lookup_id);

        prop_assert!(
            found_speaker.is_some(),
            "Speaker should be found by ID '{}'", speaker_id
        );

        let found_speaker = found_speaker.unwrap();

        prop_assert_eq!(
            found_speaker.name,
            speaker_name,
            "Found speaker name should match the original speaker name"
        );
    }


    /// *For any* ID that was not added to the system, `get_speaker_by_id()` SHALL return None.
    #[test]
    fn prop_get_speaker_by_id_returns_none_for_unknown(
        speaker_id in speaker_id_strategy(),
        unknown_id in speaker_id_strategy(),
        speaker_name in speaker_name_strategy(),
        ip in ip_strategy(),
    ) {
        // Ensure the unknown ID is different from the actual speaker ID
        prop_assume!(speaker_id != unknown_id);

        let devices = vec![Device {
            id: speaker_id.clone(),
            name: speaker_name.clone(),
            room_name: "Test Room".to_string(),
            ip_address: ip.clone(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];

        let system = SonosSystem::from_discovered_devices(devices).unwrap();

        // Look up by unknown ID
        let lookup_id = SpeakerId::new(&unknown_id);
        let found_speaker = system.get_speaker_by_id(&lookup_id);

        prop_assert!(
            found_speaker.is_none(),
            "Speaker should NOT be found for unknown ID '{}' (actual ID: '{}')",
            unknown_id, speaker_id
        );
    }
}

// ============================================================================
// Property 8: Speakers Count Consistency
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* set of devices added to the system, `system.speakers().len()` SHALL equal
    /// the number of unique devices added.
    #[test]
    fn prop_speakers_count_consistency(
        device_count in 1usize..5,
    ) {
        // Generate unique devices
        let devices: Vec<Device> = (0..device_count)
            .map(|i| Device {
                id: format!("RINCON_{:012}", i),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: format!("192.168.1.{}", 100 + i),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();

        let expected_count = devices.len();
        let system = SonosSystem::from_discovered_devices(devices).unwrap();

        let actual_count = system.speakers().len();

        prop_assert_eq!(
            actual_count,
            expected_count,
            "speakers().len() should equal the number of devices added"
        );
    }


    /// *For any* empty device list, `system.speakers().len()` SHALL equal 0.
    #[test]
    fn prop_empty_system_has_no_speakers(_dummy in Just(())) {
        let devices: Vec<Device> = vec![];
        let system = SonosSystem::from_discovered_devices(devices).unwrap();

        let count = system.speakers().len();

        prop_assert_eq!(
            count,
            0,
            "Empty system should have 0 speakers"
        );
    }


    /// *For any* set of devices added to the system, every device should be accessible
    /// via both `get_speaker_by_name()` and `get_speaker_by_id()`.
    #[test]
    fn prop_all_speakers_accessible(
        device_count in 1usize..5,
    ) {
        // Generate unique devices
        let devices: Vec<Device> = (0..device_count)
            .map(|i| Device {
                id: format!("RINCON_{:012}", i),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: format!("192.168.1.{}", 100 + i),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();

        let system = SonosSystem::from_discovered_devices(devices.clone()).unwrap();

        // Verify each device is accessible by both name and ID
        for device in &devices {
            let by_name = system.get_speaker_by_name(&device.name);
            prop_assert!(
                by_name.is_some(),
                "Device '{}' should be accessible by name", device.name
            );

            let speaker_id = SpeakerId::new(&device.id);
            let by_id = system.get_speaker_by_id(&speaker_id);
            prop_assert!(
                by_id.is_some(),
                "Device '{}' should be accessible by ID", device.id
            );

            // Both lookups should return the same speaker
            let by_name = by_name.unwrap();
            let by_id = by_id.unwrap();
            prop_assert_eq!(
                by_name.id,
                by_id.id,
                "Lookup by name and ID should return the same speaker"
            );
            prop_assert_eq!(
                by_name.name,
                by_id.name,
                "Lookup by name and ID should return the same speaker"
            );
        }
    }
}


// ============================================================================
// Property 3 (speaker-groups): Group Access Consistency
// ============================================================================

use sonos_state::{GroupId, GroupInfo};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* speaker in the system:
    /// - get_group_for_speaker(speaker_id) returns a Group containing that speaker
    /// - The returned Group's member_ids contains the speaker_id
    /// - get_group_by_id(group.id) returns the same Group
    #[test]
    fn prop_group_access_consistency(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
    ) {
        let state_manager = StateManager::new().unwrap();
        let speaker_id_obj = SpeakerId::new(&speaker_id);
        
        // Add a device
        let devices = vec![Device {
            id: speaker_id.clone(),
            name: format!("Test Speaker {}", speaker_id),
            room_name: "Test Room".to_string(),
            ip_address: ip.clone(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        state_manager.add_devices(devices).unwrap();
        
        // Create a group containing this speaker
        let group_id = GroupId::new(format!("{}:1", speaker_id));
        let group_info = GroupInfo::new(
            group_id.clone(),
            speaker_id_obj.clone(),
            vec![speaker_id_obj.clone()],
        );
        
        // Add the group to the state manager via initialize
        let topology = sonos_state::Topology::new(
            state_manager.speaker_infos(),
            vec![group_info.clone()],
        );
        state_manager.initialize(topology);
        
        // Test 1: get_group_for_speaker returns a group containing the speaker
        let found_group = state_manager.get_group_for_speaker(&speaker_id_obj);
        prop_assert!(
            found_group.is_some(),
            "get_group_for_speaker should return a group for speaker '{}'", speaker_id
        );
        
        let found_group = found_group.unwrap();
        
        // Test 2: The returned group's member_ids contains the speaker_id
        prop_assert!(
            found_group.member_ids.contains(&speaker_id_obj),
            "Group member_ids should contain the speaker_id"
        );
        
        // Test 3: get_group_by_id returns the same group
        let group_by_id = state_manager.get_group(&found_group.id);
        prop_assert!(
            group_by_id.is_some(),
            "get_group should return a group for id '{}'", found_group.id
        );
        
        let group_by_id = group_by_id.unwrap();
        prop_assert_eq!(
            found_group,
            group_by_id,
            "get_group_for_speaker and get_group should return the same group"
        );
    }


    /// *For any* group with multiple speakers, all speakers should be able to look up
    /// the same group via get_group_for_speaker.
    #[test]
    fn prop_group_access_consistency_multiple_speakers(
        speaker_count in 2usize..5,
    ) {
        let state_manager = StateManager::new().unwrap();
        
        // Generate unique speakers
        let speakers: Vec<(SpeakerId, String)> = (0..speaker_count)
            .map(|i| {
                let id = format!("RINCON_{:012}", i);
                let ip = format!("192.168.1.{}", 100 + i);
                (SpeakerId::new(&id), ip)
            })
            .collect();
        
        // Add devices
        let devices: Vec<Device> = speakers.iter().enumerate()
            .map(|(i, (id, ip))| Device {
                id: id.as_str().to_string(),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: ip.clone(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();
        state_manager.add_devices(devices).unwrap();
        
        // Create a group with all speakers (first speaker is coordinator)
        let coordinator_id = speakers[0].0.clone();
        let member_ids: Vec<SpeakerId> = speakers.iter().map(|(id, _)| id.clone()).collect();
        let group_id = GroupId::new(format!("{}:1", coordinator_id.as_str()));
        let group_info = GroupInfo::new(
            group_id.clone(),
            coordinator_id.clone(),
            member_ids.clone(),
        );
        
        // Initialize with the group
        let topology = sonos_state::Topology::new(
            state_manager.speaker_infos(),
            vec![group_info.clone()],
        );
        state_manager.initialize(topology);
        
        // Verify all speakers can look up the same group
        for (speaker_id, _) in &speakers {
            let found_group = state_manager.get_group_for_speaker(speaker_id);
            prop_assert!(
                found_group.is_some(),
                "get_group_for_speaker should return a group for speaker '{}'", speaker_id
            );
            
            let found_group = found_group.unwrap();
            prop_assert_eq!(
                found_group.id,
                group_id.clone(),
                "All speakers should be in the same group"
            );
            
            prop_assert!(
                found_group.member_ids.contains(speaker_id),
                "Group member_ids should contain speaker '{}'", speaker_id
            );
        }
        
        // Verify get_group returns the same group
        let group_by_id = state_manager.get_group(&group_id.clone());
        prop_assert!(group_by_id.is_some());
        prop_assert_eq!(group_by_id.unwrap(), group_info);
    }


    /// *For any* set of groups in the system, groups() should return all groups
    /// and each group should be accessible via get_group.
    #[test]
    fn prop_groups_list_consistency(
        group_count in 1usize..4,
    ) {
        let state_manager = StateManager::new().unwrap();
        
        // Generate unique speakers and groups (one speaker per group for simplicity)
        let mut all_groups = Vec::new();
        let mut all_devices = Vec::new();
        
        for i in 0..group_count {
            let speaker_id = SpeakerId::new(format!("RINCON_{:012}", i));
            let ip = format!("192.168.1.{}", 100 + i);
            let group_id = GroupId::new(format!("RINCON_{:012}:1", i));
            
            all_devices.push(Device {
                id: speaker_id.as_str().to_string(),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: ip,
                port: 1400,
                model_name: "Sonos One".to_string(),
            });
            
            all_groups.push(GroupInfo::new(
                group_id,
                speaker_id.clone(),
                vec![speaker_id],
            ));
        }
        
        state_manager.add_devices(all_devices).unwrap();
        
        // Initialize with all groups
        let topology = sonos_state::Topology::new(
            state_manager.speaker_infos(),
            all_groups.clone(),
        );
        state_manager.initialize(topology);
        
        // Verify groups() returns all groups
        let returned_groups = state_manager.groups();
        prop_assert_eq!(
            returned_groups.len(),
            group_count,
            "groups() should return all {} groups", group_count
        );
        
        // Verify each group is accessible via get_group
        for group in &all_groups {
            let found = state_manager.get_group(&group.id);
            prop_assert!(
                found.is_some(),
                "get_group should find group '{}'", group.id
            );
            prop_assert_eq!(
                found.unwrap(),
                group.clone(),
                "get_group should return the correct group"
            );
        }
    }
}


// ============================================================================
// Property 1 (speaker-groups): Group Invariants
// ============================================================================

// Note: Group invariants are tested using GroupInfo directly since Group::from_info
// is pub(crate). The invariants are validated through the GroupInfo struct which
// has the same fields and semantics as Group.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* Group instance (via GroupInfo), the following invariants SHALL hold:
    /// - The group has a non-empty GroupId
    /// - The coordinator_id is contained in member_ids
    /// - member_ids has at least one element (the coordinator)
    /// - is_standalone() is true for single-speaker groups
    #[test]
    fn prop_group_invariants_single_speaker(
        speaker_id in speaker_id_strategy(),
    ) {
        let speaker_id_obj = SpeakerId::new(&speaker_id);
        
        // Create a group with this speaker as coordinator
        let group_id = GroupId::new(format!("{}:1", speaker_id));
        let group_info = GroupInfo::new(
            group_id.clone(),
            speaker_id_obj.clone(),
            vec![speaker_id_obj.clone()],
        );
        
        // Invariant 1: Group has a non-empty GroupId
        prop_assert!(
            !group_info.id.as_str().is_empty(),
            "Group should have a non-empty GroupId"
        );
        
        // Invariant 2: coordinator_id is contained in member_ids
        prop_assert!(
            group_info.member_ids.contains(&group_info.coordinator_id),
            "coordinator_id should be contained in member_ids"
        );
        
        // Invariant 3: member_ids has at least one element
        prop_assert!(
            !group_info.member_ids.is_empty(),
            "member_ids should have at least one element"
        );
        
        // Invariant 4: is_standalone() is true for single-speaker groups
        prop_assert!(
            group_info.is_standalone(),
            "Single-speaker group should be standalone"
        );
        
        // Invariant 5: coordinator is correctly identified
        prop_assert!(
            group_info.coordinator_id == speaker_id_obj,
            "Coordinator should be the speaker"
        );
    }


    /// *For any* Group with multiple speakers, the invariants should hold
    /// and coordinator should be correctly identified.
    #[test]
    fn prop_group_invariants_multi_speaker(
        member_count in 2usize..5,
    ) {
        // Generate unique speakers
        let speakers: Vec<SpeakerId> = (0..member_count)
            .map(|i| SpeakerId::new(format!("RINCON_{:012}", i)))
            .collect();
        
        // Create a group with first speaker as coordinator
        let coordinator_id = speakers[0].clone();
        let member_ids = speakers.clone();
        let group_id = GroupId::new(format!("{}:1", coordinator_id.as_str()));
        let group_info = GroupInfo::new(
            group_id.clone(),
            coordinator_id.clone(),
            member_ids.clone(),
        );
        
        // Invariant 1: Group has a non-empty GroupId
        prop_assert!(
            !group_info.id.as_str().is_empty(),
            "Group should have a non-empty GroupId"
        );
        
        // Invariant 2: coordinator_id is contained in member_ids
        prop_assert!(
            group_info.member_ids.contains(&group_info.coordinator_id),
            "coordinator_id should be contained in member_ids"
        );
        
        // Invariant 3: member_ids has at least one element
        prop_assert!(
            !group_info.member_ids.is_empty(),
            "member_ids should have at least one element"
        );
        
        // Invariant 4: member_ids.len() equals member_count
        prop_assert_eq!(
            group_info.member_ids.len(),
            member_count,
            "member_ids.len() should equal member_count"
        );
        
        // Invariant 5: is_standalone() is false for multi-speaker groups
        prop_assert!(
            !group_info.is_standalone(),
            "Multi-speaker group should not be standalone"
        );
        
        // Invariant 6: All members are in member_ids
        for speaker_id in &speakers {
            prop_assert!(
                group_info.member_ids.contains(speaker_id),
                "member_ids should contain speaker '{}'", speaker_id
            );
        }
    }
}


// ============================================================================
// Property 4 (speaker-groups): GroupMembership Synchronization
// ============================================================================

use sonos_state::GroupMembership;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* topology change:
    /// - Every speaker affected by the change has its GroupMembership updated
    /// - If the property is watched, a change event is emitted
    /// - The GroupMembership.group_id is never None (always valid)
    /// - After ungrouping, each speaker has its own single-speaker group
    #[test]
    fn prop_group_membership_synchronization(
        speaker_count in 2usize..5,
    ) {
        let state_manager = StateManager::new().unwrap();
        
        // Generate unique speakers
        let speakers: Vec<(SpeakerId, String)> = (0..speaker_count)
            .map(|i| {
                let id = format!("RINCON_{:012}", i);
                let ip = format!("192.168.1.{}", 100 + i);
                (SpeakerId::new(&id), ip)
            })
            .collect();
        
        // Add devices
        let devices: Vec<Device> = speakers.iter().enumerate()
            .map(|(i, (id, ip))| Device {
                id: id.as_str().to_string(),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: ip.clone(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();
        state_manager.add_devices(devices).unwrap();
        
        // Create a group with all speakers (first speaker is coordinator)
        let coordinator_id = speakers[0].0.clone();
        let member_ids: Vec<SpeakerId> = speakers.iter().map(|(id, _)| id.clone()).collect();
        let group_id = GroupId::new(format!("{}:1", coordinator_id.as_str()));
        let group_info = GroupInfo::new(
            group_id.clone(),
            coordinator_id.clone(),
            member_ids.clone(),
        );
        
        // Initialize with the group
        let topology = sonos_state::Topology::new(
            state_manager.speaker_infos(),
            vec![group_info.clone()],
        );
        state_manager.initialize(topology);
        
        // Set GroupMembership for each speaker (simulating what event_worker does)
        for (i, (speaker_id, _)) in speakers.iter().enumerate() {
            let is_coordinator = i == 0;
            state_manager.set_property(speaker_id, GroupMembership::new(group_id.clone(), is_coordinator));
        }
        
        // Verify GroupMembership is set for all speakers
        for (i, (speaker_id, _)) in speakers.iter().enumerate() {
            let membership = state_manager.get_property::<GroupMembership>(speaker_id);
            prop_assert!(
                membership.is_some(),
                "Speaker {} should have GroupMembership set", speaker_id
            );
            
            let membership = membership.unwrap();
            
            // Property: GroupMembership.group_id is never empty (always valid)
            prop_assert!(
                !membership.group_id.as_str().is_empty(),
                "GroupMembership.group_id should never be empty"
            );
            
            // Property: GroupMembership.group_id matches the group
            prop_assert_eq!(
                membership.group_id,
                group_id.clone(),
                "GroupMembership.group_id should match the group ID"
            );
            
            // Property: is_coordinator is true only for the coordinator
            let expected_is_coordinator = i == 0;
            prop_assert_eq!(
                membership.is_coordinator,
                expected_is_coordinator,
                "is_coordinator should be {} for speaker {}", expected_is_coordinator, speaker_id
            );
        }
    }


    ///
    /// *For any* set of speakers that were grouped and then ungrouped,
    /// each speaker should have its own single-speaker group with valid GroupMembership.
    #[test]
    fn prop_group_membership_after_ungrouping(
        speaker_count in 2usize..5,
    ) {
        let state_manager = StateManager::new().unwrap();
        
        // Generate unique speakers
        let speakers: Vec<(SpeakerId, String)> = (0..speaker_count)
            .map(|i| {
                let id = format!("RINCON_{:012}", i);
                let ip = format!("192.168.1.{}", 100 + i);
                (SpeakerId::new(&id), ip)
            })
            .collect();
        
        // Add devices
        let devices: Vec<Device> = speakers.iter().enumerate()
            .map(|(i, (id, ip))| Device {
                id: id.as_str().to_string(),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: ip.clone(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();
        state_manager.add_devices(devices).unwrap();
        
        // Create individual groups for each speaker (simulating ungrouped state)
        let groups: Vec<GroupInfo> = speakers.iter()
            .map(|(speaker_id, _)| {
                let group_id = GroupId::new(format!("{}:1", speaker_id.as_str()));
                GroupInfo::new(
                    group_id,
                    speaker_id.clone(),
                    vec![speaker_id.clone()],
                )
            })
            .collect();
        
        // Initialize with individual groups
        let topology = sonos_state::Topology::new(
            state_manager.speaker_infos(),
            groups.clone(),
        );
        state_manager.initialize(topology);
        
        // Set GroupMembership for each speaker (simulating what event_worker does)
        for (speaker_id, _) in &speakers {
            let group_id = GroupId::new(format!("{}:1", speaker_id.as_str()));
            state_manager.set_property(speaker_id, GroupMembership::new(group_id, true));
        }
        
        // Verify each speaker has its own group and is coordinator
        for (speaker_id, _) in &speakers {
            let membership = state_manager.get_property::<GroupMembership>(speaker_id);
            prop_assert!(
                membership.is_some(),
                "Speaker {} should have GroupMembership", speaker_id
            );
            
            let membership = membership.unwrap();
            
            // Property: GroupMembership.group_id is always valid
            prop_assert!(
                !membership.group_id.as_str().is_empty(),
                "GroupMembership.group_id should never be empty"
            );
            
            // Property: Each speaker is coordinator of its own group
            prop_assert!(
                membership.is_coordinator,
                "Speaker {} should be coordinator of its own group", speaker_id
            );
            
            // Property: Group is standalone (single speaker)
            let group = state_manager.get_group(&membership.group_id);
            prop_assert!(group.is_some());
            prop_assert!(
                group.unwrap().is_standalone(),
                "Each speaker's group should be standalone after ungrouping"
            );
        }
    }


    /// *For any* speaker with watched GroupMembership, when the membership changes,
    /// a change event should be emitted.
    #[test]
    fn prop_group_membership_watch_events(
        speaker_id in speaker_id_strategy(),
        ip in ip_strategy(),
    ) {
        let state_manager = StateManager::new().unwrap();
        let speaker_id_obj = SpeakerId::new(&speaker_id);
        
        // Add device
        let devices = vec![Device {
            id: speaker_id.clone(),
            name: format!("Test Speaker {}", speaker_id),
            room_name: "Test Room".to_string(),
            ip_address: ip.clone(),
            port: 1400,
            model_name: "Sonos One".to_string(),
        }];
        state_manager.add_devices(devices).unwrap();
        
        // Watch GroupMembership
        state_manager.register_watch(&speaker_id_obj, GroupMembership::KEY);
        
        // Set initial GroupMembership
        let group_id1 = GroupId::new(format!("{}:1", speaker_id));
        state_manager.set_property(&speaker_id_obj, GroupMembership::new(group_id1.clone(), true));
        
        // Get iterator and consume the first event
        let iter = state_manager.iter();
        let event1 = iter.recv_timeout(std::time::Duration::from_millis(100));
        prop_assert!(event1.is_some(), "First GroupMembership change should emit event");
        
        // Change GroupMembership (simulate joining a different group)
        let group_id2 = GroupId::new(format!("OTHER_GROUP:1"));
        state_manager.set_property(&speaker_id_obj, GroupMembership::new(group_id2.clone(), false));
        
        // Should receive another event
        let event2 = iter.recv_timeout(std::time::Duration::from_millis(100));
        prop_assert!(event2.is_some(), "GroupMembership change should emit event");
        
        let event2 = event2.unwrap();
        prop_assert_eq!(event2.speaker_id, speaker_id_obj);
        prop_assert_eq!(event2.property_key, GroupMembership::KEY);
    }
}


// ============================================================================
// Property 5 (speaker-groups): Topology Property Completeness
// ============================================================================

use sonos_state::Topology;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// *For any* state after topology processing:
    /// - The Topology property contains all groups from the store
    /// - The Topology property contains all speakers from the store
    /// - Each speaker in Topology has a corresponding GroupMembership
    #[test]
    fn prop_topology_completeness(
        speaker_count in 1usize..5,
        group_count in 1usize..3,
    ) {
        // Ensure we have at least as many speakers as groups
        let actual_speaker_count = speaker_count.max(group_count);
        
        let state_manager = StateManager::new().unwrap();
        
        // Generate unique speakers
        let speakers: Vec<(SpeakerId, String)> = (0..actual_speaker_count)
            .map(|i| {
                let id = format!("RINCON_{:012}", i);
                let ip = format!("192.168.1.{}", 100 + i);
                (SpeakerId::new(&id), ip)
            })
            .collect();
        
        // Add devices
        let devices: Vec<Device> = speakers.iter().enumerate()
            .map(|(i, (id, ip))| Device {
                id: id.as_str().to_string(),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: ip.clone(),
                port: 1400,
                model_name: "Sonos One".to_string(),
            })
            .collect();
        state_manager.add_devices(devices).unwrap();
        
        // Distribute speakers across groups
        let speakers_per_group = actual_speaker_count / group_count;
        let mut groups = Vec::new();
        let mut speaker_idx = 0;
        
        for g in 0..group_count {
            let group_size = if g == group_count - 1 {
                // Last group gets remaining speakers
                actual_speaker_count - speaker_idx
            } else {
                speakers_per_group
            };
            
            if group_size == 0 {
                continue;
            }
            
            let coordinator_id = speakers[speaker_idx].0.clone();
            let member_ids: Vec<SpeakerId> = (speaker_idx..speaker_idx + group_size)
                .map(|i| speakers[i].0.clone())
                .collect();
            let group_id = GroupId::new(format!("{}:1", coordinator_id.as_str()));
            
            groups.push(GroupInfo::new(
                group_id,
                coordinator_id,
                member_ids,
            ));
            
            speaker_idx += group_size;
        }
        
        // Initialize with topology
        let speaker_infos = state_manager.speaker_infos();
        let topology = Topology::new(speaker_infos.clone(), groups.clone());
        state_manager.initialize(topology.clone());
        
        // Set GroupMembership for each speaker based on group membership
        for group in &groups {
            for (idx, member_id) in group.member_ids.iter().enumerate() {
                let is_coordinator = idx == 0 && *member_id == group.coordinator_id;
                state_manager.set_property(member_id, GroupMembership::new(group.id.clone(), is_coordinator));
            }
        }
        
        // Property: Topology contains all groups
        let stored_groups = state_manager.groups();
        prop_assert_eq!(
            stored_groups.len(),
            groups.len(),
            "Topology should contain all {} groups", groups.len()
        );
        
        // Property: Topology contains all speakers
        let stored_speakers = state_manager.speaker_infos();
        prop_assert_eq!(
            stored_speakers.len(),
            actual_speaker_count,
            "Topology should contain all {} speakers", actual_speaker_count
        );
        
        // Property: Each speaker has a corresponding GroupMembership
        for (speaker_id, _) in &speakers {
            let membership = state_manager.get_property::<GroupMembership>(speaker_id);
            prop_assert!(
                membership.is_some(),
                "Speaker {} should have GroupMembership in topology", speaker_id
            );
            
            // Verify the membership points to a valid group
            let membership = membership.unwrap();
            let group = state_manager.get_group(&membership.group_id);
            prop_assert!(
                group.is_some(),
                "GroupMembership.group_id should point to a valid group"
            );
            
            // Verify the speaker is in the group's member list
            let group = group.unwrap();
            prop_assert!(
                group.member_ids.contains(speaker_id),
                "Speaker should be in its group's member_ids"
            );
        }
    }


    /// *For any* topology, the groups in the Topology property should match
    /// the groups accessible via get_group and groups().
    #[test]
    fn prop_topology_groups_match_store(
        group_count in 1usize..4,
    ) {
        let state_manager = StateManager::new().unwrap();
        
        // Generate one speaker per group
        let mut groups = Vec::new();
        let mut devices = Vec::new();
        
        for i in 0..group_count {
            let speaker_id = SpeakerId::new(format!("RINCON_{:012}", i));
            let ip = format!("192.168.1.{}", 100 + i);
            let group_id = GroupId::new(format!("RINCON_{:012}:1", i));
            
            devices.push(Device {
                id: speaker_id.as_str().to_string(),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: ip,
                port: 1400,
                model_name: "Sonos One".to_string(),
            });
            
            groups.push(GroupInfo::new(
                group_id,
                speaker_id.clone(),
                vec![speaker_id],
            ));
        }
        
        state_manager.add_devices(devices).unwrap();
        
        // Initialize with topology
        let topology = Topology::new(
            state_manager.speaker_infos(),
            groups.clone(),
        );
        state_manager.initialize(topology);
        
        // Property: groups() returns all groups from topology
        let stored_groups = state_manager.groups();
        prop_assert_eq!(
            stored_groups.len(),
            group_count,
            "groups() should return all {} groups", group_count
        );
        
        // Property: Each group is accessible via get_group
        for group in &groups {
            let found = state_manager.get_group(&group.id);
            prop_assert!(
                found.is_some(),
                "get_group should find group '{}'", group.id
            );
            prop_assert_eq!(
                found.unwrap(),
                group.clone(),
                "get_group should return the correct group"
            );
        }
        
        // Property: group_count() matches
        prop_assert_eq!(
            state_manager.group_count(),
            group_count,
            "group_count() should match"
        );
    }


    /// *For any* speaker in the topology, get_group_for_speaker should return
    /// a valid group containing that speaker.
    #[test]
    fn prop_topology_speakers_have_groups(
        speaker_count in 1usize..5,
    ) {
        let state_manager = StateManager::new().unwrap();
        
        // Generate speakers with individual groups
        let mut groups = Vec::new();
        let mut devices = Vec::new();
        let mut speaker_ids = Vec::new();
        
        for i in 0..speaker_count {
            let speaker_id = SpeakerId::new(format!("RINCON_{:012}", i));
            let ip = format!("192.168.1.{}", 100 + i);
            let group_id = GroupId::new(format!("RINCON_{:012}:1", i));
            
            devices.push(Device {
                id: speaker_id.as_str().to_string(),
                name: format!("Speaker {}", i),
                room_name: format!("Room {}", i),
                ip_address: ip,
                port: 1400,
                model_name: "Sonos One".to_string(),
            });
            
            groups.push(GroupInfo::new(
                group_id,
                speaker_id.clone(),
                vec![speaker_id.clone()],
            ));
            
            speaker_ids.push(speaker_id);
        }
        
        state_manager.add_devices(devices).unwrap();
        
        // Initialize with topology
        let topology = Topology::new(
            state_manager.speaker_infos(),
            groups.clone(),
        );
        state_manager.initialize(topology);
        
        // Property: Each speaker has a group via get_group_for_speaker
        for speaker_id in &speaker_ids {
            let group = state_manager.get_group_for_speaker(speaker_id);
            prop_assert!(
                group.is_some(),
                "get_group_for_speaker should return a group for speaker '{}'", speaker_id
            );
            
            let group = group.unwrap();
            
            // Property: The group contains the speaker
            prop_assert!(
                group.member_ids.contains(speaker_id),
                "Group should contain speaker '{}'", speaker_id
            );
        }
    }
}
