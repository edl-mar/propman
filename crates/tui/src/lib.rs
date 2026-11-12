//! Views, rendering and input. Reads and writes only through `app`.
//!
//! `render` draws the state and changes nothing. `message_for` turns a terminal event into a
//! message, or into nothing, by the keymap of the `Ui`; the `Ui` also holds the theme, and the
//! line with the keys is made from the keymap. Nothing else knows a key or a colour. `run_loop`
//! is the event loop; it gets its source of events from the outside, so that it can be driven by
//! a fixed list of events and a test buffer instead of a terminal. `run` is the same loop on the
//! real terminal.
//!
//! The screen has a title, a line with the count, a status line (the count of the problems, only
//! if there are any), the list of the bundle overview and a line with the keys. The list shows
//! the part that the state scrolled to. An alert is a box over the top of the screen; it does
//! not move anything. An item is marked as follows, and never by colour alone: the one under the cursor
//! one by `>`, an uncertain file by `?`, an excluded bundle by `[excluded]`; the styles of the
//! theme come on top. On a screen at least 100 columns wide the list gets the left half and the
//! preview of the file under the cursor the right half, behind a column with `|`.

mod editor;
mod fake_editor;
mod keys;
mod screen;
mod ui;

use std::io;
use std::path::Path;

pub use editor::{EditorCommand, editor_command};
pub use fake_editor::outcome_for_key;
pub use keys::{Code, Context, Key, Keymap};
pub use screen::screen_text;
pub use ui::{Theme, Ui};

use app::{
    Action, AppState, Command, Confidence, EditorOutcome, Entry, ItemId, Message, OverviewItem,
    TextEdit, Value, View, applicable, candidates, commands, encoding_name, overview, perform,
    preview, summary, target, update,
};
use ratatui::backend::Backend;
use ratatui::crossterm::event::Event;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
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

/// "2 files marked", or nothing if nothing is marked.
fn marked_text(marked: &[ItemId]) -> Option<String> {
    let (singular, plural) = match marked.first()? {
        ItemId::Folder(_) => ("folder", "folders"),
        ItemId::Bundle(_) => ("bundle", "bundles"),
        ItemId::File(_) => ("file", "files"),
    };
    Some(format!(
        "{} marked",
        counted(marked.len(), singular, plural)
    ))
}

/// The status line: the count of the problems, if there are any.
fn status_line(state: &AppState) -> Option<String> {
    (!state.problems().is_empty()).then(|| counted(state.problems().len(), "problem", "problems"))
}

