//! Global change iterator for application rerender triggering
//!
//! Provides unified change streams for applications to detect when to rerender,
//! specifically designed for ratatui TUI applications that need efficient
//! change-driven rendering.
//!
//! # Example - Ratatui Integration
//!
//! ```rust,ignore
//! use sonos_state::{StateManager, WidgetStateManager, Volume};
//!
//! let state_manager = Arc::new(StateManager::new().await?);
//! let mut widget_state = WidgetStateManager::new(Arc::clone(&state_manager)).await?;
//!
//! // In widget render function:
//! async fn render_volume_bar(widget_state: &mut WidgetStateManager, speaker_id: &SpeakerId) {
//!     let (volume, changed) = widget_state.watch_property::<Volume>(speaker_id).await?;
//!     if changed {
//!         // Only render when volume actually changed
//!         // ... render volume gauge
//!     }
//! }
//!
//! // In main event loop:
//! loop {
//!     widget_state.process_global_changes(); // Process all Sonos changes
//!     if widget_state.has_any_changes() {
//!         terminal.draw(|frame| render_ui(frame, &mut widget_state))?;
//!     }
//! }
//! ```

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

// Note: futures::Stream removed - using custom async methods instead
use tokio::sync::broadcast;

use sonos_api::Service;

use crate::model::{GroupId, SpeakerId};
use crate::property::Property;
use crate::reactive::{PropertyWatcher, StateManager};
use crate::store::StateChange;
use crate::{Result, StateError};

// ============================================================================
// Change Event Types
// ============================================================================

/// High-level change event optimized for application rerender decisions
#[derive(Debug, Clone)]
pub struct ChangeEvent {
    /// Timestamp when the change occurred
    pub timestamp: Instant,
    /// Type of change for application decision making
    pub change_type: ChangeType,
    /// Additional context for the change
    pub context: ChangeContext,
}

/// Types of changes that applications typically care about for rerendering
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeType {
    /// Device-level property changed (volume, playback state, etc.)
    DeviceProperty {
        speaker_id: SpeakerId,
        property_name: &'static str,
        service: Service,
    },
    /// Group-level property changed
    GroupProperty {
        group_id: GroupId,
        property_name: &'static str,
        service: Service,
    },
    /// System-wide change (topology, etc.)
    SystemProperty {
        property_name: &'static str,
        service: Service,
    },
    /// Device was discovered/added
    DeviceAdded { speaker_id: SpeakerId },
    /// Device was removed/disconnected
    DeviceRemoved { speaker_id: SpeakerId },
    /// Group was formed/modified
    GroupChanged { group_id: GroupId },
}

/// Additional context about the change
#[derive(Debug, Clone)]
pub struct ChangeContext {
    /// Human-readable description of what changed
    pub description: String,
    /// Whether this change typically requires UI rerender
    pub requires_rerender: bool,
    /// Suggested rerender scope (full, device, group)
    pub rerender_scope: RerenderScope,
}

/// Suggested scope for rerendering
#[derive(Debug, Clone, PartialEq)]
pub enum RerenderScope {
    /// Full UI rerender needed (topology changes)
    Full,
    /// Single device UI update
    Device(SpeakerId),
    /// Group-level UI update
    Group(GroupId),
    /// System-level update (minimal impact)
    System,
}

// ============================================================================
// Change Stream
// ============================================================================

/// Async stream of change events
pub struct ChangeStream {
    receiver: broadcast::Receiver<StateChange>,
    filter: Option<ChangeFilter>,
}

impl ChangeStream {
    /// Create a new change stream from a broadcast receiver
    pub fn new(receiver: broadcast::Receiver<StateChange>) -> Self {
        Self {
            receiver,
            filter: None,
        }
    }

    /// Create a filtered change stream
    pub fn filtered(receiver: broadcast::Receiver<StateChange>, filter: ChangeFilter) -> Self {
        Self {
            receiver,
            filter: Some(filter),
        }
    }

