//! Ratatui Integration Example
//!
//! Demonstrates how to build a Sonos control TUI using ratatui and the
//! WidgetStateManager for efficient, localized property watching.
//!
//! This example shows the key pattern: widgets calling `watch_property`
//! exactly where they need it, without centralized state maps.
//!
//! Run with: `cargo run -p sonos-state --example ratatui_example`

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::*};

use sonos_discovery::{self, Device};
use sonos_state::{
    Mute, PlaybackState, SpeakerId, StateManager, Volume, WidgetStateManager,
};

/// Current speaker data queried directly from state (no caching)
#[derive(Debug, Clone)]
struct CurrentSpeakerData {
    volume: Option<Volume>,
    volume_changed: bool,
    mute: Option<Mute>,
    mute_changed: bool,
    playback_state: Option<PlaybackState>,
    playback_changed: bool,
}

struct SonosApp {
    should_quit: bool,
    selected_speaker: usize,
    speakers: Vec<SpeakerId>,
    last_interaction: Instant,
    show_help: bool,
    demo_mode: bool, // For when no devices are found
    volume_override: Option<u8>, // For demo mode
}

impl SonosApp {
    fn new(speakers: Vec<SpeakerId>) -> Self {
        let demo_mode = speakers.is_empty();
        let speakers = if demo_mode {
            // Create mock speakers for demo
            vec![
                SpeakerId::new("DEMO_KITCHEN"),
                SpeakerId::new("DEMO_LIVING_ROOM"),
                SpeakerId::new("DEMO_BEDROOM"),
            ]
        } else {
            speakers
        };

        Self {
            should_quit: false,
            selected_speaker: 0,
            speakers: speakers.clone(),
            last_interaction: Instant::now(),
            show_help: false,
            demo_mode,
            volume_override: Some(50), // Start with 50% for demo
        }
    }