/// The text broken into lines of at most `width` characters, at spaces where possible.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_owned();
        loop {
            let length = word.chars().count();
            let needed = if current.is_empty() {
                length
            } else {
                current.chars().count() + 1 + length
            };
            if needed <= width {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(&word);
                break;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                continue;
            }
            // a word that is longer than a line
            lines.push(word.chars().take(width).collect());
            word = word.chars().skip(width).collect();
            if word.is_empty() {
                break;
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// The alert as a box over the top of the screen, as high as its text needs.
fn render_alert(theme: &Theme, keymap: &Keymap, text: &str, frame: &mut Frame) {
    let screen = frame.area();
    let lines = wrap(text, usize::from(screen.width.saturating_sub(2)));
    let height = u16::try_from(lines.len() + 2)
        .unwrap_or(u16::MAX)
        .min(screen.height);
    let area = Rect { height, ..screen };
    let keys: Vec<String> = keymap
        .keys_for(Context::Alert, Message::DismissAlert)
        .iter()
        .map(Key::to_string)
        .collect();
    let mut block = Block::bordered().title(" error ").style(theme.alert);
    if !keys.is_empty() {
        block = block.title_bottom(format!(" {}: dismiss ", keys.join("/")));
    }
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let lines: Vec<Line> = lines.into_iter().map(Line::from).collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// "1 file", "12 files".
fn counted(number: usize, singular: &str, plural: &str) -> String {
    let word = if number == 1 { singular } else { plural };
    format!("{number} {word}")
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
/// behind the two characters of the cursor and the mark (`>` and `*`).
fn item_line(
    theme: &Theme,
    item: &OverviewItem,
    at_cursor: bool,
    marked: bool,
    width: usize,
    name_width: usize,
) -> Line<'static> {
    let mark = format!(
        "{}{}",
        if at_cursor { '>' } else { ' ' },
        if marked { '*' } else { ' ' }
    );
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
    let style = if marked {
        style.patch(theme.marked)
    } else {
        style
    };
    let style = if at_cursor {
        style.patch(theme.cursor)
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

    let View::Overview(view) = state.view();
    let mut counts = format!(
        "{} in {}",
        counted(session.file_count(), "file", "files"),
        counted(session.bundle_count(), "bundle", "bundles"),
    );
    if let Some(marked) = marked_text(view.marked()) {
        counts.push_str(&format!(" \u{b7} {marked}"));
    }
    frame.render_widget(Paragraph::new(counts), count);
    if let Some(status) = status {
        frame.render_widget(Paragraph::new(status), status_area);
    }

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
                    index == view.index(),
                    view.is_marked(&item.id()),
                    usize::from(list.width),
                    name_width,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), list);
    }

    frame.render_widget(Paragraph::new(key_help(&ui.keymap, state)), footer);

    render_command_line(ui, state, frame);
    if let Some(text) = state.alert() {
        render_alert(&ui.theme, &ui.keymap, text, frame);
    }
}

/// How many candidates of the command line are shown at most.
const MAX_CANDIDATES: usize = 8;

/// The context below the overlays: the view.
fn view_context(state: &AppState) -> Context {
    match state.view() {
        View::Overview(_) => Context::Overview,
    }
}

/// The command line over the bottom of the screen: the input in the last row, behind it the keys
/// of the line, and above it the candidates, the best one in the row next to the input, and on
/// top a row that says what the line works on and what it would do.
fn render_command_line(ui: &Ui, state: &AppState, frame: &mut Frame) {
    let Some(line) = state.command_line() else {
        return;
    };
    let screen = frame.area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    let aim = target(state).map(|target| match summary(state) {
        Some(summary) => format!("on: {target}   {summary}"),
        None => format!("on: {target}"),
    });
    // the row on top needs a row of its own besides the input
    let aim_rows = usize::from(aim.is_some() && screen.height > 1);
    let found = candidates(state);
    let available = usize::from(screen.height - 1)
        .saturating_sub(aim_rows)
        .min(MAX_CANDIDATES);
    // the highlighted candidate is always among the shown ones
    let first = (line.highlighted() + 1).saturating_sub(available);
    let shown = found.len().saturating_sub(first).min(available);
    let rows = if found.is_empty() {
        available.min(1)
    } else {
        shown
    };
    let input_y = screen.bottom() - 1;
    let rows_u16 = u16::try_from(rows).unwrap_or(u16::MAX);
    let aim_u16 = u16::try_from(aim_rows).unwrap_or(1);
    let block = Rect {
        y: input_y - rows_u16 - aim_u16,
        height: rows_u16 + aim_u16 + 1,
        ..screen
    };
    frame.render_widget(Clear, block);

    let row_at = |y: u16| Rect {
        y,
        height: 1,
        ..screen
    };
    if let (Some(aim), 1) = (aim, aim_rows) {
        frame.render_widget(
            Paragraph::new(aim).style(ui.theme.target),
            row_at(input_y - rows_u16 - 1),
        );
    }
    if found.is_empty() && rows > 0 {
        frame.render_widget(Paragraph::new("  no matching command"), row_at(input_y - 1));
    }
    let name_width = commands()
        .iter()
        .flat_map(|command| {
            let values: &[Value] = match &command.action {
                Action::Choose(values) => values,
                Action::Message(_) => &[],
            };
            std::iter::once(command.name).chain(values.iter().map(|value| value.name))
        })
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0);
    let label_width = commands()
        .iter()
        .map(|command| command.label.chars().count())
        .max()
        .unwrap_or(0);
    let below = view_context(state);
    for (index, candidate) in found.iter().enumerate().skip(first).take(shown) {
        let at_cursor = index == line.highlighted();
        let name = candidate.name;
        let mut spans = vec![Span::raw(if at_cursor { "> " } else { "  " })];
        for (place, letter) in name.chars().enumerate() {
            let style = if candidate.matched.contains(&place) {
                ui.theme.matched
            } else {
                Style::new()
            };
            spans.push(Span::styled(letter.to_string(), style));
        }
        // the keys of a command that is done at once; a command with values has none
        let keys: Vec<String> = match candidate.entry {
            Entry::Command(Command {
                action: Action::Message(message),
                ..
            }) => ui
                .keymap
                .keys_for(below, *message)
                .iter()
                .map(Key::to_string)
                .collect(),
            _ => Vec::new(),
        };
        spans.push(Span::raw(format!(
            "{}  {:<label_width$}  {}",
            " ".repeat(name_width.saturating_sub(name.chars().count())),
            candidate.label,
            keys.join("/"),
        )));
        let style = if at_cursor {
            ui.theme.cursor
        } else {
            Style::new()
        };
        let y = input_y - 1 - u16::try_from(index - first).unwrap_or(u16::MAX);
        frame.render_widget(Paragraph::new(Line::from(spans).style(style)), row_at(y));
    }

    // the input: the segment that was chosen, in its own style, and the field behind it
    let chosen = line.chosen().map_or("", |command| command.name);
    let text = line.field().text();
    let mut input = vec![Span::raw(":")];
    let mut prefix = 1;
    if !chosen.is_empty() {
        input.push(Span::styled(chosen.to_owned(), ui.theme.token));
        input.push(Span::raw(" "));
        prefix += chosen.chars().count() + 1;
    }
    input.push(Span::raw(text.to_owned()));
    frame.render_widget(Paragraph::new(Line::from(input)), row_at(input_y));
    // the keys of the line, at the right edge, if there is room behind the input
    let used = u16::try_from(prefix + text.chars().count() + 2).unwrap_or(u16::MAX);
    if used < screen.width {
        let hints = Rect {
            x: screen.x + used,
            y: input_y,
            width: screen.width - used,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(key_help(&ui.keymap, state)).alignment(Alignment::Right),
            hints,
        );
    }
    let caret = u16::try_from(prefix + line.field().cursor()).unwrap_or(u16::MAX);
    frame.set_cursor_position(Position::new(
        screen.x + caret.min(screen.width - 1),
        input_y,
    ));
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

/// What the keys do, as far as they are bound and can be done now: `q/ctrl+c: quit   up/k/down/j: move ...`.
fn key_help(keymap: &Keymap, state: &AppState) -> String {
    let context = Context::of(state);
    let groups: [(&[Message], &str); 16] = [
        (&[Message::Quit], "quit"),
        (&[Message::DismissAlert], "dismiss"),
        (&[Message::CloseCommandLine], "close"),
        (&[Message::RunCommand], "run"),
        (&[Message::CompleteCommand], "complete"),
        (&[Message::PreviousSegment], "back"),
        (
            &[Message::NextCandidate, Message::PreviousCandidate],
            "choose",
        ),
        (&[Message::OpenCommandLine], "commands"),
        (&[Message::Edit], "edit"),
        (&[Message::ToggleMark], "mark"),
        (&[Message::ClearMarks], "clear marks"),
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
                .filter(|&&message| applicable(state, message))
                .flat_map(|&message| {
                    // while text is typed, the global keys are not what the line is about
                    if context.takes_text() {
                        keymap.context_keys_for(context, message)
                    } else {
                        keymap.keys_for(context, message)
                    }
                })
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
pub fn message_for(ui: &Ui, context: Context, event: &Event) -> Option<Message> {
    let key = Key::from_event(event)?;
    // where text is typed, a plain character is text before the global layer is asked: `q` must
    // not quit there. The keys of the context itself (Esc, Enter, ...) are asked first.
    if context.takes_text()
        && let Code::Char(c) = key.code
        && !key.ctrl
        && !key.alt
    {
        return ui
            .keymap
            .context_message(context, key)
            .or(Some(Message::Text(TextEdit::Insert(c))));
    }
    ui.keymap.message(context, key)
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
    screen_sink: &mut dyn FnMut(&str),
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
        // a tool for the developer, not part of the program: the screen as text goes to the sink
        if Key::from_event(&event) == Some(Key::new(Code::F(12))) {
            let size = terminal.size().map_err(LoopError::Draw)?;
            screen_sink(&screen_text(ui, &state, size.width, size.height));
            continue;
        }
        if let Some(message) = message_for(ui, Context::of(&state), &event) {
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
    // to try by hand how the failure cases of the editor look, without a setup for them
    let edit: fn(&mut ratatui::DefaultTerminal, &Path) -> EditorOutcome =
        if std::env::var_os("PROPMAN_FAKE_EDITOR").is_some() {
            fake_editor::fake_editor
        } else {
            edit_in_terminal
        };
    let result = run_loop(
        &mut terminal,
        ui,
        state,
        ratatui::crossterm::event::read,
        edit,
        // F12 writes the screen as text to a file; failing to write it is of no consequence
        &mut |text| {
            let _ = std::fs::write("propman-screen.txt", text);
        },
    );
    ratatui::restore();
    result.map_err(|error| match error {
        LoopError::Draw(e) | LoopError::Events(e) => e,
    })
}
