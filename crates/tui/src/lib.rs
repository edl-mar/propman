//! Views, rendering and input. Reads and writes only through `app`.
//!
//! `render` draws the state and changes nothing. `message_for` turns a terminal event into a
//! message, or into nothing, by the keymap of the `Ui`; the `Ui` also holds the theme, and the
//! line with the keys is made from the keymap. Nothing else knows a key or a colour. `run_loop` is the event loop; it gets its source of events from the
//! outside, so that it can be driven by a fixed list of events and a test buffer instead of a
//! terminal. `run` is the same loop on the real terminal.
//!
//! The screen has a title, a line with the count, a status line (the count of the problems and
//! the notice, only if there is something to say), the list of the bundle overview and a line
//! with the keys. The list shows the part that
//! the state scrolled to. An item is marked as follows, and never by colour alone: the selected
//! one by `>`, an uncertain file by `?`, an excluded bundle by `[excluded]`; the styles of the
//! theme come on top. On a screen at least 100 columns wide the list gets the left half and the
//! preview of the selected file the right half, behind a column with `|`.

mod editor;
mod keys;
mod ui;

use std::io;
use std::path::Path;

pub use editor::{EditorCommand, editor_command};
pub use keys::{Code, Context, Key, Keymap};
pub use ui::{Theme, Ui};

use app::{
    AppState, Confidence, EditorOutcome, Encoding, Message, OverviewItem, View, overview, perform,
    preview, update,
};
use ratatui::backend::Backend;
use ratatui::crossterm::event::Event;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
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

/// The preview is drawn only on a screen at least this many columns wide.
const PREVIEW_MIN_WIDTH: u16 = 100;

/// The lines above the list (title and count) and below it (the keys).
const HEADER_LINES: usize = 2;
const FOOTER_LINES: usize = 1;

/// How many items of the overview fit on a screen of this height: what is left after the title,
/// the count, the status line (only if there is one) and the line with the keys.
pub fn list_height(screen_height: u16, has_status: bool) -> usize {
    usize::from(screen_height).saturating_sub(HEADER_LINES + usize::from(has_status) + FOOTER_LINES)
}

/// The status line: the count of the problems and the notice, if there are any.
fn status_line(state: &AppState) -> Option<String> {
    let mut parts = Vec::new();
    if !state.problems().is_empty() {
        parts.push(counted(state.problems().len(), "problem", "problems"));
    }
    if let Some(notice) = state.notice() {
        parts.push(notice.to_owned());
    }
    (!parts.is_empty()).then(|| parts.join("   "))
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
    theme: &Theme,
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
                theme.folder,
            )
        }
        OverviewItem::Bundle {
            name,
            files,
            excluded,
            ..
        } => {
            let mut text = format!("{mark}  {name}  {}", counted(*files, "file", "files"));
            let mut style = theme.bundle;
            if *excluded {
                text.push_str("  [excluded]");
                style = style.patch(theme.excluded);
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
                    theme.file.patch(theme.uncertain)
                } else {
                    theme.file
                },
            )
        }
    };
    let style = if selected {
        style.patch(theme.selected)
    } else {
        style
    };
    Line::styled(text, style)
}

/// Draws the state into the frame.
pub fn render(ui: &Ui, state: &AppState, frame: &mut Frame) {
    let status = status_line(state);
    let [title, count, status_area, list, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(u16::from(status.is_some())),
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
    if let Some(status) = status {
        frame.render_widget(Paragraph::new(status), status_area);
    }

    let View::Overview(view) = state.view();
    let (list, preview_area) = if view.preview_on() && frame.area().width >= PREVIEW_MIN_WIDTH {
        let [left, separator, right] = Layout::horizontal([
            Constraint::Length(list.width / 2),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(list);
        let bar: Vec<Line> = (0..separator.height).map(|_| Line::raw("|")).collect();
        frame.render_widget(Paragraph::new(bar), separator);
        (left, Some(right))
    } else {
        (list, None)
    };
    if let Some(area) = preview_area {
        let lines = preview_lines(&ui.theme, state, usize::from(area.width));
        let lines: Vec<Line> = lines.into_iter().take(usize::from(area.height)).collect();
        frame.render_widget(Paragraph::new(lines), area);
    }

    let items = overview(session);
    if items.is_empty() {
        frame.render_widget(Paragraph::new("no .properties files"), list);
    } else {
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
                    &ui.theme,
                    item,
                    index == view.selected(),
                    usize::from(list.width),
                    name_width,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), list);
    }

    frame.render_widget(Paragraph::new(key_help(&ui.keymap, state.view())), footer);
}

/// The columns of the line ending in the preview: wide enough for `(none)`.
const ENDING_WIDTH: usize = 6;

/// The text padded with spaces or, if it is longer, cut at the right end with `...`.
fn fit_right(text: &str, available: usize) -> String {
    let length = text.chars().count();
    if length <= available {
        return format!("{text:<available$}");
    }
    if available <= 3 {
        return ".".repeat(available);
    }
    let head: String = text.chars().take(available - 3).collect();
    format!("{head}...")
}

/// The rows of the preview pane: a header and the lines of the window, or the hint that a file
/// has to be selected. Every row starts with a space behind the separator.
fn preview_lines(theme: &Theme, state: &AppState, width: usize) -> Vec<Line<'static>> {
    let Some(preview) = preview(state) else {
        return vec![Line::raw(" select a file to preview it")];
    };
    let mut rows = vec![Line::raw(format!(
        " {}  {}  {}",
        preview.name,
        encoding_name(preview.encoding),
        counted(preview.total, "line", "lines"),
    ))];
    let digits = preview.total.to_string().len();
    let text_width = width.saturating_sub(1 + digits + 1 + 1 + ENDING_WIDTH);
    for line in &preview.lines {
        rows.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(format!("{:>digits$}", line.number), theme.line_number),
            Span::raw(format!(" {} ", fit_right(&line.text, text_width))),
            Span::styled(format!("{:>ENDING_WIDTH$}", line.ending), theme.ending),
        ]));
    }
    rows
}

