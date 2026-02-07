//! Temperature Monitor - Minimal state-store demo
//!
//! Shows event-driven rendering: re-renders only on keypress or watched property changes.
//!
//! Run: cargo run -p state-store --example temperature_monitor

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    widgets::{Block, Gauge, List, ListItem, Paragraph},
    Frame,
};
use state_store::{Property, StateStore};
use std::io;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};
use std::thread;
use std::time::Duration;

#[derive(Clone, PartialEq)]
struct Temperature(u8);

impl Property for Temperature {
    const KEY: &'static str = "temp";
}

const ROOMS: [&str; 4] = ["kitchen", "bedroom", "garage", "bathroom"];

enum Trigger {
    Key(KeyCode),
    State(String, u8),
}

fn main() -> io::Result<()> {
    let store = StateStore::<String>::new();
    for room in ROOMS {
        store.set(&room.to_string(), Temperature(70));
    }

    let running = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::channel();

    // Keyboard thread
    let tx_k = tx.clone();
    let run_k = running.clone();
    thread::spawn(move || {
        while run_k.load(Ordering::SeqCst) {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(k)) = event::read() {
                    if k.kind == KeyEventKind::Press {
                        let _ = tx_k.send(Trigger::Key(k.code));
                    }
                }
            }
        }
    });

    // State event thread
    let tx_s = tx.clone();
    let store_s = store.clone();
    let run_s = running.clone();
    thread::spawn(move || {
        let iter = store_s.iter();
        while run_s.load(Ordering::SeqCst) {
            if let Some(e) = iter.recv_timeout(Duration::from_millis(50)) {
                if let Some(Temperature(t)) = store_s.get::<Temperature>(&e.entity_id) {
                    let _ = tx_s.send(Trigger::State(e.entity_id, t));
                }
            }
        }
    });

    // Simulation thread
    let store_sim = store.clone();
    let run_sim = running.clone();
    thread::spawn(move || {
        let mut i = 0u64;
        while run_sim.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(400));
            let room = ROOMS[i as usize % ROOMS.len()].to_string();
            if let Some(Temperature(t)) = store_sim.get::<Temperature>(&room) {
                let new_t = if i % 3 == 0 { t.saturating_add(1).min(99) } else { t.saturating_sub(1).max(40) };
                store_sim.set(&room, Temperature(new_t));
            }
            i = i.wrapping_add(1);
        }
    });

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut term = ratatui::init();

    let mut selected = 0usize;
    let mut renders = 1u64;
    let mut log: Vec<String> = vec!["SPACE=watch/unwatch  q=quit".into()];

    term.draw(|f| draw(f, &store, selected, renders, &log))?;

    loop {
        match rx.recv() {
            Ok(Trigger::Key(key)) => {
                match key {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => selected = (selected + 1).min(ROOMS.len() - 1),
                    KeyCode::Char(' ') => {
                        let room = ROOMS[selected].to_string();
                        if store.is_watched(&room, Temperature::KEY) {
                            store.unwatch(&room, Temperature::KEY);
                            log.push(format!("Unwatched: {}", room));
                        } else {
                            store.watch(room.clone(), Temperature::KEY);
                            log.push(format!("Watching: {}", room));
                        }
                    }
                    _ => continue,
                }
                renders += 1;
                term.draw(|f| draw(f, &store, selected, renders, &log))?;
            }
            Ok(Trigger::State(room, temp)) => {
                log.push(format!("{}: {}°", room, temp));
                if log.len() > 8 { log.remove(0); }
                renders += 1;
                term.draw(|f| draw(f, &store, selected, renders, &log))?;
            }
            Err(_) => break,
        }
    }

    running.store(false, Ordering::SeqCst);
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn draw(f: &mut Frame, store: &StateStore<String>, selected: usize, renders: u64, log: &[String]) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(ROOMS.len() as u16 * 2 + 2),
        Constraint::Min(5),
    ]).split(f.area());

    f.render_widget(
        Paragraph::new(format!("Renders: {}", renders))
            .block(Block::bordered().title("Temperature Monitor")),
        chunks[0],
    );

    let room_chunks = Layout::vertical(ROOMS.iter().map(|_| Constraint::Length(2)).collect::<Vec<_>>())
        .split(Block::bordered().title("Rooms").inner(chunks[1]));
    f.render_widget(Block::bordered().title("Rooms"), chunks[1]);

    for (i, room) in ROOMS.iter().enumerate() {
        let temp = store.get::<Temperature>(&room.to_string()).map(|t| t.0).unwrap_or(0);
        let watched = store.is_watched(&room.to_string(), Temperature::KEY);
        let sel = if i == selected { "▶" } else { " " };
        let eye = if watched { "👁" } else { " " };
        let color = if temp < 60 { Color::Cyan } else if temp < 80 { Color::Green } else { Color::Red };

        f.render_widget(
            Gauge::default()
                .gauge_style(Style::default().fg(color))
                .percent(temp.saturating_sub(40).min(60) as u16 * 100 / 60)
                .label(format!("{}{} {:<10} {}°", sel, eye, room, temp)),
            room_chunks[i],
        );
    }

    let items: Vec<ListItem> = log.iter().rev().map(|s| ListItem::new(s.as_str()).fg(Color::Gray)).collect();
    f.render_widget(List::new(items).block(Block::bordered().title("Events")), chunks[2]);
}
