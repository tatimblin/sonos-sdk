# Detailed Refactoring Plan: Option 1 - Module Split Within Existing Crate

Based on your complete codebase, here's a comprehensive plan to reorganize the `sonos-stream` crate into well-structured modules.

## Current State Analysis

Your crate currently has:
- **broker.rs** (590+ lines) - Contains broker core, renewal logic, event processing, lifecycle management, and shutdown
- **callback_server.rs** (400+ lines) - HTTP server and event routing
- **builder.rs** - Broker construction
- **types.rs** - Core domain types
- **strategy.rs** - Strategy trait
- **subscription.rs** - Subscription trait
- **event.rs** - Event types
- **error.rs** - Error types (assumed)
- **lib.rs** - Public API

## Proposed New Structure

```
sonos-stream/
├── lib.rs                      (public API, re-exports)
├── types.rs                    (unchanged)
├── error.rs                    (unchanged)
├── event.rs                    (unchanged)
├── builder.rs                  (unchanged)
│
├── broker/
│   ├── mod.rs                  (module re-exports)
│   ├── core.rs                 (EventBroker struct + public API)
│   ├── subscription_manager.rs (subscription lifecycle)
│   ├── renewal_manager.rs      (renewal task & retry logic)
│   └── event_processor.rs      (event routing & parsing)
│
├── callback/
│   ├── mod.rs                  (module re-exports)
│   ├── server.rs               (CallbackServer)
│   └── router.rs               (EventRouter)
│
├── strategy/
│   └── mod.rs                  (trait only, moved from strategy.rs)
│
└── subscription/
    └── mod.rs                  (trait only, moved from subscription.rs)
```

## Detailed Breakdown

### Phase 1: Split `broker.rs` (Core Refactoring)

#### 1.1 Create `broker/mod.rs`

```rust
//! Broker module for managing UPnP event subscriptions.

mod core;
mod subscription_manager;
mod renewal_manager;
mod event_processor;

pub use core::EventBroker;
pub use subscription_manager::{ActiveSubscription, SubscriptionManager};
pub use renewal_manager::RenewalManager;
pub use event_processor::EventProcessor;
```

#### 1.2 Create `broker/core.rs`

**Responsibilities:**
- Main `EventBroker` struct definition
- Public API methods: `event_stream()`, `subscribe()`, `unsubscribe()`, `shutdown()`
- Delegate actual work to manager components

**What moves here from broker.rs:**
- `EventBroker` struct definition (simplified)
- `new()` constructor
- `event_stream()` method
- High-level `subscribe()` and `unsubscribe()` methods (with delegation)
- `shutdown()` orchestration method

**Size estimate:** ~150 lines

```rust
//! Core EventBroker implementation.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::callback_server::CallbackServer;
use crate::event::Event;
use crate::strategy::SubscriptionStrategy;
use crate::types::{BrokerConfig, ServiceType, SubscriptionKey};

use super::{ActiveSubscription, EventProcessor, RenewalManager, SubscriptionManager};

/// Event broker for managing UPnP event subscriptions.
pub struct EventBroker {
    /// Shared subscription state
    subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
    /// Registered strategies
    strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
    /// Callback server
    callback_server: Arc<CallbackServer>,
    /// Event sender
    event_sender: mpsc::Sender<Event>,
    /// Event receiver (taken by event_stream())
    event_receiver: Option<mpsc::Receiver<Event>>,
    /// Configuration
    config: BrokerConfig,
    
    // Component managers
    subscription_manager: SubscriptionManager,
    renewal_manager: RenewalManager,
    event_processor: EventProcessor,
}

impl EventBroker {
    /// Create a new event broker (internal use only)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        strategies: HashMap<ServiceType, Box<dyn SubscriptionStrategy>>,
        callback_server: CallbackServer,
        config: BrokerConfig,
        event_sender: mpsc::Sender<Event>,
        event_receiver: mpsc::Receiver<Event>,
        renewal_manager: RenewalManager,
        event_processor: EventProcessor,
    ) -> Self {
        let subscriptions = Arc::new(RwLock::new(HashMap::new()));
        let strategies = Arc::new(strategies);
        
        let subscription_manager = SubscriptionManager::new(
            subscriptions.clone(),
            strategies.clone(),
            Arc::new(callback_server),
            event_sender.clone(),
        );
        
        Self {
            subscriptions,
            strategies,
            callback_server: subscription_manager.callback_server(),
            event_sender,
            event_receiver: Some(event_receiver),
            config,
            subscription_manager,
            renewal_manager,
            event_processor,
        }
    }

    /// Get the event stream receiver
    pub fn event_stream(&mut self) -> mpsc::Receiver<Event> {
        self.event_receiver
            .take()
            .expect("event_stream() can only be called once")
    }

    /// Subscribe to a service
    pub async fn subscribe(
        &self,
        speaker: &crate::types::Speaker,
        service_type: ServiceType,
    ) -> crate::error::Result<()> {
        self.subscription_manager
            .subscribe(speaker, service_type, &self.config)
            .await
    }

    /// Unsubscribe from a service
    pub async fn unsubscribe(
        &self,
        speaker: &crate::types::Speaker,
        service_type: ServiceType,
    ) -> crate::error::Result<()> {
        self.subscription_manager
            .unsubscribe(speaker, service_type)
            .await
    }

    /// Shutdown the broker
    pub async fn shutdown(mut self) -> crate::error::Result<()> {
        // Shutdown components in order
        self.renewal_manager.shutdown().await?;
        self.event_processor.shutdown().await?;
        self.subscription_manager.shutdown_all().await?;
        
        // Shutdown callback server
        match Arc::try_unwrap(self.callback_server) {
            Ok(server) => server.shutdown().await.map_err(|e| {
                crate::error::BrokerError::ShutdownError(format!(
                    "Failed to shutdown callback server: {e}"
                ))
            })?,
            Err(_) => {
                eprintln!("Warning: Callback server has multiple references");
            }
        }
        
        // Close event channels
        drop(self.event_sender);
        drop(self.event_receiver);
        
        Ok(())
    }
}
```

