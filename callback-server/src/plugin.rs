//! Plugin system for extending callback server functionality.
//!
//! This module provides a pluggable architecture that allows extending the callback
//! server with additional features like firewall detection, logging, monitoring, etc.
//! Plugins are registered with a central registry and executed during server lifecycle events.

use async_trait::async_trait;
use thiserror::Error;

/// Errors that can occur during plugin operations.
#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Plugin initialization failed: {0}")]
    InitializationFailed(String),
    #[error("Plugin execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Plugin shutdown failed: {0}")]
    ShutdownFailed(String),
}

/// Context provided to plugins during execution.
///
/// Contains server information and resources that plugins might need during their lifecycle.
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// Server base URL (e.g., "http://192.168.1.100:3400")
    pub server_url: String,
    /// Server port number
    pub server_port: u16,
    /// Test endpoint path for firewall detection
    pub test_endpoint: String,
    /// HTTP client for making requests
    pub http_client: reqwest::Client,
}

impl PluginContext {
    /// Create a new plugin context with server information.
    pub fn new(server_url: String, server_port: u16) -> Self {
        Self {
            server_url,
            server_port,
            test_endpoint: "/firewall-test".to_string(),
            http_client: reqwest::Client::new(),
        }
    }

    /// Create a minimal plugin context for testing.
    pub fn minimal() -> Self {
        Self {
            server_url: "http://localhost:3400".to_string(),
            server_port: 3400,
            test_endpoint: "/firewall-test".to_string(),
            http_client: reqwest::Client::new(),
        }
    }
}

impl Default for PluginContext {
    fn default() -> Self {
        Self::minimal()
    }
}

/// Trait for implementing callback server plugins.
///
/// Plugins can extend the callback server with additional functionality
/// such as firewall detection, monitoring, logging, etc. They are executed
/// during server lifecycle events.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get the name of this plugin.
    fn name(&self) -> &'static str;

    /// Initialize the plugin with the given context.
    ///
    /// This is called during server startup and should perform any
    /// initialization work the plugin needs.
    async fn initialize(&mut self, context: &PluginContext) -> Result<(), PluginError>;

    /// Shutdown the plugin gracefully.
    ///
    /// This is called during server shutdown and should clean up any
    /// resources the plugin is using.
    async fn shutdown(&mut self) -> Result<(), PluginError>;

    /// Get a reference to the plugin as Any for downcasting.
    ///
    /// This enables type-specific operations on plugins when needed.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Registry for managing callback server plugins.
///
/// The registry maintains a collection of plugins and manages their lifecycle,
/// executing them during appropriate server events.
pub struct PluginRegistry {
    /// Collection of registered plugins
    plugins: Vec<Box<dyn Plugin>>,
    /// Whether plugins have been initialized
    initialized: bool,
}

