//! Property-based tests for the DOM-like SDK
//!
//! These tests validate the correctness properties defined in the design document.
//! Each test is tagged with the property number and requirements it validates.

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
fn create_test_state_manager(speaker_id: &str, ip: &str) -> Arc<StateManager> {
    let manager = StateManager::new().unwrap();
    let devices = vec![Device {
        id: speaker_id.to_string(),
        name: format!("Test Speaker {}", speaker_id),
        room_name: "Test Room".to_string(),
        ip_address: ip.to_string(),
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
    speaker_id: &str,
    ip: &str,
    state_manager: Arc<StateManager>,
) -> Arc<SpeakerContext> {
    let speaker_id_obj = SpeakerId::new(speaker_id);
    let speaker_ip: IpAddr = ip.parse().unwrap();
    let api_client = SonosClient::new();
    SpeakerContext::new(speaker_id_obj, speaker_ip, state_manager, api_client)
}

// ============================================================================
// Property 3: Watch Registers Property
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Feature: dom-like-sdk, Property 3: Watch Registers Property**
    ///
    /// *For any* speaker and any property type, after calling `property_handle.watch()`,
    /// the property SHALL be marked as watched (i.e., `property_handle.is_watched()` returns true).
    ///
    /// **Validates: Requirements 1.3**
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

    /// **Feature: dom-like-sdk, Property 4: Unwatch Unregisters Property**
    ///
    /// *For any* speaker and any property type that is currently watched,
    /// after calling `property_handle.unwatch()`, the property SHALL no longer
    /// be marked as watched (i.e., `property_handle.is_watched()` returns false).
    ///
    /// **Validates: Requirements 1.4**
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

    /// **Feature: dom-like-sdk, Property 3+4: Watch/Unwatch Round-Trip**
    ///
    /// *For any* speaker and property, multiple watch/unwatch cycles should
    /// correctly toggle the watched state.
    ///
    /// **Validates: Requirements 1.3, 1.4**
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

    /// **Feature: dom-like-sdk, Property 2: Fetch Updates Cache**
    ///
    /// *For any* speaker and any property type, after calling `property_handle.fetch()`
    /// successfully, a subsequent call to `property_handle.get()` SHALL return the
    /// same value that `fetch()` returned.
    ///
    /// This test validates the cache update mechanism that fetch() relies on.
    /// We test that set_property() (called by fetch() internally) correctly updates
    /// the cache so that get() returns the updated value.
    ///
    /// **Validates: Requirements 1.2**
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

    /// **Feature: dom-like-sdk, Property 2: Multiple Fetch Updates**
    ///
    /// *For any* sequence of fetch operations, the cache should always reflect
    /// the most recent fetched value.
    ///
    /// **Validates: Requirements 1.2**
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

    /// **Feature: dom-like-sdk, Property 5: Speaker Clone Equivalence**
    ///
    /// *For any* Speaker handle, cloning it SHALL produce a handle that provides
    /// access to the same property values - i.e., for any property P,
    /// `original.P.get() == clone.P.get()`.
    ///
    /// **Validates: Requirements 2.6**
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

    /// **Feature: dom-like-sdk, Property 5: Speaker Clone Shares State**
    ///
    /// *For any* Speaker handle, after cloning, changes to the underlying state
    /// should be visible through both the original and cloned handles.
    ///
    /// **Validates: Requirements 2.6**
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
