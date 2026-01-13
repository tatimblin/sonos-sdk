//! Interactive TUI example showcasing the DOM-like Sonos SDK API
//!
//! This example demonstrates the three core property access methods in an interactive terminal:
//! - get() - Cached values (🔄 Gray)
//! - fetch() - Fresh API calls (🌐 Blue)
//! - watch() - Reactive live updates (👁️ Green)
//!
//! Run with: cargo run -p sonos-sdk --example ratatui_sdk_example
//!
//! Features:
//! - Arrow keys navigate through speakers
//! - Detail view shows volume (watched) and playback state (fetched)
//! - Visual indicators for each API method
//! - Demo mode when no speakers found

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Direction},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Gauge},
    Frame, Terminal,
};
use sonos_sdk::{SonosSystem, SdkError, Speaker};
use sonos_discovery::{self, Device};
use sonos_state::{PropertyWatcher, Volume, PlaybackState};
use std::{
    io::{self, Stdout},
    time::{Duration, Instant},
};

/// Property status for visual indicators
#[derive(Clone, Debug)]
enum PropertyStatus {
    Cached,              // Gray - using get()
    Fresh,               // Blue - just fetched with fetch()
    Watching,            // Green - live updates from watch()
    Error(String),       // Red - error state
}

/// Main application state
struct SdkTuiApp {
    system: SonosSystem,
    speaker_names: Vec<String>,  // Just store names, query system for Speaker instances
    selected_index: usize,
    should_quit: bool,
    demo_mode: bool,

    // Cache selected speaker name for synchronous rendering
    selected_speaker_name: Option<String>,

    // Async state for selected speaker
    volume_watcher: Option<PropertyWatcher<Volume>>,
    current_volume: Option<Volume>,
    volume_status: PropertyStatus,

    current_playback: Option<PlaybackState>,
    playback_status: PropertyStatus,

    last_navigation: Instant,
    status_message: String,
}

impl SdkTuiApp {
    /// Initialize the application with discovered devices
    async fn new(devices: Vec<Device>) -> Result<Self, SdkError> {
        let demo_mode = devices.is_empty();

        let (system, speaker_names) = if demo_mode {
            // Demo mode: create mock system and speaker names
            (SonosSystem::from_discovered_devices(vec![]).await?, Self::create_demo_speaker_names())
        } else {
            // Real mode: use discovered devices
            let system = SonosSystem::from_discovered_devices(devices).await?;
            let speaker_names = system.speaker_names().await;
            (system, speaker_names)
        };

        let status_message = if demo_mode {
            "Demo Mode: No speakers found, showing mock data".to_string()
        } else {
            format!("Found {} speaker(s)", speaker_names.len())
        };

        let selected_speaker_name = if demo_mode && !speaker_names.is_empty() {
            Some(speaker_names[0].clone())
        } else if !speaker_names.is_empty() {
            Some(speaker_names[0].clone())
        } else {
            None
        };

        Ok(Self {
            system,
            speaker_names,
            selected_index: 0,
            should_quit: false,
            demo_mode,
            selected_speaker_name,
            volume_watcher: None,
            current_volume: None,
            volume_status: PropertyStatus::Cached,
            current_playback: None,
            playback_status: PropertyStatus::Cached,
            last_navigation: Instant::now(),
            status_message,
        })
    }

    /// Create demo speaker names for when no real devices are found
    fn create_demo_speaker_names() -> Vec<String> {
        vec![
            "Demo Kitchen".to_string(),
            "Demo Living Room".to_string(),
            "Demo Bedroom".to_string(),
        ]
    }

    /// Get the currently selected speaker from the system
    async fn selected_speaker(&self) -> Option<Speaker> {
        if self.demo_mode || self.speaker_names.is_empty() {
            None
        } else if let Some(speaker_name) = self.speaker_names.get(self.selected_index) {
            self.system.get_speaker_by_name(speaker_name).await
        } else {
            None
        }
    }