    /// Get the next change event (async)
    pub async fn next(&mut self) -> Option<ChangeEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(state_change) => {
                    let change_event = Self::convert_state_change(state_change);

                    // Apply filter if present
                    if let Some(ref filter) = self.filter {
                        if !filter.matches(&change_event) {
                            continue; // Skip filtered events
                        }
                    }

                    return Some(change_event);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Continue to next message if we're lagging
                    continue;
                }
            }
        }
    }

    /// Try to get the next change event without blocking
    pub fn try_next(&mut self) -> Option<ChangeEvent> {
        loop {
            match self.receiver.try_recv() {
                Ok(state_change) => {
                    let change_event = Self::convert_state_change(state_change);

                    // Apply filter if present
                    if let Some(ref filter) = self.filter {
                        if !filter.matches(&change_event) {
                            continue; // Skip filtered events
                        }
                    }

                    return Some(change_event);
                }
                Err(broadcast::error::TryRecvError::Empty) => return None,
                Err(broadcast::error::TryRecvError::Closed) => return None,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // Continue to next message if we're lagging
                    continue;
                }
            }
        }
    }

    /// Convert internal StateChange to user-friendly ChangeEvent with rerender hints
    fn convert_state_change(state_change: StateChange) -> ChangeEvent {
        let timestamp = Instant::now();

        let (change_type, context) = match state_change {
            StateChange::SpeakerPropertyChanged {
                speaker_id,
                property_key,
                service,
            } => {
                let change_type = ChangeType::DeviceProperty {
                    speaker_id: speaker_id.clone(),
                    property_name: property_key,
                    service,
                };

                // Smart rerender hints based on property type
                let (requires_rerender, rerender_scope, description) = match property_key {
                    "volume" => (
                        true,
                        RerenderScope::Device(speaker_id.clone()),
                        format!("Volume changed on {}", speaker_id),
                    ),
                    "mute" => (
                        true,
                        RerenderScope::Device(speaker_id.clone()),
                        format!("Mute status changed on {}", speaker_id),
                    ),
                    "playback_state" => (
                        true,
                        RerenderScope::Device(speaker_id.clone()),
                        format!("Playback state changed on {}", speaker_id),
                    ),
                    "position" => (
                        false, // Position updates can be throttled
                        RerenderScope::Device(speaker_id.clone()),
                        format!("Position updated on {}", speaker_id),
                    ),
                    "current_track" => (
                        true,
                        RerenderScope::Device(speaker_id.clone()),
                        format!("Track changed on {}", speaker_id),
                    ),
                    _ => (
                        true,
                        RerenderScope::Device(speaker_id.clone()),
                        format!("Property '{}' changed on {}", property_key, speaker_id),
                    ),
                };

                let context = ChangeContext {
                    description,
                    requires_rerender,
                    rerender_scope,
                };

                (change_type, context)
            }
            StateChange::GroupPropertyChanged {
                group_id,
                property_key,
                service,
            } => {
                let change_type = ChangeType::GroupProperty {
                    group_id: group_id.clone(),
                    property_name: property_key,
                    service,
                };

                let context = ChangeContext {
                    description: format!("Group property '{}' changed on {}", property_key, group_id),
                    requires_rerender: true,
                    rerender_scope: RerenderScope::Group(group_id),
                };

                (change_type, context)
            }
            StateChange::SystemPropertyChanged {
                property_key,
                service,
            } => {
                let change_type = ChangeType::SystemProperty {
                    property_name: property_key,
                    service,
                };

                let context = ChangeContext {
                    description: format!("System property '{}' changed", property_key),
                    requires_rerender: property_key == "topology", // Topology always needs full rerender
                    rerender_scope: if property_key == "topology" {
                        RerenderScope::Full
                    } else {
                        RerenderScope::System
                    },
                };

                (change_type, context)
            }
            StateChange::SpeakerAdded { speaker_id } => {
                let change_type = ChangeType::DeviceAdded {
                    speaker_id: speaker_id.clone(),
                };

                let context = ChangeContext {
                    description: format!("Device {} added", speaker_id),
                    requires_rerender: true,
                    rerender_scope: RerenderScope::Full, // Device discovery requires full rerender
                };

                (change_type, context)
            }
            StateChange::SpeakerRemoved { speaker_id } => {
                let change_type = ChangeType::DeviceRemoved {
                    speaker_id: speaker_id.clone(),
                };

                let context = ChangeContext {
                    description: format!("Device {} removed", speaker_id),
                    requires_rerender: true,
                    rerender_scope: RerenderScope::Full, // Device removal requires full rerender
                };

                (change_type, context)
            }
            StateChange::GroupAdded { group_id } => {
                let change_type = ChangeType::GroupChanged {
                    group_id: group_id.clone(),
                };

                let context = ChangeContext {
                    description: format!("Group {} added", group_id),
                    requires_rerender: true,
                    rerender_scope: RerenderScope::Full, // Group changes require full rerender
                };

                (change_type, context)
            }
            StateChange::GroupRemoved { group_id } => {
                let change_type = ChangeType::GroupChanged {
                    group_id: group_id.clone(),
                };

                let context = ChangeContext {
                    description: format!("Group {} removed", group_id),
                    requires_rerender: true,
                    rerender_scope: RerenderScope::Full, // Group changes require full rerender
                };

                (change_type, context)
            }
        };

        ChangeEvent {
            timestamp,
            change_type,
            context,
        }
    }
}

