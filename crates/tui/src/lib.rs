//! Views, rendering and input. Reads and writes only through `app`.
//!
//! `render` draws the state and changes nothing. `message_for` turns a terminal event into a
//! message, or into nothing. `run_loop` is the event loop; it gets its source of events from the
//! outside, so that it can be driven by a fixed list of events and a test buffer instead of a
//! terminal. `run` is the same loop on the real terminal.

use std::io;

use app::{AppState, Message, update};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};

/// Why the event loop stopped with an error.
#[derive(Debug)]
pub enum LoopError<E> {
    /// The backend could not draw.
    Draw(E),
    /// The source of events failed.
    Events(io::Error),
}

/// "1 file", "12 files".
fn counted(number: usize, singular: &str, plural: &str) -> String {
    let word = if number == 1 { singular } else { plural };
    format!("{number} {word}")
}

/// Draws the state into the frame.
pub fn render(state: &AppState, frame: &mut Frame) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let session = state.session();
    frame.render_widget(
        Paragraph::new(format!("propman \u{2014} {}", session.root().display())),
        header,
    );

    let store = session.store();
    let mut lines = vec![format!(
        "{} in {}",
        counted(store.files().len(), "file", "files"),
        counted(store.bundles().len(), "bundle", "bundles"),
    )];
    if !state.problems().is_empty() {
        lines.push(counted(state.problems().len(), "problem", "problems"));
    }
    frame.render_widget(Paragraph::new(lines.join("\n")), body);

    frame.render_widget(Paragraph::new("q: quit"), footer);
}

/// The message for an event, if there is one. Only the press of a key counts, not its release;
/// a character with Ctrl and Alt (what AltGr sends) is a plain character.
pub fn message_for(event: &Event) -> Option<Message> {
    let Event::Key(key) = event else {
        return None;
    };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    let altgr = key
        .modifiers
        .contains(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let modifiers = if altgr {
        KeyModifiers::NONE
    } else {
        key.modifiers
    };
    match (key.code, modifiers) {
        (KeyCode::Char('q'), KeyModifiers::NONE) => Some(Message::Quit),
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Message::Quit),
        _ => None,
    }
}

/// Draws, waits for an event, updates the state, and repeats until the state asks to quit.
/// Returns the final state.
pub fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    mut state: AppState,
    mut next_event: impl FnMut() -> io::Result<Event>,
) -> Result<AppState, LoopError<B::Error>> {
    loop {
        terminal
            .draw(|frame| render(&state, frame))
            .map_err(LoopError::Draw)?;
        if state.should_quit() {
            return Ok(state);
        }
        let event = next_event().map_err(LoopError::Events)?;
        if let Some(message) = message_for(&event) {
            state = update(state, message);
        }
    }
}

/// The same loop on the real terminal, which is restored when the loop ends or fails.
pub fn run(state: AppState) -> io::Result<AppState> {
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, state, ratatui::crossterm::event::read);
    ratatui::restore();
    result.map_err(|error| match error {
        LoopError::Draw(e) | LoopError::Events(e) => e,
    })
}
