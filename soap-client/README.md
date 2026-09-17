# soap-client

Internal implementation detail of [sonos-sdk](https://crates.io/crates/sonos-sdk). This crate is published to crates.io as `sonos-sdk-soap-client`, a transitive dependency, and is not intended for direct use.

A minimal SOAP client for UPnP device communication. `SoapClient::get()` returns a handle to a process-wide instance, so every caller shares one HTTP connection pool.

It covers two things:

- **Control**: `call()` sends a SOAP action to a device's `/Control` endpoint and returns the response body as text, after detecting SOAP faults and surfacing the UPnP error code
- **Eventing**: `subscribe()`, `renew_subscription()` and `unsubscribe()` implement the UPnP SUBSCRIBE/UNSUBSCRIBE methods, returning the device-assigned SID and the timeout the device actually granted

Failures surface as `SoapError::Network`, `SoapError::Parse`, or `SoapError::Fault { code, .. }`.
Response *shape* is left to the caller: [`sonos-api`](../sonos-api) owns per-service parsing.

## License

Licensed under either of [Apache License, Version 2.0](../LICENSE-APACHE) or
[MIT license](../LICENSE-MIT), at your option.