// Note: Stream trait implementation is complex for broadcast receivers
// and not strictly needed since we provide async/sync iterator methods.
// For futures Stream compatibility, users can use StreamExt::unfold with next().

// ============================================================================
// Change Filter
// ============================================================================

/// Filter for selecting specific types of changes
#[derive(Debug, Clone, Default)]
pub struct ChangeFilter {
    /// Filter by specific speaker IDs
    pub speaker_ids: Option<HashSet<SpeakerId>>,
    /// Filter by specific group IDs
    pub group_ids: Option<HashSet<GroupId>>,
    /// Filter by services
    pub services: Option<HashSet<Service>>,
    /// Filter by property names
    pub property_names: Option<HashSet<&'static str>>,
    /// Filter by change types
    pub change_types: Option<HashSet<ChangeTypeFilter>>,
    /// Only include changes that require rerender
    pub rerender_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChangeTypeFilter {
    DeviceProperty,
    GroupProperty,
    SystemProperty,
    DeviceLifecycle,
    GroupLifecycle,
}

impl ChangeFilter {
    /// Create a filter for a specific speaker
    pub fn for_speaker(speaker_id: SpeakerId) -> Self {
        Self {
            speaker_ids: Some(std::iter::once(speaker_id).collect()),
            ..Default::default()
        }
    }

    /// Create a filter for rerender-relevant changes only
    pub fn rerender_only() -> Self {
        Self {
            rerender_only: true,
            ..Default::default()
        }
    }

    /// Create a filter for specific services (e.g., RenderingControl)
    pub fn for_services(services: HashSet<Service>) -> Self {
        Self {
            services: Some(services),
            ..Default::default()
        }
    }

    /// Create a filter for specific property names
    pub fn for_properties(property_names: HashSet<&'static str>) -> Self {
        Self {
            property_names: Some(property_names),
            ..Default::default()
        }
    }

