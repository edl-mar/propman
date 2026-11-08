//! Views, rendering and input. Reads and writes only through `app`.
//!
//! `render` draws the state and changes nothing. `message_for` turns a terminal event into a
//! message, or into nothing. `run_loop` is the event loop; it gets its source of events from the
//! outside, so that it can be driven by a fixed list of events and a test buffer instead of a
//! terminal. `run` is the same loop on the real terminal.
//!
//! The screen has a title, a line with the count, a line with the problems (only if there are
//! any), the list of the bundle overview and a line with the keys. The list shows the part that
//! the state scrolled to. An item is marked as follows, and never by colour alone: the selected
//! one by `>` (and reversed), an uncertain file by `?` (and yellow), an excluded bundle by
//! `[excluded]` (and dimmed).

use std::io;

use app::{AppState, Confidence, Encoding, Message, OverviewItem, View, overview, update};
use ratatui::backend::Backend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
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

/// The lines above the list (title and count) and below it (the keys).
const HEADER_LINES: usize = 2;
const FOOTER_LINES: usize = 1;

/// How many items of the overview fit on a screen of this height: what is left after the title,
/// the count, the line for the problems (only if there are any) and the line with the keys.
pub fn list_height(screen_height: u16, has_problems: bool) -> usize {
    usize::from(screen_height)
        .saturating_sub(HEADER_LINES + usize::from(has_problems) + FOOTER_LINES)
}

/// "1 file", "12 files".
fn counted(number: usize, singular: &str, plural: &str) -> String {
    let word = if number == 1 { singular } else { plural };
    format!("{number} {word}")
}

fn encoding_name(encoding: Encoding) -> &'static str {
    match encoding {
        Encoding::Utf8 => "UTF-8",
        Encoding::Utf8Bom => "UTF-8+BOM",
        Encoding::Windows1252 => "Windows-1252",
        Encoding::Latin1 => "Latin-1",
    }
}

/// The text cut from the left to `available` characters: `...` and the end of the text.
fn fit_left(text: &str, available: usize) -> String {
    let length = text.chars().count();
    if length <= available {
        return text.to_owned();
    }
    if available <= 3 {
        return ".".repeat(available);
    }
    let tail: String = text.chars().skip(length - (available - 3)).collect();
    format!("...{tail}")
}

/// One line of the list. A folder is indented by 0, a bundle by 2 and a file by 4 characters
/// behind the two characters of the selection mark.
fn item_line(
    item: &OverviewItem,
    selected: bool,
    width: usize,
    name_width: usize,
) -> Line<'static> {
    let mark = if selected { "> " } else { "  " };
    let (text, style) = match item {
        OverviewItem::Folder { path, bundles } => {
            let count = counted(*bundles, "bundle", "bundles");
            let available = width.saturating_sub(mark.len() + 2 + count.chars().count());
            (
                format!("{mark}{}  {count}", fit_left(path, available)),
                Style::new().add_modifier(Modifier::BOLD),
            )
        }
        OverviewItem::Bundle {
            name,
            files,
            excluded,
            ..
        } => {
            let mut text = format!("{mark}  {name}  {}", counted(*files, "file", "files"));
            let mut style = Style::new();
            if *excluded {
                text.push_str("  [excluded]");
                style = style.fg(Color::DarkGray);
            }
            (text, style)
        }
        OverviewItem::File {
            name,
            locale,
            confidence,
            encoding,
            lines,
            ..
        } => {
            let uncertain = *confidence == Confidence::Uncertain;
            let locale = locale.as_deref().unwrap_or("(default)");
            let locale = if uncertain {
                format!("{locale} ?")
            } else {
                locale.to_owned()
            };
            (
                format!(
                    "{mark}    {locale:<12}  {name:<name_width$}  {:<12}  {}",
                    encoding_name(*encoding),
                    counted(*lines, "line", "lines")
                ),
                if uncertain {
                    Style::new().fg(Color::Yellow)
                } else {
                    Style::new()
                },
            )
        }
    };
    let style = if selected {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    };
    Line::styled(text, style)
}

/// Draws the state into the frame.
pub fn render(state: &AppState, frame: &mut Frame) {
    let has_problems = !state.problems().is_empty();
    let [title, count, problems, list, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(u16::from(has_problems)),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let session = state.session();
    frame.render_widget(
        Paragraph::new(format!("propman \u{2014} {}", session.root().display())),
        title,
    );

    frame.render_widget(
        Paragraph::new(format!(
            "{} in {}",
            counted(session.file_count(), "file", "files"),
            counted(session.bundle_count(), "bundle", "bundles"),
        )),
        count,
    );
    if has_problems {
        frame.render_widget(
            Paragraph::new(counted(state.problems().len(), "problem", "problems")),
            problems,
        );
    }

    let items = overview(session);
    if items.is_empty() {
        frame.render_widget(Paragraph::new("no .properties files"), list);
    } else {
        let View::Overview(view) = state.view();
        let name_width = items
            .iter()
            .filter_map(|item| match item {
                OverviewItem::File { name, .. } => Some(name.chars().count()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let lines: Vec<Line> = items
            .iter()
            .enumerate()
            .skip(view.scroll())
            .take(usize::from(list.height))
            .map(|(index, item)| {
                item_line(
                    item,
                    index == view.selected(),
                    usize::from(list.width),
                    name_width,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), list);
    }

    frame.render_widget(
        Paragraph::new("q: quit   j/k or arrows: move   PgUp/PgDn: page   Home/End: ends"),
        footer,
    );
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
        (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => Some(Message::Up),
        (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => Some(Message::Down),
        (KeyCode::PageUp, KeyModifiers::NONE) => Some(Message::PageUp),
        (KeyCode::PageDown, KeyModifiers::NONE) => Some(Message::PageDown),
        (KeyCode::Home, KeyModifiers::NONE) => Some(Message::First),
        (KeyCode::End, KeyModifiers::NONE) => Some(Message::Last),
        _ => None,
    }
}

/// Tells the state how many items fit, draws, waits for an event, updates the state, and repeats
/// until the state asks to quit. The height is told again whenever the size of the terminal
/// changes. Returns the final state.
pub fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    mut state: AppState,
    mut next_event: impl FnMut() -> io::Result<Event>,
) -> Result<AppState, LoopError<B::Error>> {
    let mut told: Option<usize> = None;
    loop {
        let size = terminal.size().map_err(LoopError::Draw)?;
        let height = list_height(size.height, !state.problems().is_empty());
        if told != Some(height) {
            state = update(state, Message::Viewport(height));
            told = Some(height);
        }
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
