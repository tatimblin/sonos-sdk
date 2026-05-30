---
title: Command Reference
description: Every sonos-cli command with flags and examples.
---

## Playback

| Command | Description |
|---------|-------------|
| `sonos play` | Start playback |
| `sonos pause` | Pause playback |
| `sonos stop` | Stop playback |
| `sonos next` | Skip to next track |
| `sonos prev` | Skip to previous track |
| `sonos seek <H:MM:SS>` | Seek to position in current track |
| `sonos mode <mode>` | Set play mode |

### Play modes

`normal`, `repeat`, `repeat-one`, `shuffle`, `shuffle-no-repeat`

```bash
sonos mode shuffle --group "Living Room"
```

## Volume & EQ

| Command | Description |
|---------|-------------|
| `sonos volume <0-100>` | Set volume level |
| `sonos mute` | Mute playback |
| `sonos unmute` | Unmute playback |
| `sonos bass <-10..10>` | Set bass level (speaker only) |
| `sonos treble <-10..10>` | Set treble level (speaker only) |
| `sonos loudness <on\|off>` | Set loudness compensation (speaker only) |

```bash
sonos volume 50 --group "Kitchen"
sonos bass 3 --speaker "Beam"
```

## Information

| Command | Description |
|---------|-------------|
| `sonos speakers` | List all speakers with state and volume |
| `sonos groups` | List all groups with playback state |
| `sonos status` | Show current track, state, and volume |

## Groups

| Command | Description |
|---------|-------------|
| `sonos join` | Add a speaker to a group |
| `sonos leave` | Remove a speaker from its group |

## Queue

| Command | Description |
|---------|-------------|
| `sonos queue` | Show the playback queue |
| `sonos queue add <uri>` | Add a URI to the queue |
| `sonos queue clear` | Clear the queue |

## Timer

| Command | Description |
|---------|-------------|
| `sonos sleep <duration>` | Set sleep timer (e.g., `30m`, `1h`) or `cancel` |

```bash
sonos sleep 45m --group "Bedroom"
sonos sleep cancel
```

## Global flags

| Flag | Description |
|------|-------------|
| `--help`, `-h` | Show help |
| `--version` | Print version |
| `--quiet`, `-q` | Suppress non-error output |
| `--verbose` | Show debug output |
| `--no-input` | Disable interactive prompts |
| `--speaker <name>` | Target a specific speaker |
| `--group <name>` | Target a specific group |