#### 1.3 Create `broker/subscription_manager.rs`

**Responsibilities:**
- Manage subscription lifecycle
- Handle subscribe/unsubscribe operations
- Maintain subscription state
- Interact with callback server for registration

**What moves here from broker.rs:**
- `ActiveSubscription` struct
- Subscription creation logic from `subscribe()`
- Subscription removal logic from `unsubscribe()`
- Subscription validation logic

**Size estimate:** ~200 lines

```rust
//! Subscription lifecycle management.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};

use crate::callback_server::CallbackServer;
use crate::error::{BrokerError, Result};
use crate::event::Event;
use crate::strategy::SubscriptionStrategy;
use crate::subscription::Subscription;
use crate::types::{BrokerConfig, ServiceType, Speaker, SubscriptionConfig, SubscriptionKey};

/// Active subscription state tracked by the broker
pub struct ActiveSubscription {
    pub key: SubscriptionKey,
    pub subscription: Box<dyn Subscription>,
    pub created_at: SystemTime,
    pub last_event: Option<SystemTime>,
}

impl ActiveSubscription {
    pub fn new(key: SubscriptionKey, subscription: Box<dyn Subscription>) -> Self {
        Self {
            key,
            subscription,
            created_at: SystemTime::now(),
            last_event: None,
        }
    }

    pub fn mark_event_received(&mut self) {
        self.last_event = Some(SystemTime::now());
    }
}

/// Manages subscription lifecycle operations
pub struct SubscriptionManager {
    subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
    strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
    callback_server: Arc<CallbackServer>,
    event_sender: mpsc::Sender<Event>,
}

impl SubscriptionManager {
    pub fn new(
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
        callback_server: Arc<CallbackServer>,
        event_sender: mpsc::Sender<Event>,
    ) -> Self {
        Self {
            subscriptions,
            strategies,
            callback_server,
            event_sender,
        }
    }

    pub fn callback_server(&self) -> Arc<CallbackServer> {
        self.callback_server.clone()
    }

    pub fn subscriptions(&self) -> Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>> {
        self.subscriptions.clone()
    }

    /// Subscribe to a service
    pub async fn subscribe(
        &self,
        speaker: &Speaker,
        service_type: ServiceType,
        config: &BrokerConfig,
    ) -> Result<()> {
        let key = SubscriptionKey::new(speaker.id.clone(), service_type);

        // Check for duplicates
        {
            let subs = self.subscriptions.read().await;
            if subs.contains_key(&key) {
                return Err(BrokerError::SubscriptionAlreadyExists {
                    speaker_id: speaker.id.clone(),
                    service_type,
                });
            }
        }

        // Look up strategy
        let strategy = self
            .strategies
            .get(&service_type)
            .ok_or(BrokerError::NoStrategyForService(service_type))?;

        // Create subscription config
        let callback_url = self.callback_server.base_url().to_string();
        let sub_config = SubscriptionConfig::new(
            config.subscription_timeout.as_secs() as u32,
            callback_url.clone(),
        );

        // Create subscription via strategy
        match strategy.create_subscription(speaker, callback_url, &sub_config) {
            Ok(subscription) => {
                let subscription_id = subscription.subscription_id().to_string();

                // Register with callback server
                self.callback_server
                    .register_subscription(
                        subscription_id.clone(),
                        speaker.id.clone(),
                        service_type,
                    )
                    .await;

                // Store subscription
                let active_sub = ActiveSubscription::new(key.clone(), subscription);
                {
                    let mut subs = self.subscriptions.write().await;
                    subs.insert(key, active_sub);
                }

                // Emit success event
                let _ = self
                    .event_sender
                    .send(Event::SubscriptionEstablished {
                        speaker_id: speaker.id.clone(),
                        service_type,
                        subscription_id,
                    })
                    .await;

                Ok(())
            }
            Err(e) => {
                // Emit failure event
                let error_msg = e.to_string();
                let _ = self
                    .event_sender
                    .send(Event::SubscriptionFailed {
                        speaker_id: speaker.id.clone(),
                        service_type,
                        error: error_msg.clone(),
                    })
                    .await;

                Err(BrokerError::StrategyError(e))
            }
        }
    }

    /// Unsubscribe from a service
    pub async fn unsubscribe(
        &self,
        speaker: &Speaker,
        service_type: ServiceType,
    ) -> Result<()> {
        let key = SubscriptionKey::new(speaker.id.clone(), service_type);

        // Remove subscription
        let subscription_opt = {
            let mut subs = self.subscriptions.write().await;
            subs.remove(&key)
        };

        if let Some(mut active_sub) = subscription_opt {
            let subscription_id = active_sub.subscription.subscription_id().to_string();

            // Unsubscribe (log errors but don't fail)
            if let Err(e) = active_sub.subscription.unsubscribe() {
                eprintln!(
                    "Warning: Failed to unsubscribe {}/{:?}: {}",
                    speaker.id.as_str(),
                    service_type,
                    e
                );
            }

            // Unregister from callback server
            self.callback_server
                .unregister_subscription(&subscription_id)
                .await;

            // Emit removal event
            let _ = self
                .event_sender
                .send(Event::SubscriptionRemoved {
                    speaker_id: speaker.id.clone(),
                    service_type,
                })
                .await;
        }

        Ok(())
    }

    /// Shutdown all active subscriptions
    pub async fn shutdown_all(&self) -> Result<()> {
        let subscription_keys: Vec<_> = {
            let subs = self.subscriptions.read().await;
            subs.keys().cloned().collect()
        };

        for key in subscription_keys {
            let subscription_opt = {
                let mut subs = self.subscriptions.write().await;
                subs.remove(&key)
            };

            if let Some(mut active_sub) = subscription_opt {
                let subscription_id = active_sub.subscription.subscription_id().to_string();

                if let Err(e) = active_sub.subscription.unsubscribe() {
                    eprintln!(
                        "Warning: Failed to unsubscribe {}/{:?} during shutdown: {}",
                        key.speaker_id.as_str(),
                        key.service_type,
                        e
                    );
                }

                self.callback_server
                    .unregister_subscription(&subscription_id)
                    .await;
            }
        }

        // Clear map
        let mut subs = self.subscriptions.write().await;
        subs.clear();

        Ok(())
    }
}
```

