//! Property handle for accessing speaker properties.
//!
//! Provides `get()` and `watch()` methods for clean property access:
//!
//! ```rust,ignore
//! let volume = speaker.volume.get();           // Sync read
//! let volume = speaker.volume.watch().await?;  // Watch (cached subscription)
//! ```

use std::marker::PhantomData;
use std::sync::Arc;

use crate::model::SpeakerId;
use crate::property::Property;
use crate::watch_cache::WatchCache;
use crate::reactive::StateManager;
use crate::Result;

/// Handle for accessing a specific property on a speaker.
///
/// Provides two access patterns:
/// - `get()`: Synchronous read of current value (no subscription)
/// - `watch()`: Ensures subscription exists and returns current value
///
/// The subscription is managed internally with automatic cleanup after
/// 5 seconds of inactivity.
pub struct PropertyHandle<P: Property> {
    /// Speaker this property belongs to
    pub(crate) speaker_id: SpeakerId,
    /// Reference to the state manager (for property access)
    pub(crate) state_manager: Arc<StateManager>,
    /// Reference to the shared watch cache
    pub(crate) watch_cache: Arc<WatchCache>,
    /// Phantom data for property type
    pub(crate) _phantom: PhantomData<P>,
}

impl<P: Property> PropertyHandle<P> {
    /// Create a new property handle
    pub(crate) fn new(
        speaker_id: SpeakerId,
        state_manager: Arc<StateManager>,
        watch_cache: Arc<WatchCache>,
    ) -> Self {
        Self {
            speaker_id,
            state_manager,
            watch_cache,
            _phantom: PhantomData,
        }
    }

    /// Get current property value (synchronous, no subscription).
    ///
    /// Returns the current value from the state store without creating
    /// or affecting any subscriptions. Use this for one-off reads.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let volume = speaker.volume.get();
    /// if let Some(vol) = volume {
    ///     println!("Volume: {}%", vol.0);
    /// }
    /// ```
    pub fn get(&self) -> Option<P> {
        self.state_manager.get_property::<P>(&self.speaker_id)
    }

    /// Watch for property value (ensures subscription exists).
    ///
    /// This method:
    /// 1. Checks if a subscription already exists in the cache
    /// 2. If not, creates a new UPnP subscription (~100-500ms)
    /// 3. Returns the current value
    ///
    /// The subscription stays active for 5 seconds after last access,
    /// allowing fast navigation without re-subscribing.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In render function
    /// let volume = speaker.volume.watch().await?;
    /// if let Some(vol) = volume {
    ///     render_volume_bar(vol.0);
    /// }
    /// ```
    ///
    /// # Performance
    ///
    /// - Cache hit: ~1us (instant)
    /// - Cache miss: ~100-500ms (UPnP subscription)
    pub async fn watch(&self) -> Result<Option<P>> {
        // Check if watch exists in cache
        if self.watch_cache.has_watch(&self.speaker_id, P::KEY).await {
            // Cache hit - just return current value from state store
            return Ok(self.state_manager.get_property::<P>(&self.speaker_id));
        }

        // Cache miss - create new watch
        // We use a wrapper struct to store the watcher in the cache
        struct WatcherWrapper<P: Property> {
            _marker: PhantomData<P>,
        }

        // Create the subscription through StateManager
        let watcher = self.state_manager.watch_property::<P>(self.speaker_id.clone()).await?;
        let current_value = watcher.current();

        // Store a marker in the cache to track that we have a subscription
        // The actual PropertyWatcher is stored to keep the subscription alive
        self.watch_cache
            .get_or_watch::<P, crate::reactive::PropertyWatcher<P>, _, _>(
                self.speaker_id.clone(),
                P::KEY,
                || async { Ok((watcher, current_value.clone())) },
            )
            .await?;

        Ok(current_value)
    }

    /// Get the speaker ID this handle belongs to
    pub fn speaker_id(&self) -> &SpeakerId {
        &self.speaker_id
    }

    /// Get the property key
    pub fn property_key(&self) -> &'static str {
        P::KEY
    }
}

impl<P: Property> Clone for PropertyHandle<P> {
    fn clone(&self) -> Self {
        Self {
            speaker_id: self.speaker_id.clone(),
            state_manager: Arc::clone(&self.state_manager),
            watch_cache: Arc::clone(&self.watch_cache),
            _phantom: PhantomData,
        }
    }
}

impl<P: Property> std::fmt::Debug for PropertyHandle<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyHandle")
            .field("speaker_id", &self.speaker_id)
            .field("property_key", &P::KEY)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    // Tests would require mocking StateManager and WatchCache
    // which is complex due to the async nature. Integration tests
    // in the example are more practical for this module.
}
