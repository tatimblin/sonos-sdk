---
title: Groups
description: Create, modify, dissolve, and navigate speaker groups.
---

Every Sonos speaker is always in a group. A standalone speaker is a group of one. One speaker in each group is the **coordinator** — it controls playback for the group.

## Navigating groups

```rust
use sonos_sdk::prelude::*;

fn main() -> Result<(), SdkError> {
    let sonos = SonosSystem::new()?;

    // List all groups
    for group in sonos.groups() {
        println!("Group {} ({} members)", group.id, group.member_count());

        if let Some(coordinator) = group.coordinator() {
            println!("  Coordinator: {}", coordinator.name);
        }

        for member in group.members() {
            println!("  - {}", member.name);
        }
    }

    Ok(())
}
```

## Speaker to group

```rust
let kitchen = sonos.speaker("Kitchen").unwrap();
let group = kitchen.group().unwrap();

println!("Kitchen's group: {}", group.id);
println!("Standalone: {}", group.is_standalone());
println!("Is coordinator: {}", group.is_coordinator(&kitchen.id));
```

## Group methods

### Membership

| Method | Description |
|--------|-------------|
| `group.add_speaker(&speaker)` | Add a speaker to this group |
| `group.remove_speaker(&speaker)` | Remove a speaker (becomes standalone) |
| `group.dissolve()` | Remove all non-coordinator members |
| `group.coordinator()` | Get the coordinator Speaker handle |
| `group.members()` | Get all member Speaker handles |
| `group.speaker(name)` | Find a member by name |
| `group.member_count()` | Number of members |
| `group.is_standalone()` | True if single-speaker group |
| `group.is_coordinator(id)` | Check if a speaker is coordinator |

### Speaker convenience methods

| Method | Description |
|--------|-------------|
| `speaker.join_group(&group)` | Join a group (wraps `group.add_speaker`) |
| `speaker.leave_group()` | Leave group, become standalone |
| `speaker.become_standalone()` | Same as `leave_group()` |
| `speaker.delegate_coordination_to(id, rejoin)` | Transfer coordinator role |

## Join a group

```rust
let living_room_group = sonos.speaker("Living Room").unwrap().group().unwrap();
let kitchen = sonos.speaker("Kitchen").unwrap();

// From the speaker side
kitchen.join_group(&living_room_group)?;

// Or from the group side
living_room_group.add_speaker(&kitchen)?;
```

Cannot add the coordinator to its own group (returns `SdkError::InvalidOperation`).

## Leave a group

```rust
let kitchen = sonos.speaker("Kitchen").unwrap();

// Kitchen becomes its own standalone group
kitchen.leave_group()?;
```

## Remove a speaker

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();
let kitchen = sonos.speaker("Kitchen").unwrap();

group.remove_speaker(&kitchen)?;
```

Cannot remove the coordinator — delegate first if needed.

## Dissolve a group

Breaks the group apart so every member becomes standalone:

```rust
let group = sonos.speaker("Living Room").unwrap().group().unwrap();

let result = group.dissolve();

if result.is_success() {
    println!("All {} speakers ungrouped", result.succeeded.len());
}

if result.is_partial() {
    for (id, err) in &result.failed {
        println!("Failed to ungroup {}: {}", id, err);
    }
}
```

`dissolve()` attempts every member and reports partial failures via `GroupChangeResult`.

## Delegate coordination

Transfer the coordinator role to another speaker without dissolving the group:

```rust
let living_room = sonos.speaker("Living Room").unwrap();
let kitchen = sonos.speaker("Kitchen").unwrap();

// Kitchen becomes coordinator; Living Room stays in group
living_room.delegate_coordination_to(&kitchen.id, true)?;

// Kitchen becomes coordinator; Living Room leaves
living_room.delegate_coordination_to(&kitchen.id, false)?;
```

## Watch for group changes

The `group_membership` property updates when speakers join or leave groups:

```rust
for event in sonos.iter() {
    let membership = speaker.group_membership.watch()?;

    if let Some(info) = membership.value() {
        println!("Group: {}, Is coordinator: {}", info.group_id, info.is_coordinator);
    }
}
```