    /// Handle keyboard input
    fn handle_input(&mut self, key: KeyCode, state_manager: &StateManager) {
        self.last_interaction = Instant::now();

        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char('h') | KeyCode::F(1) => {
                self.show_help = !self.show_help;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_speaker > 0 {
                    self.selected_speaker -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_speaker < self.speakers.len() - 1 {
                    self.selected_speaker += 1;
                }
            }
            KeyCode::Char(' ') => {
                // Toggle playback (in real app, would call Sonos API)
                if let Some(speaker_id) = self.speakers.get(self.selected_speaker) {
                    // For demo, just show feedback
                    if self.demo_mode {
                        // Simulate playback toggle
                    } else {
                        // In real app: call state_manager with play/pause operation
                        println!("Would toggle playback for {}", speaker_id);
                    }
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                // Volume up
                if self.demo_mode {
                    if let Some(ref mut vol) = self.volume_override {
                        *vol = (*vol + 10).min(100);
                        if let Some(speaker_id) = self.speakers.get(self.selected_speaker) {
                            state_manager.update_property(speaker_id, Volume::new(*vol));
                        }
                    }
                }
            }
            KeyCode::Char('-') => {
                // Volume down
                if self.demo_mode {
                    if let Some(ref mut vol) = self.volume_override {
                        *vol = vol.saturating_sub(10);
                        if let Some(speaker_id) = self.speakers.get(self.selected_speaker) {
                            state_manager.update_property(speaker_id, Volume::new(*vol));
                        }
                    }
                }
            }
            KeyCode::Char('m') => {
                // Toggle mute
                if let Some(speaker_id) = self.speakers.get(self.selected_speaker) {
                    if self.demo_mode {
                        // Simulate mute toggle
                        state_manager.update_property(speaker_id, Mute::new(true));
                        // Quick toggle for demo
                        state_manager.update_property(speaker_id, Mute::new(false));
                    }
                }
            }
            _ => {}
        }
    }

    /// Render the main application UI
    fn render(
        &self,
        frame: &mut Frame<'_>,
        current_speaker_data: Option<&CurrentSpeakerData>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.show_help {
            render_help(frame);
            return Ok(());
        }

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(frame.size());

        // Left side: Speaker list
        self.render_speaker_list(frame, chunks[0])?;

        // Right side: Selected speaker details
        if let Some(speaker_id) = self.speakers.get(self.selected_speaker) {
            if let Some(data) = current_speaker_data {
                self.render_speaker_details(frame, chunks[1], speaker_id, data)?;
            }
        }

        Ok(())
    }

    /// Render the speaker list widget
    fn render_speaker_list(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let items: Vec<ListItem> = self
            .speakers
            .iter()
            .enumerate()
            .map(|(i, speaker_id)| {
                let style = if i == self.selected_speaker {
                    Style::default().bg(Color::Blue).fg(Color::White)
                } else {
                    Style::default()
                };

                let display_name = if self.demo_mode {
                    // Pretty names for demo
                    match speaker_id.as_str() {
                        "DEMO_KITCHEN" => "🍳 Kitchen",
                        "DEMO_LIVING_ROOM" => "🛋️  Living Room",
                        "DEMO_BEDROOM" => "🛏️  Bedroom",
                        _ => speaker_id.as_str(),
                    }
                } else {
                    speaker_id.as_str()
                };

                ListItem::new(display_name).style(style)
            })
            .collect();

        let title = if self.demo_mode {
            "📱 Speakers (Demo Mode)"
        } else {
            "📱 Speakers"
        };

        let items_empty = items.is_empty();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::White)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::LightBlue)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, area, &mut ListState::default());

        // Show selection indicator
        if !items_empty {
            let indicator_area = Rect {
                x: area.x + 1,
                y: area.y + 1 + self.selected_speaker as u16,
                width: 2,
                height: 1,
            };

            if indicator_area.y < area.y + area.height - 1 {
                let indicator = Paragraph::new("►")
                    .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                frame.render_widget(indicator, indicator_area);
            }
        }

        Ok(())
    }

    /// Render detailed view of selected speaker
    fn render_speaker_details(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        speaker_id: &SpeakerId,
        speaker_data: &CurrentSpeakerData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Title
                Constraint::Length(3),  // Volume bar
                Constraint::Length(3),  // Controls
                Constraint::Length(3),  // Mute status
                Constraint::Min(0),     // Current track info
            ])
            .split(area);

        // Title
        let display_name = if self.demo_mode {
            match speaker_id.as_str() {
                "DEMO_KITCHEN" => "🍳 Kitchen Speaker",
                "DEMO_LIVING_ROOM" => "🛋️  Living Room Speaker",
                "DEMO_BEDROOM" => "🛏️  Bedroom Speaker",
                _ => speaker_id.as_str(),
            }
        } else {
            speaker_id.as_str()
        };

        let title = Paragraph::new(display_name)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("🔊 Selected Speaker")
                    .border_style(Style::default().fg(Color::White)),
            )
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        frame.render_widget(title, chunks[0]);

        // Render with current speaker data (queried from state before rendering)
        // Volume bar widget - demonstrates direct state usage without caching
        render_volume_bar(speaker_data, frame, chunks[1], speaker_id)?;

        // Playback controls widget
        render_playback_controls(speaker_data, frame, chunks[2], speaker_id)?;

        // Mute status widget
        render_mute_status(speaker_data, frame, chunks[3], speaker_id)?;

        // Current track widget (placeholder)
        render_current_track_info(frame, chunks[4], speaker_id)?;

        Ok(())
    }
}

/// Volume bar widget - demonstrates direct state access without caching
fn render_volume_bar(
    speaker_data: &CurrentSpeakerData,
    frame: &mut Frame<'_>,
    area: Rect,
    _speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    // 🚀 KEY PATTERN: Use state data queried directly before rendering (no caching)
    let volume_opt = speaker_data.volume.clone();
    let changed = speaker_data.volume_changed;

    // Only render when volume changed (efficient!)
    if changed || true {
        // Always render for demo visibility
        let volume_value = volume_opt.map(|v| v.0).unwrap_or(0);

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("🔊 Volume: {}%", volume_value))
                    .border_style(if changed {
                        Style::default().fg(Color::Green) // Green when changed
                    } else {
                        Style::default().fg(Color::White)
                    }),
            )
            .gauge_style(
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::Black)
                    .add_modifier(if volume_value == 0 {
                        Modifier::DIM
                    } else {
                        Modifier::empty()
                    }),
            )
            .percent(volume_value as u16);

        frame.render_widget(gauge, area);

        // Show change indicator
        if changed {
            let indicator_area = Rect {
                x: area.x + area.width - 6,
                y: area.y,
                width: 5,
                height: 1,
            };
            let indicator =
                Paragraph::new("NEW!").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            frame.render_widget(indicator, indicator_area);
        }
    }

    Ok(())
}

