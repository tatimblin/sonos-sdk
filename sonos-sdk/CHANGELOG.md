# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.1](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.5.0...sonos-sdk-v0.5.1) - 2026-05-02

### Other

- remove print ([#70](https://github.com/tatimblin/sonos-sdk/pull/70))

## [0.5.0](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.4.0...sonos-sdk-v0.5.0) - 2026-05-02

### Added

- read-time coordinator resolution for PerCoordinator properties ([#68](https://github.com/tatimblin/sonos-sdk/pull/68))

### Fixed

- data freshness — topology overhaul + fetch coordinator routing ([#69](https://github.com/tatimblin/sonos-sdk/pull/69))

### Other

- release v0.4.0 ([#66](https://github.com/tatimblin/sonos-sdk/pull/66))

## [0.4.0](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.3.0...sonos-sdk-v0.4.0) - 2026-03-29

### Added

- Integration test suite for real speaker validation ([#61](https://github.com/tatimblin/sonos-sdk/pull/61))

### Fixed

- *(sdk)* move EventInitFn to StateManager to fix watch() propagation ([#64](https://github.com/tatimblin/sonos-sdk/pull/64))
- *(callback-server)* buffer + replay events for unregistered SIDs ([#63](https://github.com/tatimblin/sonos-sdk/pull/63))

## [0.3.0](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.2.1...sonos-sdk-v0.3.0) - 2026-03-28

### Added

- RAII WatchHandle with 50ms grace period ([#59](https://github.com/tatimblin/sonos-sdk/pull/59))

## [0.2.1](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.2.0...sonos-sdk-v0.2.1) - 2026-03-20

### Other

- move smart_dashboard example to sonos-sdk crate ([#58](https://github.com/tatimblin/sonos-sdk/pull/58))
- release v0.2.0 ([#56](https://github.com/tatimblin/sonos-sdk/pull/56))

## [0.2.0](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.1.0...sonos-sdk-v0.2.0) - 2026-03-15

### Added

- *(sdk)* add with_groups() test helper for group topology
- *(sdk)* add prelude module and #[non_exhaustive] on SdkError
- *(sdk)* method renames and fluent entity navigation
- *(sdk)* lazy event manager initialization
- *(sdk)* re-export sonos_discovery for consumer access to Device type
- *(sdk)* add discovery caching and auto-rediscovery

### Fixed

- *(sdk)* use room names for speaker and group identity ([#51](https://github.com/tatimblin/sonos-sdk/pull/51))
- *(ci)* resolve P1 CI failures and add comprehensive SDK demo

### Other

- add contributing guide ([#52](https://github.com/tatimblin/sonos-sdk/pull/52))
- Merge branch 'feat/sdk-api-best-practices' into main
- *(sdk)* fix clippy warning and suppress dead_code on event_manager
- update lib.rs examples, spec, and plan status
- *(sdk)* tighten public API and add test-support feature
- address code review findings (wave 1-3)
