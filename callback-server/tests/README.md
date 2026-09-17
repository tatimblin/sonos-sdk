# Callback Server Integration Tests

This directory contains end-to-end integration tests for the callback server. Every test drives a real HTTP server over real requests; nothing is mocked.

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

### `test_oversized_notify_body_rejected`
- An oversized NOTIFY body is rejected before it is buffered in memory
- Guards against any host on the LAN exhausting memory through an unbounded read

### `test_invalid_nt_without_nts_is_rejected`
- `NT` and `NTS` are validated independently
- A request carrying only one of the two is still checked, so a bogus `NT` cannot slip through

### `test_notify_without_content_length_is_rejected`
- The size cap keys off `Content-Length`, so a chunked body has nothing to check against and is refused
- Driven over a raw socket, since `reqwest` always sets `Content-Length` for a sized body

### `test_notify_before_register_is_replayed`
- Covers the SUBSCRIBE/NOTIFY race, where a device delivers the first event before the SUBSCRIBE response has been processed
- The notification is held and replayed once the subscription registers

## Running Tests

The published package name is what `-p` takes:

```bash
# Run only integration tests
cargo test -p sonos-sdk-callback-server --test integration_tests

# Run all callback-server tests (unit + integration)
cargo test -p sonos-sdk-callback-server

# Run with output
cargo test -p sonos-sdk-callback-server --test integration_tests -- --nocapture
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
7. **Resource Limits**: Validates body size caps and `Content-Length` requirements