/// Playback controls widget
fn render_playback_controls(
    speaker_data: &CurrentSpeakerData,
    frame: &mut Frame<'_>,
    area: Rect,
    _speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    // Use playback state queried directly before rendering
    let playback_state = speaker_data.playback_state.clone();
    let changed = speaker_data.playback_changed;

    if changed || true {
        // For demo, always render
        let is_playing = playback_state.map(|ps| ps.is_playing()).unwrap_or(false);

        let controls_text = if is_playing {
            "⏸️  Playing - Press SPACE to pause"
        } else {
            "▶️  Paused - Press SPACE to play"
        };

        let controls = Paragraph::new(controls_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("🎵 Playback")
                    .border_style(if changed {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::White)
                    }),
            )
            .style(Style::default().fg(if is_playing {
                Color::Green
            } else {
                Color::Gray
            }))
            .alignment(Alignment::Center);

        frame.render_widget(controls, area);
    }

    Ok(())
}

/// Mute status widget
fn render_mute_status(
    speaker_data: &CurrentSpeakerData,
    frame: &mut Frame<'_>,
    area: Rect,
    _speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    // Use mute state queried directly before rendering
    let mute_state = speaker_data.mute.clone();
    let changed = speaker_data.mute_changed;

    if changed || true {
        let is_muted = mute_state.map(|m| m.0).unwrap_or(false);

        let mute_text = if is_muted {
            "🔇 MUTED - Press 'M' to unmute"
        } else {
            "🔊 Unmuted - Press 'M' to mute"
        };

        let mute_widget = Paragraph::new(mute_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("🔇 Audio")
                    .border_style(if changed {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::White)
                    }),
            )
            .style(Style::default().fg(if is_muted {
                Color::Red
            } else {
                Color::White
            }))
            .alignment(Alignment::Center);

        frame.render_widget(mute_widget, area);
    }

    Ok(())
}

/// Current track info widget (placeholder)
fn render_current_track_info(
    frame: &mut Frame<'_>,
    area: Rect,
    speaker_id: &SpeakerId,
) -> Result<(), Box<dyn std::error::Error>> {
    // In a real app, this would use widget_state.watch_property::<CurrentTrack>()
    // For demo, show static info

    let demo_track_info = if speaker_id.as_str().contains("KITCHEN") {
        "🎵 Now Playing: Jazz Playlist\n🎤 Artist: Miles Davis\n💿 Album: Kind of Blue"
    } else if speaker_id.as_str().contains("LIVING_ROOM") {
        "🎵 Now Playing: Classical Radio\n🎼 Composer: Mozart\n🎻 Symphony No. 40"
    } else {
        "🎵 Now Playing: Ambient Sounds\n🌙 Track: Rain Forest\n⏱️  Duration: 8 hours"
    };

    let track_info = Paragraph::new(demo_track_info)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("🎵 Now Playing")
                .border_style(Style::default().fg(Color::White)),
        )
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Left);

    frame.render_widget(track_info, area);

    Ok(())
}

/// Render help screen
fn render_help(frame: &mut Frame) {
    let help_text = "🎵 Sonos Control TUI - Help

NAVIGATION:
  ↑/k       - Move up in speaker list
  ↓/j       - Move down in speaker list

CONTROLS:
  SPACE     - Toggle play/pause
  +/=       - Volume up (demo mode)
  -         - Volume down (demo mode)
  m         - Toggle mute

INTERFACE:
  h/F1      - Toggle this help
  q/ESC     - Quit application

DEMO MODE:
This example runs in demo mode when no Sonos devices
are found on the network. Use +/- to change volume
and see the real-time UI updates.

REAL USAGE:
When connected to actual Sonos devices, the app will
show real device names and respond to actual device
state changes through the reactive state system.

🚀 KEY FEATURE DEMO:
Notice how each widget (volume bar, controls, etc.)
calls watch_property() exactly where it needs the data!
No centralized state management required.

Press 'h' or F1 to return to the main interface.";

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("📖 Help")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Left);

    frame.render_widget(help, frame.size());
}

/// Render footer with controls hint
fn render_footer(frame: &mut Frame, demo_mode: bool) {
    let controls = if demo_mode {
        "Demo Mode | ↑↓: Select | +/-: Volume | m: Mute | SPACE: Play/Pause | h: Help | q: Quit"
    } else {
        "Live Mode | ↑↓: Select | SPACE: Play/Pause | h: Help | q: Quit"
    };

    let footer_area = Rect {
        x: 0,
        y: frame.size().height.saturating_sub(1),
        width: frame.size().width,
        height: 1,
    };

    let footer = Paragraph::new(controls)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .alignment(Alignment::Center);

    frame.render_widget(footer, footer_area);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup
    println!("🎵 Starting Sonos TUI Control...");

    // Discover devices (must be done in blocking context due to reqwest::blocking)
    println!("🔍 Discovering Sonos devices...");
    let devices = sonos_discovery::get();
    let speaker_ids: Vec<SpeakerId> = devices.iter().map(|d| SpeakerId::new(&d.id)).collect();

    // Create tokio runtime
    let rt = tokio::runtime::Runtime::new()?;

    // Run the async main function with discovered devices
    rt.block_on(async_main(devices, speaker_ids))
}