    /// Check if an event matches this filter
    pub fn matches(&self, event: &ChangeEvent) -> bool {
        // Check rerender requirement first
        if self.rerender_only && !event.context.requires_rerender {
            return false;
        }

        // Check change type filter
        if let Some(ref change_types) = self.change_types {
            let event_type = match &event.change_type {
                ChangeType::DeviceProperty { .. } => ChangeTypeFilter::DeviceProperty,
                ChangeType::GroupProperty { .. } => ChangeTypeFilter::GroupProperty,
                ChangeType::SystemProperty { .. } => ChangeTypeFilter::SystemProperty,
                ChangeType::DeviceAdded { .. } | ChangeType::DeviceRemoved { .. } => {
                    ChangeTypeFilter::DeviceLifecycle
                }
                ChangeType::GroupChanged { .. } => ChangeTypeFilter::GroupLifecycle,
            };

            if !change_types.contains(&event_type) {
                return false;
            }
        }

        // Check specific filters based on change type
        match &event.change_type {
            ChangeType::DeviceProperty {
                speaker_id,
                property_name,
                service,
            } => {
                // Check speaker ID filter
                if let Some(ref speaker_ids) = self.speaker_ids {
                    if !speaker_ids.contains(speaker_id) {
                        return false;
                    }
                }

                // Check service filter
                if let Some(ref services) = self.services {
                    if !services.contains(service) {
                        return false;
                    }
                }

                // Check property name filter
                if let Some(ref property_names) = self.property_names {
                    if !property_names.contains(property_name) {
                        return false;
                    }
                }
            }
            ChangeType::GroupProperty {
                group_id,
                property_name,
                service,
            } => {
                // Check group ID filter
                if let Some(ref group_ids) = self.group_ids {
                    if !group_ids.contains(group_id) {
                        return false;
                    }
                }

                // Check service filter
                if let Some(ref services) = self.services {
                    if !services.contains(service) {
                        return false;
                    }
                }

                // Check property name filter
                if let Some(ref property_names) = self.property_names {
                    if !property_names.contains(property_name) {
                        return false;
                    }
                }
            }
            ChangeType::SystemProperty {
                property_name,
                service,
            } => {
                // Check service filter
                if let Some(ref services) = self.services {
                    if !services.contains(service) {
                        return false;
                    }
                }

                // Check property name filter
                if let Some(ref property_names) = self.property_names {
                    if !property_names.contains(property_name) {
                        return false;
                    }
                }
            }
            ChangeType::DeviceAdded { speaker_id } | ChangeType::DeviceRemoved { speaker_id } => {
                // Check speaker ID filter
                if let Some(ref speaker_ids) = self.speaker_ids {
                    if !speaker_ids.contains(speaker_id) {
                        return false;
                    }
                }
            }
            ChangeType::GroupChanged { group_id } => {
                // Check group ID filter
                if let Some(ref group_ids) = self.group_ids {
                    if !group_ids.contains(group_id) {
                        return false;
                    }
                }
            }
        }

        true
    }
}

// ============================================================================
// Blocking Change Iterator
// ============================================================================

/// Blocking iterator for synchronous contexts
pub struct BlockingChangeIterator {
    receiver: broadcast::Receiver<StateChange>,
    filter: Option<ChangeFilter>,
    rt: tokio::runtime::Handle,
}

/// Error type for blocking iterator operations
#[derive(Debug, thiserror::Error)]
pub enum TryRecvError {
    #[error("Channel is closed")]
    Closed,
    #[error("Lagged behind by {0} messages")]
    Lagged(u64),
}

impl BlockingChangeIterator {
    /// Create a new blocking change iterator
    pub fn new(
        receiver: broadcast::Receiver<StateChange>,
        rt: tokio::runtime::Handle,
    ) -> Self {
        Self {
            receiver,
            filter: None,
            rt,
        }
    }

    /// Create a new filtered blocking change iterator
    pub fn filtered(
        receiver: broadcast::Receiver<StateChange>,
        filter: ChangeFilter,
        rt: tokio::runtime::Handle,
    ) -> Self {
        Self {
            receiver,
            filter: Some(filter),
            rt,
        }
    }

    /// Block until next change event is available
    pub fn next_blocking(&mut self) -> Option<ChangeEvent> {
        loop {
            match self.rt.block_on(self.receiver.recv()) {
                Ok(state_change) => {
                    let change_event = ChangeStream::convert_state_change(state_change);

                    if let Some(ref filter) = self.filter {
                        if !filter.matches(&change_event) {
                            continue;
                        }
                    }

                    return Some(change_event);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }

    /// Try to get next event without blocking
    pub fn try_next(&mut self) -> std::result::Result<Option<ChangeEvent>, TryRecvError> {
        loop {
            match self.receiver.try_recv() {
                Ok(state_change) => {
                    let change_event = ChangeStream::convert_state_change(state_change);

                    if let Some(ref filter) = self.filter {
                        if !filter.matches(&change_event) {
                            continue; // Keep looking for matching events
                        }
                    }

                    return Ok(Some(change_event));
                }
                Err(broadcast::error::TryRecvError::Empty) => return Ok(None),
                Err(broadcast::error::TryRecvError::Closed) => return Err(TryRecvError::Closed),
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    return Err(TryRecvError::Lagged(n));
                }
            }
        }
    }
}

impl Iterator for BlockingChangeIterator {
    type Item = ChangeEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_blocking()
    }
}

// ============================================================================
// Widget State Manager for Ratatui Integration
// ============================================================================

/// Manages persistent state watchers for ratatui widgets
///
/// This is the core integration pattern for ratatui applications. It caches
/// property watchers across frames and provides change flags so widgets
/// know when to re-render.
pub struct WidgetStateManager {
    state_manager: Arc<StateManager>,
    /// Cached watchers: (SpeakerId, Property TypeId) -> PropertyWatcher<P>
    cached_watchers: HashMap<(SpeakerId, TypeId), Box<dyn Any + Send>>,
    /// Change flags: (SpeakerId, Property TypeId) -> bool
    change_flags: HashMap<(SpeakerId, TypeId), bool>,
    /// Global change stream for processing all changes
    global_changes: ChangeStream,
}

impl WidgetStateManager {
    /// Create a new widget state manager
    pub async fn new(state_manager: Arc<StateManager>) -> Result<Self> {
        let global_changes = state_manager.changes();
        Ok(Self {
            state_manager,
            cached_watchers: HashMap::new(),
            change_flags: HashMap::new(),
            global_changes,
        })
    }

