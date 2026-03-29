---
title: "fix: Event router registration race drops initial UPnP events"
type: fix
status: active
date: 2026-03-28
---

# fix: Event router registration race drops initial UPnP events

## What We're Building

Fix a race condition in the SDK where the initial UPnP NOTIFY event (sent immediately after SUBSCRIBE) can be silently dropped because the EventRouter hasn't registered the subscription ID yet. This causes `watch()` to return `None` indefinitely until the next state *change* on the speaker.

## The Problem

When a consumer calls `watch()`, the SDK:

1. Sends `Command::Subscribe` to the worker thread
2. Worker calls `EventBroker.register_speaker_service()`
3. `SubscriptionManager.create_subscription()` sends HTTP SUBSCRIBE
4. Speaker responds with `SID: uuid:...`
5. **Then** `EventRouter.register(sid)` is called
6. Speaker sends initial NOTIFY with current state

**The race:** Step 6 can arrive before step 5 completes. The callback server receives the NOTIFY, checks if the SID is registered in EventRouter, finds it isn't, and **silently drops the event**. The state store never gets populated.

**Impact:** Consumers see `None` from `get()`/`watch()` until the speaker's state changes (e.g., user presses play/pause). For idle speakers, this means permanently blank data in the TUI.

## Why This Approach

### Buffer + replay unroutable events

When `EventRouter.route_event()` receives a NOTIFY with an unrecognized SID, buffer it briefly instead of dropping. When `register(sid)` is called moments later, replay any buffered events matching that SID. This is a well-known pattern in distributed systems (catch-up subscriptions, event sourcing).

```
route_event(sid, xml):
  if sid in registered:
    send(payload)                    # fast path — known SID
  else:
    buffer.push(sid, xml, now())     # hold briefly for late registration

register(sid):
  registered.insert(sid)
  for buffered where sid matches:
    send(payload)                    # replay missed events
  cleanup entries older than 5s      # prevent unbounded growth
```

### Alternatives considered

1. **Route by (speaker_ip, service) instead of SID** — Would eliminate the race by pre-registering before SUBSCRIBE. But UPnP NOTIFY doesn't include the service path — only the SID header. Would require encoding (ip, service) in the callback URL path, changing routing semantics, and making the URL structure load-bearing. Bigger refactor.

2. **Wildcard pre-registration by speaker IP** — Accept all events from a speaker IP during the registration window. Security concern (routes events to wrong subscription if multiple active). Complex state machine.

3. **Call fetch() inside watch() on cold cache** — Workaround, not a fix. Adds blocking SOAP calls to the watch path. The subscription mechanism should just work.

## Key Decisions

- **Buffer + replay, not re-route**: Keeps existing SID-based routing unchanged. Surgical fix — only `EventRouter` needs modification. No callback URL or protocol-level changes.
- **Fix in SDK, not consumer**: The TUI shouldn't need `bootstrap_group_data()` or any workaround — `watch()` + subscription events should populate data natively.
- **Keep watch() non-blocking**: `watch()` returns `None` on cold cache, subscription delivers initial event via buffer replay, `ChangeEvent` flows through `iter()`, consumer re-renders. No `fetch()` in the watch path.
- **5-second buffer TTL**: Generous window — the race is microseconds, but 5s handles any pathological scheduling delay. Stale entries cleaned up during `register()` calls.

## Scope

**In scope:**
- Add pending event buffer to `EventRouter`
- `route_event()` buffers events with unrecognized SIDs instead of dropping
- `register()` replays buffered events for the newly registered SID
- Stale buffer entry cleanup (TTL-based)
- Verify initial events are now reliably delivered

**Out of scope:**
- Adding fetch() fallback in watch() — the subscription should handle it
- TUI changes — the existing clear-before-draw pattern handles re-renders
- Polling fallback changes
- Changing callback URL structure

## Open Questions

None — approach is clear.

## Sources

- EventRouter registration: `callback-server/router.rs:80-83` (register), `157-172` (route_event)
- Subscription creation: `sonos-stream/broker.rs:510-518` (registers SID after SUBSCRIBE)
- Callback server NOTIFY handler: `callback-server/server.rs:252-349`
- Event processor: `sonos-stream/events/processor.rs:54-136`
- Related TUI issue: `sonos-cli` PR #32 — watch migration showed persistent blank state
