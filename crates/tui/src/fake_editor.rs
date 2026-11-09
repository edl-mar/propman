//! A stand-in for the editor, to try by hand how the program looks when the editor fails,
//! without changing anything on the computer. It is used when `PROPMAN_FAKE_EDITOR` is set: the
//! screen shows what the keys do and waits for one of them.

use std::path::Path;

use app::EditorOutcome;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

const KEYS: &str = "fake editor   1: no editor   2: could not start   3: exit code 1   \
                    4: ended with an error   5: done   (esc: done)";

/// The way the fake editor ends for a key; `None` for any other key.
pub fn outcome_for_key(code: KeyCode) -> Option<EditorOutcome> {
    match code {
        KeyCode::Char('1') => Some(EditorOutcome::NoEditor),
        KeyCode::Char('2') => Some(EditorOutcome::CouldNotStart),
        KeyCode::Char('3') => Some(EditorOutcome::Failed(Some(1))),
        KeyCode::Char('4') => Some(EditorOutcome::Failed(None)),
        KeyCode::Char('5') | KeyCode::Esc => Some(EditorOutcome::Done),
        _ => None,
    }
}

/// Shows the keys and waits for one that ends the fake editor. The file is not touched.
pub fn fake_editor<B: Backend>(terminal: &mut Terminal<B>, _path: &Path) -> EditorOutcome {
    let _ = terminal.draw(|frame| {
        let area = frame.area();
        if area.height > 0 {
            let row = Rect {
                y: area.bottom() - 1,
                height: 1,
                ..area
            };
            frame.render_widget(Paragraph::new(KEYS), row);
        }
    });
    loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                if let Some(outcome) = outcome_for_key(key.code) {
                    return outcome;
                }
            }
            Ok(_) => {}
            Err(_) => return EditorOutcome::CouldNotStart,
        }
    }
}