/// What the keys do, as far as they are bound: `q/ctrl+c: quit   up/k/down/j: move ...`.
fn key_help(keymap: &Keymap, view: &View) -> String {
    let context = Context::of(view);
    let groups: [(&[Message], &str); 7] = [
        (&[Message::Quit], "quit"),
        (&[Message::Edit], "edit"),
        (&[Message::Up, Message::Down], "move"),
        (&[Message::PageUp, Message::PageDown], "page"),
        (&[Message::First, Message::Last], "ends"),
        (&[Message::TogglePreview], "preview"),
        (
            &[Message::PreviewHalfPageUp, Message::PreviewHalfPageDown],
            "scroll preview",
        ),
    ];
    groups
        .iter()
        .filter_map(|(messages, label)| {
            let mut keys: Vec<Key> = Vec::new();
            for key in messages
                .iter()
                .flat_map(|&message| keymap.keys_for(context, message))
            {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            if keys.is_empty() {
                return None;
            }
            let keys: Vec<String> = keys.iter().map(Key::to_string).collect();
            Some(format!("{}: {label}", keys.join("/")))
        })
        .collect::<Vec<_>>()
        .join("   ")
}

/// The message for an event, if there is one. Only the press of a key counts, not its release;
/// a character with Ctrl and Alt (what AltGr sends) is a plain character.
pub fn message_for(ui: &Ui, view: &View, event: &Event) -> Option<Message> {
    let key = Key::from_event(event)?;
    ui.keymap.message(Context::of(view), key)
}

/// Tells the state how many items fit, draws, waits for an event, updates the state, and repeats
/// until the state asks to quit. The height is told again whenever the size of the terminal
/// changes. Returns the final state.
pub fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    ui: &Ui,
    mut state: AppState,
    mut next_event: impl FnMut() -> io::Result<Event>,
    mut edit: impl FnMut(&mut Terminal<B>, &Path) -> EditorOutcome,
) -> Result<AppState, LoopError<B::Error>> {
    let mut told: Option<usize> = None;
    loop {
        let size = terminal.size().map_err(LoopError::Draw)?;
        let height = list_height(size.height, status_line(&state).is_some());
        if told != Some(height) {
            state = update(state, Message::Viewport(height)).0;
            told = Some(height);
        }
        terminal
            .draw(|frame| render(ui, &state, frame))
            .map_err(LoopError::Draw)?;
        if state.should_quit() {
            return Ok(state);
        }
        let event = next_event().map_err(LoopError::Events)?;
        if let Some(message) = message_for(ui, state.view(), &event) {
            let (next, mut effect) = update(state, message);
            state = next;
            // an effect brings a message about its result, which can bring the next effect
            while let Some(current) = effect {
                let (next, result) = perform(state, current, |path| edit(terminal, path));
                let (next, following) = update(next, result);
                state = next;
                effect = following;
            }
        }
    }
}

/// Runs the editor of the user on a file: leaves the terminal, waits until the editor ends and
/// enters the terminal again. Nothing happens to the terminal if there is no editor to run.
fn edit_in_terminal(terminal: &mut ratatui::DefaultTerminal, path: &Path) -> EditorOutcome {
    let visual = std::env::var("VISUAL").ok();
    let editor = std::env::var("EDITOR").ok();
    let Some(command) = editor_command(visual.as_deref(), editor.as_deref(), path) else {
        return EditorOutcome::NoEditor;
    };
    ratatui::restore();
    let status = std::process::Command::new(&command.program)
        .args(&command.args)
        .status();
    *terminal = ratatui::init();
    match status {
        Err(_) => EditorOutcome::CouldNotStart,
        Ok(status) if status.success() => EditorOutcome::Done,
        Ok(status) => EditorOutcome::Failed(status.code()),
    }
}

/// The same loop on the real terminal, which is restored when the loop ends or fails.
pub fn run(ui: &Ui, state: AppState) -> io::Result<AppState> {
    let mut terminal = ratatui::init();
    let result = run_loop(
        &mut terminal,
        ui,
        state,
        ratatui::crossterm::event::read,
        edit_in_terminal,
    );
    ratatui::restore();
    result.map_err(|error| match error {
        LoopError::Draw(e) | LoopError::Events(e) => e,
    })
}