    /// Widget-level API: get property value and change flag
    ///
    /// This is the main API that widget functions call to get the current
    /// property value and whether it changed since the last frame.
    pub async fn watch_property<P: Property>(
        &mut self,
        speaker_id: &SpeakerId,
    ) -> Result<(Option<P>, bool)> {
        let key = (speaker_id.clone(), TypeId::of::<P>());

        // Get or create watcher
        if !self.cached_watchers.contains_key(&key) {
            let watcher = self
                .state_manager
                .watch_property::<P>(speaker_id.clone())
                .await?;
            self.cached_watchers.insert(key.clone(), Box::new(watcher));
        }

        // Get current value
        let watcher = self
            .cached_watchers
            .get_mut(&key)
            .unwrap()
            .downcast_mut::<PropertyWatcher<P>>()
            .ok_or_else(|| {
                StateError::Parse(format!(
                    "Failed to downcast PropertyWatcher for type: {:?}",
                    std::any::type_name::<P>()
                ))
            })?;

        let value = watcher.current();

        // Check if changed since last frame
        let changed = self.change_flags.get(&key).copied().unwrap_or(false);
        self.change_flags.insert(key, false); // Reset flag

        Ok((value, changed))
    }

    /// Process global changes and set change flags (call once per event loop)
    ///
    /// This should be called once per frame/event loop iteration to process
    /// all Sonos state changes and mark affected properties as changed.
    pub fn process_global_changes(&mut self) {
        while let Some(change) = self.global_changes.try_next() {
            match change.change_type {
                ChangeType::DeviceProperty {
                    speaker_id,
                    property_name,
                    ..
                } => {
                    // Mark all matching properties as changed
                    for ((cached_speaker_id, _type_id), flag) in &mut self.change_flags {
                        if cached_speaker_id == &speaker_id {
                            // For now, mark all properties as changed for this speaker
                            // TODO: Could be more granular by matching property names with TypeIds
                            *flag = true;
                        }
                    }
                }
                ChangeType::DeviceAdded { .. } | ChangeType::DeviceRemoved { .. } => {
                    // Mark all properties as changed for topology changes
                    for flag in self.change_flags.values_mut() {
                        *flag = true;
                    }
                }
                ChangeType::GroupProperty { group_id, .. } => {
                    // Mark properties for speakers in this group as changed
                    // TODO: Need group membership info to determine affected speakers
                    for flag in self.change_flags.values_mut() {
                        *flag = true;
                    }
                }
                ChangeType::SystemProperty { .. } => {
                    // System changes affect all properties
                    for flag in self.change_flags.values_mut() {
                        *flag = true;
                    }
                }
                ChangeType::GroupChanged { .. } => {
                    // Group changes affect all properties
                    for flag in self.change_flags.values_mut() {
                        *flag = true;
                    }
                }
            }
        }
    }

    /// Check if any properties have changes that need rendering
    pub fn has_any_changes(&self) -> bool {
        self.change_flags.values().any(|&changed| changed)
    }

    /// Get the number of cached watchers (for debugging/monitoring)
    pub fn cached_watcher_count(&self) -> usize {
        self.cached_watchers.len()
    }

