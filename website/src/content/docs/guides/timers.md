---
title: Timers & Alarms
description: Sleep timers and alarm management.
---

## Sleep timer methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `configure_sleep_timer` | `configure_sleep_timer(duration: &str)` | Set timer (format: `"HH:MM:SS"`) |
| `cancel_sleep_timer` | `cancel_sleep_timer()` | Cancel active timer |
| `get_remaining_sleep_timer` | `get_remaining_sleep_timer()` | Check time remaining |

## Set a sleep timer

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;
    let speaker = sonos.speaker("Bedroom").unwrap();

    // Set a 1-hour timer
    speaker.configure_sleep_timer("01:00:00")?;

    // Set a 30-minute timer
    speaker.configure_sleep_timer("00:30:00")?;

    Ok(())
}
```

## Cancel a sleep timer

```rust
speaker.cancel_sleep_timer()?;
```

Equivalent to `speaker.configure_sleep_timer("")`.

## Check remaining time

```rust
let remaining = speaker.get_remaining_sleep_timer()?;
println!("Time left: {}", remaining.remaining_sleep_timer_duration);
// Returns empty string if no timer is active
```

## Alarm methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `snooze_alarm` | `snooze_alarm(duration: &str)` | Snooze active alarm |
| `get_running_alarm_properties` | `get_running_alarm_properties()` | Get details of ringing alarm |

## Snooze an alarm

Only works when an alarm is currently ringing on the speaker:

```rust
// Snooze for 10 minutes
speaker.snooze_alarm("00:10:00")?;
```

## Get running alarm info

```rust
let alarm = speaker.get_running_alarm_properties()?;
println!("Alarm ID: {}", alarm.alarm_id);
println!("Group ID: {}", alarm.group_id);
println!("Started at: {}", alarm.logged_start_time);
```