#### 1.4 Create `broker/renewal_manager.rs`

**Responsibilities:**
- Background renewal task
- Retry logic with exponential backoff
- Subscription expiration handling

**What moves here from broker.rs:**
- `start_renewal_task()` static method
- `check_and_renew_subscriptions()` static method
- `renew_subscription_with_retry()` static method
- `handle_subscription_expiration()` static method

**Size estimate:** ~200 lines

```rust
//! Automatic subscription renewal management.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::error::{BrokerError, Result};
use crate::event::Event;
use crate::types::{BrokerConfig, SubscriptionKey};

use super::ActiveSubscription;

/// Manages automatic renewal of subscriptions
pub struct RenewalManager {
    background_task: Option<JoinHandle<()>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl RenewalManager {
    /// Start the renewal manager
    pub fn start(
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
        config: BrokerConfig,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        
        let background_task = Self::start_renewal_task(
            subscriptions,
            event_sender,
            shutdown_rx,
            config,
        );
        
        Self {
            background_task: Some(background_task),
            shutdown_tx: Some(shutdown_tx),
        }
    }

    /// Shutdown the renewal manager
    pub async fn shutdown(mut self) -> Result<()> {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Wait for task to complete
        if let Some(task) = self.background_task.take() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                task,
            ).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => {
                    eprintln!("Warning: Renewal task panicked: {e}");
                    Ok(())
                }
                Err(_) => Err(BrokerError::ShutdownError(
                    "Renewal task shutdown timed out".to_string(),
                )),
            }
        } else {
            Ok(())
        }
    }

    /// Start the background renewal task
    fn start_renewal_task(
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
        mut shutdown_rx: mpsc::Receiver<()>,
        config: BrokerConfig,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut renewal_interval = 
                tokio::time::interval(std::time::Duration::from_secs(60));

            loop {
                tokio::select! {
                    _ = renewal_interval.tick() => {
                        Self::check_and_renew_subscriptions(
                            &subscriptions,
                            &event_sender,
                            &config,
                        ).await;
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        })
    }

    /// Check all subscriptions and renew those that need it
    async fn check_and_renew_subscriptions(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
        config: &BrokerConfig,
    ) {
        // Get subscriptions needing renewal
        let subscriptions_to_renew: Vec<SubscriptionKey> = {
            let subs = subscriptions.read().await;
            subs.iter()
                .filter_map(|(key, active_sub)| {
                    if active_sub.subscription.is_active() {
                        if let Some(time_until) = active_sub.subscription.time_until_renewal() {
                            if time_until <= config.renewal_threshold {
                                return Some(key.clone());
                            }
                        }
                    }
                    None
                })
                .collect()
        };

        // Renew each subscription
        for key in subscriptions_to_renew {
            Self::renew_subscription_with_retry(
                subscriptions,
                &key,
                event_sender,
                config,
            ).await;
        }
    }

    /// Renew a subscription with retry logic
    async fn renew_subscription_with_retry(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        key: &SubscriptionKey,
        event_sender: &mpsc::Sender<Event>,
        config: &BrokerConfig,
    ) {
        let mut attempt = 0;
        let max_attempts = config.max_retry_attempts;
        let base_backoff = config.retry_backoff_base;

        loop {
            // Try renewal
            let renewal_result = {
                let mut subs = subscriptions.write().await;
                if let Some(active_sub) = subs.get_mut(key) {
                    active_sub.subscription.renew()
                } else {
                    return;
                }
            };

            match renewal_result {
                Ok(()) => {
                    // Success
                    let _ = event_sender
                        .send(Event::SubscriptionRenewed {
                            speaker_id: key.speaker_id.clone(),
                            service_type: key.service_type,
                        })
                        .await;
                    return;
                }
                Err(e) => {
                    attempt += 1;

                    // Check for non-retryable errors
                    if matches!(e, crate::error::SubscriptionError::Expired) {
                        Self::handle_subscription_expiration(
                            subscriptions,
                            key,
                            event_sender,
                        ).await;
                        return;
                    }

                    // Check retry limit
                    if attempt >= max_attempts {
                        Self::handle_subscription_expiration(
                            subscriptions,
                            key,
                            event_sender,
                        ).await;
                        return;
                    }

                    // Exponential backoff
                    let backoff = base_backoff * 2_u32.pow(attempt - 1);
                    
                    eprintln!(
                        "Renewal failed for {}/{:?} (attempt {}/{}): {}. Retrying in {:?}...",
                        key.speaker_id.as_str(),
                        key.service_type,
                        attempt,
                        max_attempts,
                        e,
                        backoff
                    );

                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    /// Handle subscription expiration
    async fn handle_subscription_expiration(
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        key: &SubscriptionKey,
        event_sender: &mpsc::Sender<Event>,
    ) {
        // Remove subscription
        let mut subs = subscriptions.write().await;
        subs.remove(key);
        drop(subs);

        // Emit expiration event
        let _ = event_sender
            .send(Event::SubscriptionExpired {
                speaker_id: key.speaker_id.clone(),
                service_type: key.service_type,
            })
            .await;
    }
}
```

