How Watchable Properties Work

  Here's the flow:

  1. Data Structures

  StateManager
  ├── store: RwLock<StateStore>           # Property values
  │   └── speaker_props: HashMap<SpeakerId, PropertyBag>
  │       └── PropertyBag: HashMap<TypeId, Box<dyn Any>>  # Type-erased values
  │
  ├── watched: RwLock<HashSet<(SpeakerId, &'static str)>>  # Which properties to emit events for
  │
  ├── event_tx: mpsc::Sender<ChangeEvent>   # Send side
  └── event_rx: Arc<Mutex<Receiver>>        # Receive side (for iter())

  2. The Watch Flow

  speaker.volume.watch()
         │
         ▼
  PropertyHandle::watch()  (speaker.rs:102-105)
         │
         ├─► register_watch(speaker_id, "volume")
         │         │
         │         ▼
         │   watched.insert((speaker_id, "volume"))  # Now tracked
         │
         └─► return self.get()  # Return current cached value

  3. When a Property Changes

  manager.set_property(&speaker_id, Volume::new(75))
         │
         ▼
  StateManager::set_property()  (state.rs:330-342)
         │
         ├─► store.set::<Volume>(speaker_id, value)
         │         │
         │         ▼
         │   PropertyBag::set() returns true if value actually changed
         │
         └─► if changed: maybe_emit_change(speaker_id, "volume", Service::RenderingControl)
                    │
                    ▼
             is_watched = watched.contains((speaker_id, "volume"))
                    │
                    ▼ (if true)
             event_tx.send(ChangeEvent { speaker_id, property_key: "volume", ... })

  4. Consuming Events with iter()

  for event in manager.iter() { ... }
         │
         ▼
  ChangeIterator::next()
         │
         ▼
  event_rx.recv()  ← Blocks here until event arrives
         │
         ▼
  Returns ChangeEvent { speaker_id, property_key, service, timestamp }

  5. Key Points
  ┌───────────────┬────────────────────────────────────────────────────────────────────────┐
  │    Aspect     │                                Behavior                                │
  ├───────────────┼────────────────────────────────────────────────────────────────────────┤
  │ Filtering     │ Only watched properties emit events to iter()                          │
  ├───────────────┼────────────────────────────────────────────────────────────────────────┤
  │ Storage       │ Values stored regardless of watch status (get() works without watch()) │
  ├───────────────┼────────────────────────────────────────────────────────────────────────┤
  │ Channel       │ std::sync::mpsc - unbounded, blocking                                  │
  ├───────────────┼────────────────────────────────────────────────────────────────────────┤
  │ Type erasure  │ PropertyBag uses TypeId + Box<dyn Any> for type-safe storage           │
  ├───────────────┼────────────────────────────────────────────────────────────────────────┤
  │ No duplicates │ PropertyBag::set() only returns true if value actually changed         │
  └───────────────┴────────────────────────────────────────────────────────────────────────┘
  6. Example

  let manager = StateManager::new()?;
  manager.add_devices(devices)?;

  let speakers = manager.speakers();
  let speaker = &speakers[0];

  // This does NOT emit events - just tracks that we care
  speaker.volume.watch()?;
  speaker.mute.watch()?;

  // Now when values change, iter() will emit events
  // (Currently nothing updates values since background worker isn't wired up)

  for event in manager.iter() {
      match event.property_key {
          "volume" => {
              let vol = speaker.volume.get();
              println!("Volume changed: {:?}", vol);
          }
          "mute" => {
              let muted = speaker.mute.get();
              println!("Mute changed: {:?}", muted);
          }
          _ => {}
      }
  }