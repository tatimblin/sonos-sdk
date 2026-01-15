//! Logging infrastructure for Sonos SDK
//!
//! This module provides a centralized logging system that can be configured
//! for different environments, particularly to ensure TUI applications
//! render cleanly without stderr/stdout contamination.

use tracing_subscriber::{fmt, EnvFilter, Registry};

/// Logging mode for different use cases
#[derive(Debug, Clone, Copy)]
pub enum LoggingMode {
    /// No output - perfect for TUI applications
    Silent,
    /// Pretty stderr output for development
    Development,
    /// Verbose diagnostics for debugging
    Debug,
}

/// Logging configuration error
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("Failed to initialize tracing subscriber: {0}")]
    TracingInit(String),

    #[error("Invalid environment variable: {0}")]
    InvalidEnv(String),
}

/// Initialize logging with the specified mode
///
/// This function should be called early in the application lifecycle,
/// before any other Sonos SDK operations that might generate log output.
///
/// # Examples
///
/// ```rust,ignore
/// // For TUI applications - no output
/// sonos_state::logging::init_logging(LoggingMode::Silent)?;
///
/// // For development - structured logs to stderr
/// sonos_state::logging::init_logging(LoggingMode::Development)?;
///
/// // For debugging - verbose logs with source locations
/// sonos_state::logging::init_logging(LoggingMode::Debug)?;
/// ```
///
/// # Environment Variables
///
/// - `SONOS_LOG_LEVEL`: Override log level (error, warn, info, debug, trace)
/// - `SONOS_LOG_TARGET`: Filter by target (e.g., "sonos_stream::events")
///
pub fn init_logging(mode: LoggingMode) -> Result<(), LoggingError> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    match mode {
        LoggingMode::Silent => {
            // No subscriber - all logs are dropped at compile time when possible
            // This is the most efficient option for production TUI applications
            Ok(())
        }
        LoggingMode::Development => {
            // Pretty stderr output suitable for development
            let filter = create_env_filter("info")?;

            let subscriber = Registry::default()
                .with(fmt::layer()
                    .with_target(false)  // Don't show module paths by default
                    .with_thread_ids(false)
                    .with_file(false)
                    .with_line_number(false)
                    .compact())
                .with(filter);

            subscriber.try_init()
                .map_err(|e| LoggingError::TracingInit(e.to_string()))?;

            Ok(())
        }
        LoggingMode::Debug => {
            // Verbose formatter with source locations for debugging
            let filter = create_env_filter("debug")?;

            let subscriber = Registry::default()
                .with(fmt::layer()
                    .pretty()
                    .with_thread_ids(true)
                    .with_file(true)
                    .with_line_number(true))
                .with(filter);

            subscriber.try_init()
                .map_err(|e| LoggingError::TracingInit(e.to_string()))?;

            Ok(())
        }
    }
}

/// Initialize logging from environment variables
///
/// Reads the `SONOS_LOG_MODE` environment variable to determine the logging mode:
/// - "silent" -> LoggingMode::Silent
/// - "development" -> LoggingMode::Development
/// - "debug" -> LoggingMode::Debug
///
/// Defaults to Silent mode if not specified or invalid.
pub fn init_logging_from_env() -> Result<(), LoggingError> {
    let mode = match std::env::var("SONOS_LOG_MODE").as_deref() {
        Ok("development") => LoggingMode::Development,
        Ok("debug") => LoggingMode::Debug,
        _ => LoggingMode::Silent,  // Default to silent for TUI compatibility
    };

    init_logging(mode)
}

/// Create an environment filter with fallback to default level
fn create_env_filter(default_level: &str) -> Result<EnvFilter, LoggingError> {
    // First try SONOS_LOG_LEVEL, then RUST_LOG, then default
    let filter = if let Ok(level) = std::env::var("SONOS_LOG_LEVEL") {
        EnvFilter::new(level)
    } else if let Ok(rust_log) = std::env::var("RUST_LOG") {
        EnvFilter::new(rust_log)
    } else {
        EnvFilter::new(default_level)
    };

    Ok(filter)
}

/// Check if logging has been initialized
///
/// This is useful to avoid double-initialization in complex applications.
pub fn is_initialized() -> bool {
    tracing::dispatcher::has_been_set()
}

/// Convenience function to initialize with silent mode for TUI applications
///
/// This is equivalent to `init_logging(LoggingMode::Silent)` but more explicit
/// for TUI use cases.
pub fn init_silent() -> Result<(), LoggingError> {
    init_logging(LoggingMode::Silent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silent_mode() {
        // Silent mode should not fail
        assert!(init_logging(LoggingMode::Silent).is_ok());
    }

    #[test]
    fn test_logging_mode_debug() {
        format!("{:?}", LoggingMode::Debug);
    }
}