#### 1.5 Create `broker/event_processor.rs`

**Responsibilities:**
- Background event processing task
- Route raw events to strategies
- Parse events and emit ServiceEvent
- Handle parse errors

**What moves here from broker.rs:**
- `start_event_processing_task()` static method
- `process_raw_event()` static method

**Size estimate:** ~150 lines

```rust
//! Event processing and routing.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::callback_server::RawEvent;
use crate::error::{BrokerError, Result};
use crate::event::Event;
use crate::strategy::SubscriptionStrategy;
use crate::types::{ServiceType, SubscriptionKey};

use super::ActiveSubscription;

/// Processes incoming raw events and routes them to strategies
pub struct EventProcessor {
    processing_task: Option<JoinHandle<()>>,
}

impl EventProcessor {
    /// Start the event processor
    pub fn start(
        raw_event_rx: mpsc::UnboundedReceiver<RawEvent>,
        strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
    ) -> Self {
        let processing_task = Self::start_processing_task(
            raw_event_rx,
            strategies,
            subscriptions,
            event_sender,
        );
        
        Self {
            processing_task: Some(processing_task),
        }
    }

    /// Shutdown the event processor
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(task) = self.processing_task.take() {
            // Abort the task since it waits on channel
            task.abort();
            
            match tokio::time::timeout(
                std::time::Duration::from_secs(2),
                task,
            ).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) if e.is_cancelled() => Ok(()),
                Ok(Err(e)) => {
                    eprintln!("Warning: Event processor panicked: {e}");
                    Ok(())
                }
                Err(_) => {
                    eprintln!("Warning: Event processor shutdown timed out");
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    /// Start the processing task
    fn start_processing_task(
        mut raw_event_rx: mpsc::UnboundedReceiver<RawEvent>,
        strategies: Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
        subscriptions: Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: mpsc::Sender<Event>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(raw_event) = raw_event_rx.recv().await {
                Self::process_raw_event(
                    raw_event,
                    &strategies,
                    &subscriptions,
                    &event_sender,
                )
                .await;
            }
        })
    }

    /// Process a single raw event
    async fn process_raw_event(
        raw_event: RawEvent,
        strategies: &Arc<HashMap<ServiceType, Box<dyn SubscriptionStrategy>>>,
        subscriptions: &Arc<RwLock<HashMap<SubscriptionKey, ActiveSubscription>>>,
        event_sender: &mpsc::Sender<Event>,
    ) {
        let speaker_id = raw_event.speaker_id.clone();
        let service_type = raw_event.service_type;
        let event_xml = raw_event.event_xml;

        // Look up strategy
        let strategy = match strategies.get(&service_type) {
            Some(s) => s,
            None => {
                let _ = event_sender
                    .send(Event::ParseError {
                        speaker_id,
                        service_type,
                        error: format!("No strategy registered for service type: {service_type:?}"),
                    })
                    .await;
                return;
            }
        };

        // Parse event
        match strategy.parse_event(&speaker_id, &event_xml) {
            Ok(parsed_events) => {
                // Emit ServiceEvent for each parsed event
                for parsed_event in parsed_events {
                    let _ = event_sender
                        .send(Event::ServiceEvent {
                            speaker_id: speaker_id.clone(),
                            service_type,
                            event: parsed_event,
                        })
                        .await;
                }

                // Update last event timestamp
                let key = SubscriptionKey::new(speaker_id, service_type);
                let mut subs = subscriptions.write().await;
                if let Some(active_sub) = subs.get_mut(&key) {
                    active_sub.mark_event_received();
                }
            }
            Err(e) => {
                let _ = event_sender
                    .send(Event::ParseError {
                        speaker_id,
                        service_type,
                        error: e.to_string(),
                    })
                    .await;
            }
        }
    }
}
```