    /// Handle navigation input
    async fn handle_navigation(&mut self, direction: NavigationDirection) -> Result<(), SdkError> {
        if self.speaker_names.is_empty() && !self.demo_mode {
            return Ok(());
        }

        let speaker_count = if self.demo_mode {
            3 // Mock 3 demo speakers
        } else {
            self.speaker_names.len()
        };

        match direction {
            NavigationDirection::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                } else {
                    self.selected_index = speaker_count - 1;
                }
            }
            NavigationDirection::Down => {
                if self.selected_index < speaker_count - 1 {
                    self.selected_index += 1;
                } else {
                    self.selected_index = 0;
                }
            }
        }

        self.last_navigation = Instant::now();

        // Update cached speaker name for rendering
        self.selected_speaker_name = self.speaker_names.get(self.selected_index).cloned();

        // Update watchers and fetch fresh data for the new selection
        self.update_watchers().await?;
        self.refresh_playback_state().await?;

        Ok(())
    }

    /// Update volume watcher for selected speaker
    async fn update_watchers(&mut self) -> Result<(), SdkError> {
        // Clear existing watcher
        self.volume_watcher = None;
        self.current_volume = None;
        self.volume_status = PropertyStatus::Cached;

        if let Some(speaker) = self.selected_speaker().await {
            // Start watching volume for the selected speaker
            match speaker.volume.watch().await {
                Ok(watcher) => {
                    self.current_volume = watcher.current();
                    self.volume_watcher = Some(watcher);
                    self.volume_status = PropertyStatus::Watching;
                }
                Err(e) => {
                    self.volume_status = PropertyStatus::Error(e.to_string());
                }
            }
        } else if self.demo_mode {
            // Demo mode: simulate volume watching
            self.current_volume = Some(Volume(65 + (self.selected_index * 10) as u8));
            self.volume_status = PropertyStatus::Watching;
        }

        Ok(())
    }

    /// Refresh playback state for selected speaker
    async fn refresh_playback_state(&mut self) -> Result<(), SdkError> {
        if let Some(speaker) = self.selected_speaker().await {
            // Fetch fresh playback state
            match speaker.playback_state.fetch().await {
                Ok(state) => {
                    self.current_playback = Some(state);
                    self.playback_status = PropertyStatus::Fresh;
                }
                Err(e) => {
                    self.playback_status = PropertyStatus::Error(e.to_string());
                }
            }
        } else if self.demo_mode {
            // Demo mode: simulate playback state
            use sonos_state::PlaybackState;
            let states = [PlaybackState::Playing, PlaybackState::Paused, PlaybackState::Stopped];
            self.current_playback = Some(states[self.selected_index % states.len()].clone());
            self.playback_status = PropertyStatus::Fresh;
        }

        Ok(())
    }

    /// Handle volume watcher changes
    async fn handle_volume_change(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(ref mut watcher) = self.volume_watcher {
            if watcher.changed().await.is_ok() {
                self.current_volume = watcher.current();
                self.volume_status = PropertyStatus::Watching;
            }
        }
        Ok(())
    }

    /// Handle keyboard input
    fn handle_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Up => {
                // Handle navigation asynchronously later
            }
            KeyCode::Down => {
                // Handle navigation asynchronously later
            }
            KeyCode::Char(' ') => {
                // Manual refresh - handle asynchronously later
            }
            _ => {}
        }
    }

    /// Render the application UI
    fn render(&self, frame: &mut Frame) {
        // Create layout: left pane (speakers) + right pane (details)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(frame.size());

        self.render_speaker_list(frame, chunks[0]);
        self.render_details(frame, chunks[1]);
    }

    /// Render the speaker list (left pane)
    fn render_speaker_list(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let speakers: Vec<ListItem> = if self.demo_mode {
            // Demo speakers
            let demo_names = ["🍳 Kitchen", "🛋️ Living Room", "🛏️ Bedroom"];
            demo_names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let style = if i == self.selected_index {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(name.to_string()).style(style)
                })
                .collect()
        } else {
            self.speaker_names
                .iter()
                .enumerate()
                .map(|(i, speaker_name)| {
                    let display = format!("🔊 {}", speaker_name);
                    let style = if i == self.selected_index {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(display).style(style)
                })
                .collect()
        };

        let title = if self.demo_mode {
            "📱 Speakers (Demo)"
        } else {
            "📱 Speakers"
        };

        let speakers_list = List::new(speakers)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(Style::default().fg(Color::Yellow));

        frame.render_widget(speakers_list, area);
    }

    /// Render the details pane (right pane)
    fn render_details(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        // Split details into sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Length(5), // Volume
                Constraint::Length(5), // Playback
                Constraint::Min(1),    // Status/Help
            ])
            .split(area);

        // Header
        let speaker_name = self.selected_speaker_name
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "No Speaker Selected".to_string());

        let header = Paragraph::new(format!("🔊 {}", speaker_name))
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
        frame.render_widget(header, chunks[0]);

        // Volume section
        self.render_volume_section(frame, chunks[1]);

        // Playback section
        self.render_playback_section(frame, chunks[2]);

        // Status and help
        self.render_status_help(frame, chunks[3]);
    }

    /// Render volume section with status indicator
    fn render_volume_section(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let (volume_text, volume_value, status_color, status_text) = match (&self.current_volume, &self.volume_status) {
            (Some(vol), PropertyStatus::Watching) => (
                format!("Volume: {}%", vol.0),
                vol.0 as f64 / 100.0,
                Color::Green,
                "👁️ Live - Watched".to_string()
            ),
            (Some(vol), PropertyStatus::Fresh) => (
                format!("Volume: {}%", vol.0),
                vol.0 as f64 / 100.0,
                Color::Blue,
                "🌐 Fresh - Fetched".to_string()
            ),
            (Some(vol), PropertyStatus::Cached) => (
                format!("Volume: {}%", vol.0),
                vol.0 as f64 / 100.0,
                Color::Gray,
                "🔄 Cached".to_string()
            ),
            (None, PropertyStatus::Error(err)) => (
                "Volume: Error".to_string(),
                0.0,
                Color::Red,
                format!("❌ Error: {}", err)
            ),
            _ => ("Volume: --".to_string(), 0.0, Color::Gray, "🔄 No Data".to_string()),
        };

        let volume_block = Block::default()
            .title("🔊 Volume")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(status_color));

        let volume_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
            .margin(1)
            .split(area);

        frame.render_widget(volume_block, area);

        let gauge = Gauge::default()
            .block(Block::default())
            .gauge_style(Style::default().fg(status_color))
            .percent((volume_value * 100.0) as u16)
            .label(volume_text);

        frame.render_widget(gauge, volume_chunks[0]);

        let status = Paragraph::new(status_text)
            .style(Style::default().fg(status_color));
        frame.render_widget(status, volume_chunks[1]);
    }

    /// Render playback section with status indicator
    fn render_playback_section(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let (playback_text, status_color, status_text) = match (&self.current_playback, &self.playback_status) {
            (Some(state), PropertyStatus::Fresh) => (
                format!("Playback: {:?}", state),
                Color::Blue,
                "🌐 Fresh - Fetched".to_string()
            ),
            (Some(state), PropertyStatus::Cached) => (
                format!("Playback: {:?}", state),
                Color::Gray,
                "🔄 Cached".to_string()
            ),
            (None, PropertyStatus::Error(err)) => (
                "Playback: Error".to_string(),
                Color::Red,
                format!("❌ Error: {}", err)
            ),
            _ => ("Playback: --".to_string(), Color::Gray, "🔄 No Data".to_string()),
        };

        let playback_block = Block::default()
            .title("🎵 Playback State")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(status_color));

        let playback_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .margin(1)
            .split(area);

        frame.render_widget(playback_block, area);

        let playback_display = Paragraph::new(playback_text)
            .style(Style::default().fg(Color::White));
        frame.render_widget(playback_display, playback_chunks[0]);

        let status = Paragraph::new(status_text)
            .style(Style::default().fg(status_color));
        frame.render_widget(status, playback_chunks[1]);
    }

    /// Render status and help section
    fn render_status_help(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let help_text = vec![
            "↑↓ Navigate speakers  SPACE Refresh playback  Q Quit",
            "",
            &self.status_message,
        ].join("\n");

        let help = Paragraph::new(help_text)
            .block(Block::default().title("Controls").borders(Borders::ALL))
            .style(Style::default().fg(Color::DarkGray));

        frame.render_widget(help, area);
    }
}