impl PluginRegistry {
    /// Create a new empty plugin registry.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            initialized: false,
        }
    }

    /// Register a plugin with the registry.
    ///
    /// The plugin will be stored and executed during server lifecycle events.
    /// Plugins are initialized in the order they are registered.
    ///
    /// # Arguments
    ///
    /// * `plugin` - The plugin to register
    ///
    /// # Example
    ///
    /// ```
    /// use callback_server::plugin::{PluginRegistry, Plugin, PluginContext, PluginError};
    /// use async_trait::async_trait;
    ///
    /// struct MyPlugin;
    ///
    /// #[async_trait]
    /// impl Plugin for MyPlugin {
    ///     fn name(&self) -> &'static str { "my-plugin" }
    ///     async fn initialize(&mut self, _context: &PluginContext) -> Result<(), PluginError> { Ok(()) }
    ///     async fn shutdown(&mut self) -> Result<(), PluginError> { Ok(()) }
    /// }
    ///
    /// let mut registry = PluginRegistry::new();
    /// registry.register(Box::new(MyPlugin));
    /// ```
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    /// Initialize all registered plugins.
    ///
    /// This should be called during server startup. Plugins are initialized
    /// in the order they were registered. If any plugin fails to initialize,
    /// the error is logged but initialization continues for remaining plugins.
    ///
    /// # Arguments
    ///
    /// * `context` - The plugin context to provide to each plugin
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if initialization completes, even if some plugins failed.
    /// Individual plugin errors are logged but don't prevent overall initialization.
    pub async fn initialize_all(&mut self, context: &PluginContext) -> Result<(), PluginError> {
        if self.initialized {
            return Ok(());
        }

        for plugin in &mut self.plugins {
            match plugin.initialize(context).await {
                Ok(()) => {
                    eprintln!("✅ Plugin '{}' initialized successfully", plugin.name());
                }
                Err(e) => {
                    eprintln!("❌ Plugin '{}' initialization failed: {}", plugin.name(), e);
                    // Continue with other plugins even if one fails
                }
            }
        }

        self.initialized = true;
        Ok(())
    }

    /// Shutdown all registered plugins.
    ///
    /// This should be called during server shutdown. Plugins are shut down
    /// in reverse order of initialization. Errors are logged but don't prevent
    /// shutdown of remaining plugins.
    pub async fn shutdown_all(&mut self) -> Result<(), PluginError> {
        if !self.initialized {
            return Ok(());
        }

        // Shutdown in reverse order
        for plugin in self.plugins.iter_mut().rev() {
            match plugin.shutdown().await {
                Ok(()) => {
                    eprintln!("✅ Plugin '{}' shut down successfully", plugin.name());
                }
                Err(e) => {
                    eprintln!("❌ Plugin '{}' shutdown failed: {}", plugin.name(), e);
                    // Continue with other plugins even if one fails
                }
            }
        }

        self.initialized = false;
        Ok(())
    }

    /// Get the number of registered plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Check if plugins have been initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get the names of all registered plugins.
    pub fn plugin_names(&self) -> Vec<&'static str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    /// Get a reference to a plugin by name.
    pub fn get_plugin(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.iter().find(|p| p.name() == name).map(|p| p.as_ref())
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        name: &'static str,
        initialized: bool,
        should_fail_init: bool,
        should_fail_shutdown: bool,
    }

    impl TestPlugin {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                initialized: false,
                should_fail_init: false,
                should_fail_shutdown: false,
            }
        }

        fn with_init_failure(mut self) -> Self {
            self.should_fail_init = true;
            self
        }

        fn with_shutdown_failure(mut self) -> Self {
            self.should_fail_shutdown = true;
            self
        }
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn initialize(&mut self, _context: &PluginContext) -> Result<(), PluginError> {
            if self.should_fail_init {
                return Err(PluginError::InitializationFailed("Test failure".to_string()));
            }
            self.initialized = true;
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), PluginError> {
            if self.should_fail_shutdown {
                return Err(PluginError::ShutdownFailed("Test failure".to_string()));
            }
            self.initialized = false;
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn test_plugin_registry_basic_operations() {
        let mut registry = PluginRegistry::new();
        assert_eq!(registry.plugin_count(), 0);
        assert!(!registry.is_initialized());

        // Register plugins
        registry.register(Box::new(TestPlugin::new("plugin1")));
        registry.register(Box::new(TestPlugin::new("plugin2")));
        assert_eq!(registry.plugin_count(), 2);

        let names = registry.plugin_names();
        assert_eq!(names, vec!["plugin1", "plugin2"]);
    }

    #[tokio::test]
    async fn test_plugin_initialization_and_shutdown() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(TestPlugin::new("test-plugin")));

        let context = PluginContext::minimal();

        // Initialize plugins
        let result = registry.initialize_all(&context).await;
        assert!(result.is_ok());
        assert!(registry.is_initialized());

        // Shutdown plugins
        let result = registry.shutdown_all().await;
        assert!(result.is_ok());
        assert!(!registry.is_initialized());
    }

    #[tokio::test]
    async fn test_plugin_initialization_with_failures() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(TestPlugin::new("good-plugin")));
        registry.register(Box::new(TestPlugin::new("bad-plugin").with_init_failure()));
        registry.register(Box::new(TestPlugin::new("another-good-plugin")));

        let context = PluginContext::minimal();

        // Should succeed even with one plugin failing
        let result = registry.initialize_all(&context).await;
        assert!(result.is_ok());
        assert!(registry.is_initialized());
    }

    #[tokio::test]
    async fn test_plugin_shutdown_with_failures() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(TestPlugin::new("good-plugin")));
        registry.register(Box::new(TestPlugin::new("bad-plugin").with_shutdown_failure()));

        let context = PluginContext::minimal();

        // Initialize first
        registry.initialize_all(&context).await.unwrap();

        // Should succeed even with one plugin failing shutdown
        let result = registry.shutdown_all().await;
        assert!(result.is_ok());
        assert!(!registry.is_initialized());
    }

    #[tokio::test]
    async fn test_double_initialization() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(TestPlugin::new("test-plugin")));

        let context = PluginContext::minimal();

        // Initialize twice - second should be no-op
        registry.initialize_all(&context).await.unwrap();
        registry.initialize_all(&context).await.unwrap();
        assert!(registry.is_initialized());
    }

    #[tokio::test]
    async fn test_shutdown_without_initialization() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(TestPlugin::new("test-plugin")));

        // Should succeed even without initialization
        let result = registry.shutdown_all().await;
        assert!(result.is_ok());
        assert!(!registry.is_initialized());
    }
}
#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Generate arbitrary plugin names for testing
    prop_compose! {
        fn arb_plugin_name()(name in "[a-zA-Z][a-zA-Z0-9_-]{0,20}") -> String {
            name
        }
    }

    // Simple test plugin for property testing
    struct PropertyTestPlugin {
        name: String,
    }

    impl PropertyTestPlugin {
        fn new(name: String) -> Self {
            Self { name }
        }
    }

    #[async_trait]
    impl Plugin for PropertyTestPlugin {
        fn name(&self) -> &'static str {
            // We need to leak the string to get a 'static reference
            // This is acceptable for testing purposes
            Box::leak(self.name.clone().into_boxed_str())
        }

        async fn initialize(&mut self, _context: &PluginContext) -> Result<(), PluginError> {
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<(), PluginError> {
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    proptest! {
        /// **Feature: firewall-detection, Property 1: Plugin Registration Persistence**
        /// 
        /// For any plugin registered with the plugin registry, querying the registry 
        /// should return that plugin until it is explicitly removed.
        #[test]
        fn test_plugin_registration_persistence(plugin_names in prop::collection::vec(arb_plugin_name(), 1..10)) {
            tokio_test::block_on(async {
                let mut registry = PluginRegistry::new();
                
                // Register all plugins
                for name in &plugin_names {
                    let plugin = PropertyTestPlugin::new(name.clone());
                    registry.register(Box::new(plugin));
                }
                
                // Verify all plugins are registered
                prop_assert_eq!(registry.plugin_count(), plugin_names.len());
                
                let registered_names = registry.plugin_names();
                prop_assert_eq!(registered_names.len(), plugin_names.len());
                
                // Verify each plugin name is present (order may differ)
                for expected_name in &plugin_names {
                    prop_assert!(registered_names.iter().any(|&name| name == expected_name));
                }
                
                Ok(())
            })?;
        }
    }
}