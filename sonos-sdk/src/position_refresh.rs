//! Keeps watched playback positions current.
//!
//! Sonos events never carry `RelTime`, so a cached [`Position`] is as old as the
//! last `fetch()`. `sonos-state` knows when a watched position has jumped (track
//! or transport state changed, or a new watcher arrived) and calls the hook
//! installed here; the hook queues the speaker for a background thread that
//! fetches `GetPositionInfo`. The fetch writes through the normal store path,
//! so watchers get a `ChangeEvent` like any other update.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use sonos_api::SonosClient;
use sonos_state::{Position, PositionRefreshFn, SpeakerId, StateManager};

use crate::property::{PropertyHandle, SpeakerContext};

/// Install the position refresh hook on `state_manager`.
///
/// The worker thread is spawned on the first request, so a system that never
/// watches a position never pays for it. The hook lives on the manager and owns
/// the only `Sender`; the thread holds a `Weak` manager, so dropping the system
/// closes the channel and the thread exits instead of keeping the manager alive.
pub(crate) fn install(state_manager: &Arc<StateManager>, api_client: &SonosClient) {
    let weak_sm = Arc::downgrade(state_manager);
    let api_client = api_client.clone();
    let sender: Mutex<Option<Sender<SpeakerId>>> = Mutex::new(None);

    let hook: PositionRefreshFn = Arc::new(move |speaker_id: &SpeakerId| {
        let mut sender = sender.lock().unwrap_or_else(|e| e.into_inner());
        let tx = sender.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel();
            spawn_worker(rx, weak_sm.clone(), api_client.clone());
            tx
        });
        // A closed channel means the worker could not start or the system is
        // shutting down; either way there is nothing left to refresh.
        let _ = tx.send(speaker_id.clone());
    });

    state_manager.set_position_refresh(hook);
}

fn spawn_worker(rx: Receiver<SpeakerId>, state_manager: Weak<StateManager>, client: SonosClient) {
    let spawned = thread::Builder::new()
        .name("sonos-position-refresh".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                // One event can queue the same coordinator several times (a
                // track change carries both the track and the state); fetch once.
                let mut seen = HashSet::new();
                let pending: Vec<SpeakerId> = std::iter::once(first)
                    .chain(rx.try_iter())
                    .filter(|id| seen.insert(id.clone()))
                    .collect();

                let Some(sm) = state_manager.upgrade() else {
                    return;
                };
                for speaker_id in pending {
                    refresh(&sm, &client, speaker_id);
                }
            }
        });

    if let Err(e) = spawned {
        tracing::warn!("failed to start the position refresh thread: {e}");
    }
}

fn refresh(state_manager: &Arc<StateManager>, client: &SonosClient, speaker_id: SpeakerId) {
    let Some(ip) = state_manager.get_speaker_ip(&speaker_id) else {
        return;
    };
    let handle: PropertyHandle<Position> = PropertyHandle::new(SpeakerContext::new(
        speaker_id.clone(),
        ip,
        Arc::clone(state_manager),
        client.clone(),
    ));
    if let Err(e) = handle.fetch() {
        tracing::debug!(speaker = %speaker_id, "position refresh failed: {e}");
    }
}