async fn async_main(devices: Vec<Device>, speaker_ids: Vec<SpeakerId>) -> Result<(), Box<dyn std::error::Error>> {
    // Create state manager
    let state_manager = Arc::new(StateManager::new().await?);
    println!("✅ Created StateManager with event processing");

    let demo_mode = devices.is_empty();
    if demo_mode {
        println!("⚠️  No Sonos devices found - running in demo mode");
        println!("💡 Use +/- keys to change volume and see reactive updates");
    } else {
        println!("📱 Found {} device(s)", devices.len());
        state_manager.add_devices(devices).await?;
    }

    // Create widget state manager for ratatui integration
    let mut widget_state = WidgetStateManager::new(Arc::clone(&state_manager)).await?;
    println!("✅ Created WidgetStateManager for TUI integration");

    // Create app
    let mut app = SonosApp::new(speaker_ids);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    println!("🚀 Starting TUI...");

    // Main application loop
    let result = run_app(&mut terminal, &mut app, &mut widget_state, &state_manager).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        println!("❌ Application error: {:?}", err);
    } else {
        println!("✅ Application closed cleanly");
    }

    Ok(())
}

async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut SonosApp,
    widget_state: &mut WidgetStateManager,
    state_manager: &StateManager,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // 1. PROCESS GLOBAL CHANGES (key part of ratatui integration!)
        widget_state.process_global_changes();

        // 2. HANDLE INPUT (non-blocking with 16ms timeout for ~60 FPS)
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                app.handle_input(key.code, state_manager);
            }
        }

        // 3. RENDER ONLY WHEN NEEDED (change-driven rendering!)
        let should_render = widget_state.has_any_changes()
            || app.last_interaction.elapsed() < Duration::from_millis(100) // Always render briefly after interaction
            || app.show_help; // Always render when showing help

        if should_render {
            // Query current speaker data and render in one step
            let current_speaker_data = if let Some(speaker_id) = app.speakers.get(app.selected_speaker) {
                if app.demo_mode {
                    Some(CurrentSpeakerData {
                        volume: app.volume_override.map(Volume::new),
                        mute: Some(Mute::new(false)),
                        playback_state: Some(PlaybackState::Paused),
                        volume_changed: false,
                        mute_changed: false,
                        playback_changed: false,
                    })
                } else {
                    // Query state directly for current speaker
                    let volume_result = widget_state.watch_property::<Volume>(speaker_id).await.unwrap_or((None, false));
                    let mute_result = widget_state.watch_property::<Mute>(speaker_id).await.unwrap_or((None, false));
                    let playback_result = widget_state.watch_property::<PlaybackState>(speaker_id).await.unwrap_or((None, false));

                    Some(CurrentSpeakerData {
                        volume: volume_result.0,
                        mute: mute_result.0,
                        playback_state: playback_result.0,
                        volume_changed: volume_result.1,
                        mute_changed: mute_result.1,
                        playback_changed: playback_result.1,
                    })
                }
            } else {
                None
            };

            terminal.draw(|frame| {
                // Render main UI
                if let Err(e) = app.render(frame, current_speaker_data.as_ref()) {
                    // In a real app, you'd handle this error better
                    eprintln!("Render error: {}", e);
                }

                // Render footer
                render_footer(frame, app.demo_mode);
            })?;
        }

        // 4. CHECK EXIT CONDITION
        if app.should_quit {
            break;
        }

        // Sleep for consistent frame rate (~60 FPS)
        tokio::time::sleep(Duration::from_millis(16)).await;
    }

    Ok(())
}

// Additional demo functionality for when no devices are present
async fn simulate_background_changes(
    state_manager: Arc<StateManager>,
    speakers: Vec<SpeakerId>,
) {
    tokio::spawn(async move {
        let mut volume = 50u8;
        let mut direction = 1i8;

        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;

            // Simulate gradual volume changes
            if direction > 0 && volume < 90 {
                volume += 5;
            } else if direction < 0 && volume > 10 {
                volume -= 5;
            } else {
                direction *= -1;
            }

            // Apply to a random speaker
            if let Some(speaker) = speakers.get(0) {
                state_manager.update_property(speaker, Volume::new(volume));
            }
        }
    });
}