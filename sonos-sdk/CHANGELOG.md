# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/tatimblin/sonos-sdk/compare/sonos-sdk-v0.1.0...sonos-sdk-v0.2.0) - 2026-03-14

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