/// Navigation direction for speakers
enum NavigationDirection {
    Up,
    Down,
}

/// Main async event loop
async fn run_app(mut app: SdkTuiApp, mut terminal: Terminal<CrosstermBackend<Stdout>>) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize watchers for the first speaker
    app.update_watchers().await?;
    app.refresh_playback_state().await?;

    loop {
        tokio::select! {
            // Handle keyboard input with timeout
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if event::poll(Duration::from_millis(0))? {
                    if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                        match code {
                            KeyCode::Up => {
                                if let Err(e) = app.handle_navigation(NavigationDirection::Up).await {
                                    app.status_message = format!("Navigation error: {}", e);
                                }
                            }
                            KeyCode::Down => {
                                if let Err(e) = app.handle_navigation(NavigationDirection::Down).await {
                                    app.status_message = format!("Navigation error: {}", e);
                                }
                            }
                            KeyCode::Char(' ') => {
                                if let Err(e) = app.refresh_playback_state().await {
                                    app.status_message = format!("Refresh error: {}", e);
                                }
                            }
                            _ => {
                                app.handle_input(code);
                            }
                        }
                    }
                }
            }

            // Handle volume watcher updates
            result = async {
                if let Some(ref mut watcher) = app.volume_watcher {
                    watcher.changed().await
                } else {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Ok(())
                }
            } => {
                if result.is_ok() {
                    let _ = app.handle_volume_change().await;
                }
            }
        }

        // Render UI
        terminal.draw(|f| app.render(f))?;

        if app.should_quit {
            break;
        }

        // Small sleep to prevent excessive CPU usage
        tokio::time::sleep(Duration::from_millis(16)).await; // ~60 FPS
    }

    Ok(())
}

/// Terminal setup
fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Terminal teardown
fn restore_terminal() -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// Main entry point
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: Initialize logging in silent mode FIRST to prevent TUI interference
    sonos_state::init_silent()?;

    // Discover devices (must be done in blocking context) - now silent
    let devices = sonos_discovery::get();

    // Setup terminal AFTER logging is configured
    let terminal = setup_terminal()?;

    // Create tokio runtime and run the app
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(async {
        match SdkTuiApp::new(devices).await {
            Ok(app) => run_app(app, terminal).await,
            Err(e) => {
                restore_terminal()?;
                Err(format!("Failed to initialize app: {}", e).into())
            }
        }
    });

    // Restore terminal
    restore_terminal()?;

    // Now safe to print to stdout after terminal restoration
    match result {
        Ok(_) => {
            println!("🎵 Sonos SDK - Interactive TUI Example");
            println!("✨ Thanks for using the Sonos SDK TUI!");
        },
        Err(e) => {
            println!("🎵 Sonos SDK - Interactive TUI Example");
            println!("❌ Error: {}", e);
        },
    }

    Ok(())
}