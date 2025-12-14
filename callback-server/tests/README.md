# Callback Server Integration Tests

This directory contains comprehensive integration tests for the callback server that verify end-to-end functionality.

## Test Coverage

### `test_callback_server_end_to_end`
- Starts a real HTTP server
- Registers a subscription
- Sends valid UPnP event notifications via HTTP POST
- Verifies events are received and processed correctly
- Tests both full UPnP headers (SID, NT, NTS) and minimal headers (SID only)
- Validates unregistered subscriptions return 404
- Tests invalid requests (missing SID header) return 400

### `test_multiple_subscriptions_concurrent_events`
- Tests multiple subscription registration
- Sends concurrent HTTP requests to different subscriptions
- Verifies all events are received and routed correctly
- Ensures no cross-contamination between subscriptions

### `test_dynamic_subscription_management`
- Tests subscription lifecycle (register/unregister)
- Verifies events are rejected before registration (404)
- Confirms events are accepted after registration (200)
- Validates events are rejected after unregistration (404)

### `test_server_ip_and_url_detection`
- Verifies server starts on correct IP and port
- Tests URL format and reachability
- Confirms server responds to HTTP requests

### `test_error_handling`
- Tests various malformed requests
- Verifies proper HTTP status codes for different error conditions
- Ensures malformed requests don't generate notifications

## Running Tests

```bash
# Run only integration tests
cargo test --package callback-server --test integration_tests

# Run all callback-server tests (unit + integration)
cargo test --package callback-server

# Run with output
cargo test --package callback-server --test integration_tests -- --nocapture
```

## Test Dependencies

- `reqwest` - HTTP client for sending test requests
- `tokio` - Async runtime for test execution

## Key Features Tested

1. **Real HTTP Server**: Tests use actual HTTP server instances, not mocks
2. **Network Communication**: Verifies real HTTP requests and responses
3. **Concurrent Operations**: Tests multiple simultaneous requests
4. **Error Conditions**: Validates proper error handling and status codes
5. **Subscription Management**: Tests dynamic registration/unregistration
6. **UPnP Protocol Compliance**: Validates UPnP header handling