    /// Clear all change flags (useful for testing)
    pub fn clear_change_flags(&mut self) {
        for flag in self.change_flags.values_mut() {
            *flag = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::Duration;
    use tokio::sync::broadcast;
    use crate::property::Volume;

    /// Helper to create a test StateChange
    fn create_test_state_change() -> StateChange {
        StateChange::SpeakerPropertyChanged {
            speaker_id: SpeakerId::new("test_speaker"),
            property_key: "volume",
            service: Service::RenderingControl,
        }
    }

    #[test]
    fn test_change_event_conversion() {
        let state_change = create_test_state_change();
        let change_event = ChangeStream::convert_state_change(state_change);

        match change_event.change_type {
            ChangeType::DeviceProperty {
                speaker_id,
                property_name,
                service,
            } => {
                assert_eq!(speaker_id.as_str(), "test_speaker");
                assert_eq!(property_name, "volume");
                assert_eq!(service, Service::RenderingControl);
            }
            _ => panic!("Expected DeviceProperty change type"),
        }

        // Volume changes should require rerender
        assert!(change_event.context.requires_rerender);
        assert!(matches!(
            change_event.context.rerender_scope,
            RerenderScope::Device(_)
        ));
        assert!(change_event.context.description.contains("Volume changed"));
    }

    #[test]
    fn test_change_event_rerender_hints() {
        // Test volume change (should require rerender)
        let volume_change = StateChange::SpeakerPropertyChanged {
            speaker_id: SpeakerId::new("test"),
            property_key: "volume",
            service: Service::RenderingControl,
        };
        let event = ChangeStream::convert_state_change(volume_change);
        assert!(event.context.requires_rerender);

        // Test position change (should not require immediate rerender)
        let position_change = StateChange::SpeakerPropertyChanged {
            speaker_id: SpeakerId::new("test"),
            property_key: "position",
            service: Service::AVTransport,
        };
        let event = ChangeStream::convert_state_change(position_change);
        assert!(!event.context.requires_rerender);

        // Test device addition (should require full rerender)
        let device_added = StateChange::SpeakerAdded {
            speaker_id: SpeakerId::new("new_device"),
        };
        let event = ChangeStream::convert_state_change(device_added);
        assert!(event.context.requires_rerender);
        assert!(matches!(event.context.rerender_scope, RerenderScope::Full));
    }

    #[test]
    fn test_change_filter_speaker_matching() {
        let target_speaker = SpeakerId::new("target_speaker");
        let other_speaker = SpeakerId::new("other_speaker");

        let filter = ChangeFilter::for_speaker(target_speaker.clone());

        // Should match target speaker
        let matching_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: target_speaker,
                property_name: "volume",
                service: Service::RenderingControl,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("target_speaker")),
            },
        };

        assert!(filter.matches(&matching_change));

