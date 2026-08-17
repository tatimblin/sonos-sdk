# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.7.0...sonos-sdk-v0.7.1) - 2026-08-17

### Fixed

- exclude satellites before name-keying so bonded home theaters survive ([#102](https://github.com/tatimblin/sonos-sdk/pull/102))

### Other

- release v0.7.0 ([#101](https://github.com/tatimblin/sonos-sdk/pull/101))

## [0.7.0](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.6.0...sonos-sdk-v0.7.0) - 2026-08-17

### Added

- [**breaking**] give every iter() its own event stream instead of splitting one ([#100](https://github.com/tatimblin/sonos-sdk/pull/100))
- [**breaking**] make WatchHandle a live view instead of a frozen snapshot ([#99](https://github.com/tatimblin/sonos-sdk/pull/99))
- [**breaking**] carry changed value on ChangeEvent and order writes by observation ([#94](https://github.com/tatimblin/sonos-sdk/pull/94))

### Fixed

- stamp event writes at observation time, not apply time ([#96](https://github.com/tatimblin/sonos-sdk/pull/96))
- stamp polling events at poll request, not at response ([#98](https://github.com/tatimblin/sonos-sdk/pull/98))
- close duplicate-registration SID leak and unblock polling stats ([#97](https://github.com/tatimblin/sonos-sdk/pull/97))

## [0.6.0](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.5.3...sonos-sdk-v0.6.0) - 2026-08-16

### Fixed

- break SDK Arc cycle and stop one watcher release silencing others ([#93](https://github.com/tatimblin/sonos-sdk/pull/93))
- correct AVTransport request element names and escape queue URIs ([#87](https://github.com/tatimblin/sonos-sdk/pull/87))
- parse RenderingControl EQ variables as per-channel ([#85](https://github.com/tatimblin/sonos-sdk/pull/85))
- detect SOAP faults instead of reporting them as network errors ([#88](https://github.com/tatimblin/sonos-sdk/pull/88))
- stop unrelated topology events from wiping all group state ([#90](https://github.com/tatimblin/sonos-sdk/pull/90))
- select callback URL per interface instead of by default route ([#92](https://github.com/tatimblin/sonos-sdk/pull/92))
- stop polling fallback from running forever on healthy UPnP events ([#91](https://github.com/tatimblin/sonos-sdk/pull/91))
- harden unauthenticated UPnP callback endpoint against resource exhaustion ([#86](https://github.com/tatimblin/sonos-sdk/pull/86))

### Other

- make SDK unit and property tests fully offline ([#83](https://github.com/tatimblin/sonos-sdk/pull/83))
- prune unused dependencies and fix stale docs ([#84](https://github.com/tatimblin/sonos-sdk/pull/84))

## [0.5.3](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.5.2...sonos-sdk-v0.5.3) - 2026-08-15

### Fixed

- send SSDP M-SEARCH per interface instead of from 0.0.0.0 ([#80](https://github.com/tatimblin/sonos-sdk/pull/80))

## [0.5.2](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.5.1...sonos-sdk-v0.5.2) - 2026-05-03

### Other

- watch or fetch ([#73](https://github.com/tatimblin/sonos-sdk/pull/73))

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
