//! SyncWatcher - blocking wrapper for watch channels
//!
//! Provides a synchronous interface for code that can't use async/await,
//! such as CLI applications or tests.

use tokio::sync::watch;

use crate::property::Property;

/// A synchronous wrapper around a watch receiver
///
/// Useful for CLI applications or other synchronous code that needs
/// to watch for property changes.
///
/// # Example
///
/// ```rust,ignore
/// let watcher = SyncWatcher::new(rx, runtime.handle().clone());
///
/// // Get current value (instant)
/// if let Some(volume) = watcher.get() {
///     println!("Current volume: {}%", volume.0);
/// }
///
/// // Block until value changes
/// loop {
///     if let Some(new_volume) = watcher.wait() {
///         println!("Volume changed to: {}%", new_volume.0);
///     }
/// }
/// ```
pub struct SyncWatcher<P: Property> {
    rx: watch::Receiver<Option<P>>,
    rt: tokio::runtime::Handle,
}

impl<P: Property> SyncWatcher<P> {
    /// Create a new SyncWatcher
    ///
    /// # Arguments
    ///
    /// * `rx` - The watch receiver to wrap
    /// * `rt` - A handle to a Tokio runtime for blocking operations
    pub fn new(rx: watch::Receiver<Option<P>>, rt: tokio::runtime::Handle) -> Self {
        Self { rx, rt }
    }

    /// Get the current value without blocking
    ///
    /// Returns `None` if no value has been set yet.
    pub fn get(&self) -> Option<P> {
        self.rx.borrow().clone()
    }

    /// Block until the value changes, then return the new value
    ///
    /// Returns `None` if the channel is closed.
    pub fn wait(&mut self) -> Option<P> {
        self.rt.block_on(self.rx.changed()).ok()?;
        self.rx.borrow().clone()
    }

    /// Block until the value changes or timeout expires
    ///
    /// Returns `None` if the channel is closed or the timeout expires.
    pub fn wait_timeout(&mut self, timeout: std::time::Duration) -> Option<P> {
        let result = self.rt.block_on(async {
            tokio::time::timeout(timeout, self.rx.changed()).await
        });

        match result {
            Ok(Ok(())) => self.rx.borrow().clone(),
            _ => None,
        }
    }

    /// Check if the channel has been modified since last check
    ///
    /// Unlike `wait()`, this doesn't block. Returns true if a new value
    /// is available to read via `get()`.
    pub fn has_changed(&self) -> bool {
        self.rx.has_changed().unwrap_or(false)
    }
}

impl<P: Property> Clone for SyncWatcher<P> {
    fn clone(&self) -> Self {
        Self {
            rx: self.rx.clone(),
            rt: self.rt.clone(),
        }
    }
}

/// Extension trait to create SyncWatcher from StateStore
///
/// Requires a runtime handle because blocking operations need a runtime.
pub trait SyncWatchExt {
    /// Create a sync watcher for a speaker property
    fn sync_watch<P: Property>(
        &self,
        id: &crate::model::SpeakerId,
        rt: tokio::runtime::Handle,
    ) -> SyncWatcher<P>;

    /// Create a sync watcher for a group property
    fn sync_watch_group<P: Property>(
        &self,
        id: &crate::model::GroupId,
        rt: tokio::runtime::Handle,
    ) -> SyncWatcher<P>;

    /// Create a sync watcher for a system property
    fn sync_watch_system<P: Property>(&self, rt: tokio::runtime::Handle) -> SyncWatcher<P>;
}

impl SyncWatchExt for crate::store::StateStore {
    fn sync_watch<P: Property>(
        &self,
        id: &crate::model::SpeakerId,
        rt: tokio::runtime::Handle,
    ) -> SyncWatcher<P> {
        let rx = self.watch::<P>(id);
        SyncWatcher::new(rx, rt)
    }

    fn sync_watch_group<P: Property>(
        &self,
        id: &crate::model::GroupId,
        rt: tokio::runtime::Handle,
    ) -> SyncWatcher<P> {
        let rx = self.watch_group::<P>(id);
        SyncWatcher::new(rx, rt)
    }

    fn sync_watch_system<P: Property>(&self, rt: tokio::runtime::Handle) -> SyncWatcher<P> {
        let rx = self.watch_system::<P>();
        SyncWatcher::new(rx, rt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SpeakerId, SpeakerInfo};
    use crate::property::Volume;
    use crate::store::StateStore;

    fn create_test_store() -> StateStore {
        let store = StateStore::new();
        store.add_speaker(SpeakerInfo {
            id: SpeakerId::new("RINCON_123"),
            name: "Test".to_string(),
            room_name: "Test".to_string(),
            ip_address: "192.168.1.100".parse().unwrap(),
            port: 1400,
            model_name: "Test".to_string(),
            software_version: "1.0".to_string(),
            satellites: vec![],
        });
        store
    }

    #[tokio::test]
    async fn test_sync_watcher_get() {
        let store = create_test_store();
        let id = SpeakerId::new("RINCON_123");

        // Set initial value
        store.set(&id, Volume::new(50));

        // Create watcher
        let rt = tokio::runtime::Handle::current();
        let watcher: SyncWatcher<Volume> = store.sync_watch(&id, rt);

        // Get current value
        assert_eq!(watcher.get(), Some(Volume::new(50)));
    }

    #[test]
    fn test_sync_watcher_wait_timeout() {
        // Create a runtime for this test (can't use tokio::test with block_on)
        let rt = tokio::runtime::Runtime::new().unwrap();

        let store = create_test_store();
        let id = SpeakerId::new("RINCON_123");

        store.set(&id, Volume::new(50));

        let mut watcher: SyncWatcher<Volume> = store.sync_watch(&id, rt.handle().clone());

        // Wait with short timeout - should timeout since no change
        let result = watcher.wait_timeout(std::time::Duration::from_millis(10));
        assert!(result.is_none());
    }

    #[test]
    fn test_sync_watcher_detects_change() {
        // Create a runtime for this test (can't use tokio::test with block_on)
        let rt = tokio::runtime::Runtime::new().unwrap();

        let store = create_test_store();
        let id = SpeakerId::new("RINCON_123");

        store.set(&id, Volume::new(50));

        let store_clone = store.clone();
        let id_clone = id.clone();

        let mut watcher: SyncWatcher<Volume> = store.sync_watch(&id, rt.handle().clone());

        // Spawn task in a separate thread to change value
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            store_clone.set(&id_clone, Volume::new(75));
        });

        // Wait for change
        let result = watcher.wait_timeout(std::time::Duration::from_millis(100));
        assert_eq!(result, Some(Volume::new(75)));

        handle.join().unwrap();
    }
}