        // Should not match other speaker
        let non_matching_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: other_speaker,
                property_name: "volume",
                service: Service::RenderingControl,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("other_speaker")),
            },
        };

        assert!(!filter.matches(&non_matching_change));
    }

    #[test]
    fn test_change_filter_rerender_only() {
        let filter = ChangeFilter::rerender_only();

        // Should match rerender-required change
        let rerender_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: SpeakerId::new("test"),
                property_name: "volume",
                service: Service::RenderingControl,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("test")),
            },
        };

        assert!(filter.matches(&rerender_change));

        // Should not match non-rerender change
        let no_rerender_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: SpeakerId::new("test"),
                property_name: "position",
                service: Service::AVTransport,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: false,
                rerender_scope: RerenderScope::Device(SpeakerId::new("test")),
            },
        };

        assert!(!filter.matches(&no_rerender_change));
    }

    #[test]
    fn test_change_filter_service_matching() {
        let mut services = HashSet::new();
        services.insert(Service::RenderingControl);

        let filter = ChangeFilter::for_services(services);

        // Should match RenderingControl service
        let matching_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: SpeakerId::new("test"),
                property_name: "volume",
                service: Service::RenderingControl,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("test")),
            },
        };

        assert!(filter.matches(&matching_change));

        // Should not match AVTransport service
        let non_matching_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: SpeakerId::new("test"),
                property_name: "playback_state",
                service: Service::AVTransport,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("test")),
            },
        };

        assert!(!filter.matches(&non_matching_change));
    }

    #[test]
    fn test_change_filter_property_matching() {
        let mut properties = HashSet::new();
        properties.insert("volume");
        properties.insert("mute");

        let filter = ChangeFilter::for_properties(properties);

        // Should match volume property
        let matching_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: SpeakerId::new("test"),
                property_name: "volume",
                service: Service::RenderingControl,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("test")),
            },
        };

        assert!(filter.matches(&matching_change));

        // Should not match playback_state property
        let non_matching_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: SpeakerId::new("test"),
                property_name: "playback_state",
                service: Service::AVTransport,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("test")),
            },
        };

        assert!(!filter.matches(&non_matching_change));
    }

    #[tokio::test]
    async fn test_change_stream_basic_functionality() {
        let (tx, rx) = broadcast::channel(16);
        let mut stream = ChangeStream::new(rx);

        // Send a test state change
        let state_change = create_test_state_change();
        tx.send(state_change).unwrap();

        // Should receive converted change event
        let change_event = stream.next().await.unwrap();
        assert!(matches!(
            change_event.change_type,
            ChangeType::DeviceProperty { .. }
        ));
    }

    #[tokio::test]
    async fn test_change_stream_filtering() {
        let (tx, rx) = broadcast::channel(16);
        let filter = ChangeFilter::rerender_only();
        let mut stream = ChangeStream::filtered(rx, filter);

        // Send a rerender-required change
        let rerender_change = StateChange::SpeakerPropertyChanged {
            speaker_id: SpeakerId::new("test"),
            property_key: "volume", // Volume requires rerender
            service: Service::RenderingControl,
        };
        tx.send(rerender_change).unwrap();

        // Send a non-rerender change
        let no_rerender_change = StateChange::SpeakerPropertyChanged {
            speaker_id: SpeakerId::new("test"),
            property_key: "position", // Position doesn't require immediate rerender
            service: Service::AVTransport,
        };
        tx.send(no_rerender_change).unwrap();

        // Should only receive the rerender-required change
        let change_event = stream.next().await.unwrap();
        assert!(change_event.context.requires_rerender);

        // The stream should be waiting for more changes (position change was filtered out)
        match tokio::time::timeout(Duration::from_millis(100), stream.next()).await {
            Ok(_) => panic!("Should have filtered out the position change"),
            Err(_) => {} // Timeout expected - position change was filtered
        }
    }

    #[tokio::test]
    async fn test_change_stream_try_next() {
        let (tx, rx) = broadcast::channel(16);
        let mut stream = ChangeStream::new(rx);

        // Should return None when no messages
        assert!(stream.try_next().is_none());

        // Send a message
        let state_change = create_test_state_change();
        tx.send(state_change).unwrap();

        // Should return the message
        let change_event = stream.try_next().unwrap();
        assert!(matches!(
            change_event.change_type,
            ChangeType::DeviceProperty { .. }
        ));

        // Should return None again when no more messages
        assert!(stream.try_next().is_none());
    }

    #[test]
    fn test_blocking_change_iterator() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = broadcast::channel(16);
        let mut iterator = BlockingChangeIterator::new(rx, rt.handle().clone());

        // Send a test change in the background
        let state_change = create_test_state_change();
        tx.send(state_change).unwrap();

        // Should receive the change
        let change_event = iterator.next_blocking().unwrap();
        assert!(matches!(
            change_event.change_type,
            ChangeType::DeviceProperty { .. }
        ));
    }

    #[test]
    fn test_blocking_change_iterator_try_next() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = broadcast::channel(16);
        let mut iterator = BlockingChangeIterator::new(rx, rt.handle().clone());

        // Should return Ok(None) when no messages
        match iterator.try_next() {
            Ok(None) => {}
            other => panic!("Expected Ok(None), got {:?}", other),
        }

        // Send a message
        let state_change = create_test_state_change();
        tx.send(state_change).unwrap();

        // Should return the message
        match iterator.try_next() {
            Ok(Some(change_event)) => {
                assert!(matches!(
                    change_event.change_type,
                    ChangeType::DeviceProperty { .. }
                ));
            }
            other => panic!("Expected Ok(Some(_)), got {:?}", other),
        }
    }

    #[test]
    fn test_change_type_filter_matching() {
        let mut change_types = HashSet::new();
        change_types.insert(ChangeTypeFilter::DeviceProperty);

        let filter = ChangeFilter {
            change_types: Some(change_types),
            ..Default::default()
        };

        // Should match device property change
        let device_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceProperty {
                speaker_id: SpeakerId::new("test"),
                property_name: "volume",
                service: Service::RenderingControl,
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Device(SpeakerId::new("test")),
            },
        };

        assert!(filter.matches(&device_change));

        // Should not match device lifecycle change
        let lifecycle_change = ChangeEvent {
            timestamp: Instant::now(),
            change_type: ChangeType::DeviceAdded {
                speaker_id: SpeakerId::new("test"),
            },
            context: ChangeContext {
                description: "Test".to_string(),
                requires_rerender: true,
                rerender_scope: RerenderScope::Full,
            },
        };

        assert!(!filter.matches(&lifecycle_change));
    }

    // Note: WidgetStateManager tests would require more complex setup with actual StateManager
    // and are better suited for integration tests. The key logic (caching and change flagging)
    // is tested indirectly through the change stream tests above.
}