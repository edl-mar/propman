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
use ratatui::{prelude::*, widgets::{Block, Borders, Paragraph, Wrap}};
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
        Mode::KeyNaming    => &keybindings.key_naming,
        Mode::KeyRenaming  => &keybindings.key_renaming,
        Mode::Deleting     => &keybindings.deleting,
        Mode::Filter       => &keybindings.filter,
    };

    if let Some(msg) = mode_map.get(&key).cloned() {
        return update(state, msg);
    }

    // Keys not in the active map fall through to the TextArea (text modes) or
    // are silently ignored (Normal mode).
    match state.mode {
        Mode::Editing | Mode::Continuation => update(state, Message::TextInput(key)),
        Mode::KeyNaming | Mode::KeyRenaming => update(state, Message::TextInput(key)),
        Mode::Filter                        => update(state, Message::FilterInput(key)),
        // Deleting pane is read-only — unbound keys are silently ignored.
        Mode::Deleting | Mode::Normal       => state,
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, state: &AppState) {
    let area = f.area();

    if matches!(state.mode, Mode::Editing | Mode::Continuation | Mode::KeyNaming | Mode::KeyRenaming | Mode::Deleting) {
        // Edit/confirm pane: height grows with content, capped at 8 lines.
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
    } else if state.show_preview {
        // Preview pane: read-only, same slot as the edit pane.
        let pane_height = preview_pane_height(state);
        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(pane_height),
            Constraint::Length(1), // filter bar
            Constraint::Length(1), // status bar
        ])
        .split(area);
        draw_table(f, chunks[0], state);
        draw_preview_pane(f, chunks[1], state);
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
        Some(DisplayRow::Header { prefix, .. }) => prefix.as_str(),
        _ => "",
    };
    let locale = state.visible_locales
        .get(state.cursor_col.saturating_sub(1))
        .map(|s| s.as_str())
        .unwrap_or("");

    let title = if matches!(state.mode, Mode::KeyNaming) {
        format!(" new key [{locale}] ")
    } else if matches!(state.mode, Mode::KeyRenaming) {
        let is_header = matches!(
            state.display_rows.get(state.cursor_row),
            Some(DisplayRow::Header { .. })
        );
        if is_header {
            format!(" rename prefix · {full_key} ")
        } else {
            let prefix = format!("{full_key}.");
            let has_children = state.workspace.merged_keys.iter().any(|k| k.starts_with(&prefix));
            if has_children {
                let scope = if state.rename_children { "+children" } else { "exact" };
                format!(" rename · {full_key} [{scope}] Tab: toggle ")
            } else {
                format!(" rename · {full_key} ")
            }
        }
    } else if matches!(state.mode, Mode::Deleting) {
        let is_header = matches!(
            state.display_rows.get(state.cursor_row),
            Some(DisplayRow::Header { .. })
        );
        if is_header {
            format!(" delete prefix · {full_key} ")
        } else {
            let prefix = format!("{full_key}.");
            let has_children = state.workspace.merged_keys.iter().any(|k| k.starts_with(&prefix));
            if has_children {
                let scope = if state.delete_children { "+children" } else { "exact" };
                format!(" delete · {full_key} [{scope}] Tab: toggle ")
            } else {
                format!(" delete · {full_key} ")
            }
        }
    } else {
        format!(" {full_key} [{locale}] ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(&edit.textarea, inner);
}

/// Returns `(title, content)` for the preview pane at the current cursor position,
/// or `None` when there is nothing to preview (e.g. a header locale cell).
fn preview_content(state: &AppState) -> Option<(String, String)> {
    let full_key = match state.display_rows.get(state.cursor_row)? {
        DisplayRow::Key { full_key, .. } => full_key.as_str(),
        DisplayRow::Header { prefix, .. } => prefix.as_str(),
    };

    if state.cursor_col == 0 {
        // Key column: show the full bundle-qualified key path.
        Some((format!(" {full_key} "), full_key.to_string()))
    } else {
        // Locale column: show the full value for this (key, locale) pair.
        // Header locale cells have no value and produce no preview.
        if matches!(state.display_rows.get(state.cursor_row), Some(DisplayRow::Header { .. })) {
            return None;
        }
        let locale = state.visible_locales.get(state.cursor_col - 1)?;
        let value = state.workspace.get_value(full_key, locale);
        // Convert physical continuation markers (\+newline) to display newlines.
        let content = match value {
            Some(v) => v.replace("\\\n", "\n"),
            None    => "<missing>".to_string(),
        };
        Some((format!(" {full_key} [{locale}] "), content))
    }
}

fn preview_pane_height(state: &AppState) -> u16 {
    let content_lines = preview_content(state)
        .map(|(_, c)| c.lines().count().max(1))
        .unwrap_or(1);
    (content_lines + 2).min(8) as u16
}

fn draw_preview_pane(f: &mut Frame, area: Rect, state: &AppState) {
    let Some((title, content)) = preview_content(state) else { return };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(content).wrap(Wrap { trim: false }),
        inner,
    );
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
            DisplayRow::Header { display, prefix, depth } => {
                (
                    "  ".repeat(*depth),
                    format!("{display}:"),
                    prefix.as_str(),
                    true,
                )
            }
            DisplayRow::Key { display, full_key, depth } => {
                let indent = "  ".repeat(*depth);
                let dangling = if state.workspace.is_dangling(full_key) { "*" } else { "" };
                (indent, format!("{dangling}{display}: "), full_key.as_str(), false)
            }
        };

        // Bundle-level headers (depth 0, prefix == bundle name) never have
        // locale columns — the bundle name is not itself a translatable key.
        let is_bundle_header = is_header && state.workspace.is_bundle_name(full_key);

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

        if is_bundle_header {
            lines.push(Line::from(spans));
            continue;
        }

        // Locale columns (cursor_col == locale_idx + 1).
        for (col_idx, locale) in locales.iter().enumerate() {
            let locale_col = col_idx + 1;
            let is_cursor_cell = is_selected_row && state.cursor_col == locale_col;

            // Headers have no stored values — their cells are empty/creatable.
            // Bundle headers (prefix == bundle name) are also always empty.
            let value = if is_header {
                None
            } else {
                state.workspace.get_value(full_key, locale)
            };

            let tag_style = if is_selected_row {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(format!("[{locale}] "), tag_style));

            // Strip `\`+newline continuation markers — they're an on-disk format
            // detail. The logical value (without them) is what the cell should show.
            let display = value.map(|v| v.replace("\\\n", ""));
            let display_str = display.as_deref().unwrap_or("");

            let (text, style) = match (is_cursor_cell, &state.mode, &state.edit_buffer) {
                // Cell is being edited in the bottom pane — show the workspace value
                // reversed so the user can see which cell is active.
                (true, Mode::Editing, _) => (
                    display_str.to_string(),
                    Style::default().add_modifier(Modifier::REVERSED),
                ),
                (true, _, _) => (
                    display_str.to_string(),
                    Style::default().add_modifier(Modifier::REVERSED),
                ),
                // Key row: value missing in this locale but present in others.
                (false, _, _) if value.is_none() && !is_header => {
                    ("<missing>".to_string(), Style::default().fg(Color::Red))
                }
                _ => (display_str.to_string(), Style::default()),
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
        Mode::KeyNaming    => "NEWKEY",
        Mode::KeyRenaming  => "RENAME",
        Mode::Deleting     => "DELETE",
        Mode::Filter       => "FILTER",
    };
    let dirty = if state.unsaved_changes { " [+]" } else { "    " };
    // Status message (e.g. rename conflict) overrides the normal hints for one keypress.
    let hints: std::borrow::Cow<str> = if let Some(msg) = &state.status_message {
        format!("  {msg}").into()
    } else {
        match state.mode {
            Mode::Editing      => "  Enter commit  Esc cancel  \\ continuation".into(),
            Mode::Continuation => "  Enter new line  Esc cancel \\".into(),
            Mode::KeyNaming    => "  Enter confirm key name  Esc cancel".into(),
            Mode::KeyRenaming  => "  Enter confirm  Esc cancel".into(),
            Mode::Deleting     => "  Enter confirm  Esc cancel".into(),
            _                  => "  q quit  ↑↓←→/hjkl navigate  Enter edit/rename  n new  d delete  Space preview  / filter  Ctrl+S save".into(),
        }
    };
    let status = format!(" {mode_label}{dirty}{hints}");
    f.render_widget(
        Paragraph::new(status).style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
    );
}