### Phase 2: Split `callback_server.rs`

#### 2.1 Create `callback/mod.rs`

```rust
//! HTTP callback server for receiving UPnP notifications.

mod server;
mod router;

pub use server::CallbackServer;
pub use router::{EventRouter, RawEvent};
```

#### 2.2 Create `callback/router.rs`

**What moves here from callback_server.rs:**
- `RawEvent` struct
- `EventRouter` struct and implementation

**Size estimate:** ~100 lines

#### 2.3 Create `callback/server.rs`

**What moves here from callback_server.rs:**
- `CallbackServer` struct and implementation
- HTTP server logic
- Port binding logic
- IP detection logic

**Size estimate:** ~300 lines

### Phase 3: Simplify Existing Files

#### 3.1 Update `strategy/mod.rs`

Move the entire contents of `strategy.rs` here since it only contains the trait.

#### 3.2 Update `subscription/mod.rs`

Move the entire contents of `subscription.rs` here since it only contains the trait.

### Phase 4: Update `builder.rs`

Update the builder to use the new module structure:

```rust
use crate::broker::{EventProcessor, RenewalManager};
use crate::callback::CallbackServer;

// In build() method:
let renewal_manager = RenewalManager::start(
    subscriptions.clone(),
    event_tx.clone(),
    self.config.clone(),
);

let event_processor = EventProcessor::start(
    raw_event_rx,
    strategies_arc.clone(),
    subscriptions.clone(),
    event_tx.clone(),
);

EventBroker::new(
    strategies,
    callback_server,
    config,
    event_tx,
    event_rx,
    renewal_manager,
    event_processor,
)
```

