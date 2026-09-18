---
title: Command Reference
description: Every sonos-cli command with flags and examples.
---

Running `sonos` with no subcommand launches the interactive TUI when stdout is a
terminal. With a subcommand it runs that command and exits.

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
| `sonos sleep <duration>` | Set sleep timer (e.g., `30m`, `1h`, `90m`) or `cancel` |

```bash
sonos sleep 45m --group "Bedroom"
sonos sleep cancel
```

## Config

| Command | Description |
|---------|-------------|
| `sonos config alias` | List all aliases |
| `sonos config alias <name> <alias>` | Assign a short alias to a speaker or group |
| `sonos config alias <name>` | Clear that name's alias |

```bash
sonos config alias "Living Room" lr
sonos volume 40 --speaker lr
```

## Global flags

| Flag | Description |
|------|-------------|
| `--help`, `-h` | Show help |
| `--version`, `-V` | Print version |
| `--quiet`, `-q` | Suppress all non-error stdout output |
| `--verbose`, `-v` | Increase log verbosity; repeatable (`-v` info, `-vv` debug, `-vvv` trace) |
| `--no-input` | Disable interactive prompts |
| `--speaker <name>`, `-s` | Target a specific speaker by friendly name or alias |
| `--group <name>`, `-g` | Target a group by name or alias |
