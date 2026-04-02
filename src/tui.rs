use crate::{
    messages::Message,
    render_model::DisplayRow,
    state::{AppState, Mode},
    update::update,
};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::{Block, Borders, Paragraph}};
use crate::keybindings::Keybindings;

pub fn run(mut state: AppState, keybindings: Keybindings) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| draw(f, &state))?;

        if let Event::Key(key) = event::read()? {
            // Ignore key-release and key-repeat events; process press only.
            // On Windows crossterm fires both Press and Release, which would
            // otherwise cause every keystroke to be handled twice.
            if key.kind != KeyEventKind::Press {
                continue;
            }
            state = handle_key(state, key, &keybindings);

            if state.quitting {
                break;
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// On Windows, AltGr is emitted as Ctrl+Alt. Normalize those combinations back
/// to a plain Char event so keybindings match and TextArea receives a character
/// rather than a control shortcut.
fn normalize_altgr(key: KeyEvent) -> KeyEvent {
    use crossterm::event::KeyModifiers;
    if key.modifiers == (KeyModifiers::CONTROL | KeyModifiers::ALT) {
        if let crossterm::event::KeyCode::Char(_) = key.code {
            return KeyEvent::new(key.code, KeyModifiers::NONE);
        }
    }
    key
}

fn handle_key(state: AppState, key: KeyEvent, keybindings: &Keybindings) -> AppState {
    let key = normalize_altgr(key);

    let mode_map = match state.mode {
        Mode::Normal       => &keybindings.normal,
        Mode::Editing      => &keybindings.editing,
        Mode::Continuation => &keybindings.continuation,
        Mode::Filter       => &keybindings.filter,
    };

    if let Some(msg) = mode_map.get(&key).cloned() {
        return update(state, msg);
    }

    // Keys not in the active map fall through to the TextArea (text modes) or
    // are silently ignored (Normal mode).
    match state.mode {
        Mode::Editing | Mode::Continuation => update(state, Message::TextInput(key)),
        Mode::Filter                       => update(state, Message::FilterInput(key)),
        Mode::Normal                       => state,
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, state: &AppState) {
    let area = f.area();

    if matches!(state.mode, Mode::Editing | Mode::Continuation) {
        // Pane grows with content: 2 border lines + one per TextArea line, capped at 8.
        let content_lines = state.edit_buffer.as_ref()
            .map(|e| e.textarea.lines().len())
            .unwrap_or(1);
        let pane_height = (content_lines + 2).min(8) as u16;

        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(pane_height),
            Constraint::Length(1), // filter bar
            Constraint::Length(1), // status bar
        ])
        .split(area);
        draw_table(f, chunks[0], state);
        draw_edit_pane(f, chunks[1], state);
        draw_filter_bar(f, chunks[2], state);
        draw_status(f, chunks[3], state);
    } else {
        let chunks = Layout::vertical([
            Constraint::Fill(1),   // main table
            Constraint::Length(1), // filter bar
            Constraint::Length(1), // status bar
        ])
        .split(area);
        draw_table(f, chunks[0], state);
        draw_filter_bar(f, chunks[1], state);
        draw_status(f, chunks[2], state);
    }
}

fn draw_edit_pane(f: &mut Frame, area: Rect, state: &AppState) {
    let Some(edit) = &state.edit_buffer else { return };

    let full_key = match state.display_rows.get(state.cursor_row) {
        Some(DisplayRow::Key { full_key, .. }) => full_key.as_str(),
        _ => "",
    };
    let locale = state.visible_locales
        .get(state.cursor_col.saturating_sub(1))
        .map(|s| s.as_str())
        .unwrap_or("");

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {full_key} [{locale}] "));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(&edit.textarea, inner);
}

fn draw_filter_bar(f: &mut Frame, area: Rect, state: &AppState) {
    let query = &state.filter_textarea.lines()[0];

    let (text, style) = match state.mode {
        // Focused: show cursor at the correct position within the query.
        Mode::Filter => {
            let col = state.filter_textarea.cursor().1;
            let byte_pos = query.char_indices().nth(col).map(|(i, _)| i).unwrap_or(query.len());
            let (before, after) = query.split_at(byte_pos);
            (
                format!("/ {before}_{after}"),
                Style::default().bg(Color::Yellow),
            )
        }
        // Active filter, unfocused: always show the applied query.
        _ if !query.is_empty() => (format!("/ {query}"), Style::default()),
        // No filter: dim placeholder.
        _ => (String::from("/ "), Style::default().fg(Color::Blue)),
    };
    f.render_widget(Paragraph::new(text).style(style), area);
}

fn draw_table(f: &mut Frame, area: Rect, state: &AppState) {
    if state.workspace.groups.is_empty() {
        f.render_widget(
            Paragraph::new("No .properties files found in the given directory."),
            area,
        );
        return;
    }

    let locales = &state.visible_locales;
    let viewport = area.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(viewport);

    for (row_idx, display_row) in state
        .display_rows
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(viewport)
    {
        let is_selected_row = row_idx == state.cursor_row;

        // Resolve key text, full key for lookups, and indentation.
        let (indent, key_text, full_key, is_header) = match display_row {
            DisplayRow::Header { prefix } => ("", format!("{prefix}:"), prefix.as_str(), true),
            DisplayRow::Key { display, full_key } => {
                let indent = if display.starts_with('.') { "  " } else { "" };
                (indent, format!("{display}: "), full_key.as_str(), false)
            }
        };

        // Key / prefix column (cursor_col == 0).
        let key_col_style = if is_selected_row && state.cursor_col == 0 {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else if is_header {
            Style::default().fg(Color::DarkGray)
        } else if is_selected_row {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let mut spans = vec![Span::raw(indent), Span::styled(key_text, key_col_style)];

        // Locale columns (cursor_col == locale_idx + 1).
        for (col_idx, locale) in locales.iter().enumerate() {
            let locale_col = col_idx + 1;
            let is_cursor_cell = is_selected_row && state.cursor_col == locale_col;

            // Headers have no stored values — their cells are empty/creatable.
            let value = if is_header {
                None
            } else {
                state
                    .workspace
                    .groups
                    .iter()
                    .flat_map(|g| g.files.iter())
                    .find(|f| &f.locale == locale)
                    .and_then(|f| f.get(full_key))
            };

            let tag_style = if is_selected_row {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(format!("[{locale}] "), tag_style));

            let (text, style) = match (is_cursor_cell, &state.mode, &state.edit_buffer) {
                // Cell is being edited in the bottom pane — show the workspace value
                // reversed so the user can see which cell is active.
                (true, Mode::Editing, _) => (
                    value.unwrap_or("").to_string(),
                    Style::default().add_modifier(Modifier::REVERSED),
                ),
                (true, _, _) => (
                    value.unwrap_or("").to_string(),
                    Style::default().add_modifier(Modifier::REVERSED),
                ),
                // Key row: value missing in this locale but present in others.
                (false, _, _) if value.is_none() && !is_header => {
                    ("<missing>".to_string(), Style::default().fg(Color::Red))
                }
                _ => (value.unwrap_or("").to_string(), Style::default()),
            };
            spans.push(Span::styled(format!("{text}  "), style));
        }

        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn draw_status(f: &mut Frame, area: Rect, state: &AppState) {
    let mode_label = match state.mode {
        Mode::Normal       => "NORMAL",
        Mode::Editing      => "EDIT  ",
        Mode::Continuation => "CONT  ",
        Mode::Filter       => "FILTER",
    };
    let dirty = if state.unsaved_changes { " [+]" } else { "    " };
    let hints = match state.mode {
        Mode::Editing      => "  Enter commit  Esc cancel  \\ continuation",
        Mode::Continuation => "  Enter new line  Esc cancel \\",
        _                  => "  q quit  ↑↓←→/hjkl navigate  Enter edit  / filter  Ctrl+S save",
    };
    let status = format!(" {mode_label}{dirty}{hints}");
    f.render_widget(
        Paragraph::new(status).style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
    );
}