### Phase 5: Update `lib.rs`

Update the module declarations and re-exports:

```rust
mod broker;
mod builder;
mod callback;
mod error;
mod event;
mod strategy;
mod subscription;
mod types;

// Re-export main types
pub use broker::{ActiveSubscription, EventBroker};
pub use builder::EventBrokerBuilder;
pub use callback::{CallbackServer, EventRouter, RawEvent};
pub use error::{BrokerError, StrategyError, SubscriptionError};
pub use event::{Event, ParsedEvent};
pub use strategy::SubscriptionStrategy;
pub use subscription::Subscription;
pub use types::{
    BrokerConfig, ServiceType, Speaker, SpeakerId,
    SubscriptionConfig, SubscriptionKey, SubscriptionScope,
};
```

### Phase 6: Move Tests

#### 6.1 Test Organization

- Keep `ActiveSubscription` tests in `broker/subscription_manager.rs`
- Keep renewal tests in `broker/renewal_manager.rs`
- Keep event processing tests in `broker/event_processor.rs`
- Keep integration tests in `broker/core.rs`
- Keep callback server tests in `callback/server.rs`
- Keep router tests in `callback/router.rs`

## Implementation Checklist

- [ ] **Phase 1.1:** Create `broker/mod.rs`
- [ ] **Phase 1.2:** Create `broker/core.rs` and move core broker logic
- [ ] **Phase 1.3:** Create `broker/subscription_manager.rs` and move subscription logic
- [ ] **Phase 1.4:** Create `broker/renewal_manager.rs` and move renewal logic
- [ ] **Phase 1.5:** Create `broker/event_processor.rs` and move event processing logic
- [ ] **Phase 2.1:** Create `callback/mod.rs`
- [ ] **Phase 2.2:** Create `callback/router.rs` and move router logic
- [ ] **Phase 2.3:** Create `callback/server.rs` and move server logic
- [ ] **Phase 3.1:** Convert `strategy.rs` to `strategy/mod.rs`
- [ ] **Phase 3.2:** Convert `subscription.rs` to `subscription/mod.rs`
- [ ] **Phase 4:** Update `builder.rs` to use new managers
- [ ] **Phase 5:** Update `lib.rs` with new module structure
- [ ] **Phase 6:** Move and verify all tests pass
- [ ] **Verification:** Run `cargo test`
- [ ] **Verification:** Run `cargo clippy`
- [ ] **Verification:** Run `cargo doc` and verify docs render correctly
- [ ] **Final:** Update any internal documentation

## Benefits of This Refactoring

1. **Clear Separation of Concerns**: Each module has a single, well-defined responsibility
2. **Easier Navigation**: Files are now 100-300 lines instead of 590+
3. **Better Testability**: Each component can be tested in isolation
4. **Maintainability**: Easier to locate and modify specific functionality
5. **No Breaking Changes**: Public API remains identical
6. **Better Documentation**: Each module can have focused documentation
7. **Future-Proof**: Easy to extract to separate crates later if needed

## Estimated Timeline

- **Phase 1 (Broker split):** 3-4 hours
- **Phase 2 (Callback split):** 1-2 hours
- **Phase 3 (Strategy/Subscription):** 30 minutes
- **Phase 4 (Builder update):** 30 minutes
- **Phase 5 (Lib.rs update):** 30 minutes
- **Phase 6 (Test migration):** 1-2 hours
- **Total:** 7-10 hours

Would you like me to generate the complete code for any specific phase?