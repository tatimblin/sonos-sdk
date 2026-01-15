# Rules for AI Agents

**IF YOU ARE AN AI AGENT YOU MUST FOLLOW THESE RULES**

## Standard Development Workflow

1. If you are working on a crate first read the SPEC file located in `/docs/spec/[CRATE_NAME].md` and any other service required for the task.
2. Always update the appropriate README.md or SPEC file when you make changes that impact the accuracy of these documents.
3. Do not create additional markdown files in the repository unless you are explicitly instructed to do so.
4. When writing unit tests, make concise and easy to read unit tests.
5. When writing unit tests, understand there can be too many unit tests. Strike a balance in test coverage and comprehension.

## Project Overview

A multi-crate rust project to provide a sonos SDK capable of live updates and full local api control.

## Repository Structure

- **`/docs/`** - Living documentation that reflects the current state of the codebase.
- **`/docs/SUMMARY.md`** - The entry point for the documentation.
- **`/callback-server/`** - Internal rust crate used by sonos-stream providing a callback server for upnp NOTIFY events to be sent to.
- **`/soap-client/`** - Internal rust crate for SOAP API utilities used by sonos-api providing an http client tailored to SOAP requests.
- **`/sonos-discovery/`** - Public crate for discovering Sonos devices on a network.
- **`/sonos-stream/`** - Internal rust crate for creating and receiving streamed Sonos events.
- **`/sonos-event-manager/`** - Internal rust crate that wraps sonos-event and manages subscription lifecycle.
- **`/sonos-api/`** - Public rust crate that provides an Http client for interacting with the local Sonos api.
- **`/sonos-state/`** - Internal rust crate that registers and holds live Sonos properties.
- **`/sonos-sdk/`** - Public crate for interacting with a Sonos system at a high level.

## Documentation

- **`/docs/SPEC_TEMPLATE.md`** - A template for writing specs. Specs live in each service and answer the "WHY"" for each technical detail in the system. Knowing the "WHY" is important for staying true to the intent of a system.
- **`/docs/SUMMARY.md`** - Index of each crate in the system and how they fit together.
- **`/docs/spec/[CRATE_NAME]`** - A living spec for a crate. It's accuracy is as important as the accuracy of the code.
