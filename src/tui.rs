use crate::{
    widgets::PropertiesWidget,
    messages::Message,
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
        let mut vp_height = 0usize;
        terminal.draw(|f| draw(f, &state, &mut vp_height))?;
        state.vp_height = vp_height;
        // Re-clamp scroll after every render so that layout changes (preview pane,
        // edit pane, terminal resize) never leave scroll_offset stale.
        state.clamp_scroll();

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
        Mode::KeyNaming     => &keybindings.key_naming,
        Mode::BundleNaming  => &keybindings.bundle_naming,
        Mode::LocaleNaming  => &keybindings.locale_naming,
        Mode::KeyRenaming   => &keybindings.key_renaming,
        Mode::Deleting     => &keybindings.deleting,
        Mode::Filter       => &keybindings.filter,
        Mode::Pasting      => &keybindings.pasting,
    };

    if let Some(msg) = mode_map.get(&key).cloned() {
        return update(state, msg);
    }

    // Keys not in the active map fall through to the TextArea (text modes) or
    // are silently ignored (Normal mode).
    match state.mode {
        Mode::Editing | Mode::Continuation => update(state, Message::TextInput(key)),
        Mode::KeyNaming | Mode::BundleNaming | Mode::LocaleNaming | Mode::KeyRenaming => update(state, Message::TextInput(key)),
        Mode::Filter                        => update(state, Message::FilterInput(key)),
        // Deleting and Pasting panes are read-only — unbound keys are silently ignored.
        Mode::Deleting | Mode::Pasting | Mode::Normal => state,
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, state: &AppState, vp_height: &mut usize) {
    let area = f.area();

    if matches!(state.mode, Mode::Editing | Mode::Continuation | Mode::KeyNaming | Mode::BundleNaming | Mode::LocaleNaming | Mode::KeyRenaming | Mode::Deleting) {
        // Edit/confirm pane: height grows with content, capped at 8 lines.
        let content_lines = state.edit_buffer.as_ref()
            .map(|e| e.textarea.lines().len())
            .unwrap_or(1);
        let pane_height = (content_lines + 2).min(8) as u16;

        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(pane_height),
            Constraint::Length(3), // filter pane
            Constraint::Length(1), // status bar
        ])
        .split(area);
        *vp_height = chunks[0].height as usize;
        f.render_widget(PropertiesWidget::new(state), chunks[0]);
        draw_edit_pane(f, chunks[1], state);
        draw_filter_bar(f, chunks[2], state);
        draw_status(f, chunks[3], state);
    } else if matches!(state.mode, Mode::Pasting) {
        let pane_height = paste_pane_height(state);
        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(pane_height),
            Constraint::Length(3), // filter pane
            Constraint::Length(1), // status bar
        ])
        .split(area);
        *vp_height = chunks[0].height as usize;
        f.render_widget(PropertiesWidget::new(state), chunks[0]);
        draw_paste_pane(f, chunks[1], state);
        draw_filter_bar(f, chunks[2], state);
        draw_status(f, chunks[3], state);
    } else if state.show_preview {
        // Preview pane: read-only, same slot as the edit pane.
        let pane_height = preview_pane_height(state);
        let chunks = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(pane_height),
            Constraint::Length(3), // filter pane
            Constraint::Length(1), // status bar
        ])
        .split(area);
        *vp_height = chunks[0].height as usize;
        f.render_widget(PropertiesWidget::new(state), chunks[0]);
        draw_preview_pane(f, chunks[1], state);
        draw_filter_bar(f, chunks[2], state);
        draw_status(f, chunks[3], state);
    } else {
        let chunks = Layout::vertical([
            Constraint::Fill(1),   // main table
            Constraint::Length(3), // filter pane
            Constraint::Length(1), // status bar
        ])
        .split(area);
        *vp_height = chunks[0].height as usize;
        f.render_widget(PropertiesWidget::new(state), chunks[0]);
        draw_filter_bar(f, chunks[1], state);
        draw_status(f, chunks[2], state);
    }
}

fn draw_edit_pane(f: &mut Frame, area: Rect, state: &AppState) {
    let Some(edit) = &state.edit_buffer else { return };

    let full_key_owned = state.cursor_key_for_ops().unwrap_or_default();
    let full_key = full_key_owned.as_str();
    let locale = state.effective_locale_idx()
        .and_then(|i| state.visible_locales.get(i))
        .map(|s| s.as_str())
        .unwrap_or("");

    let title = if matches!(state.mode, Mode::KeyNaming) {
        format!(" new key [{locale}] ")
    } else if matches!(state.mode, Mode::BundleNaming) {
        " new bundle ".to_string()
    } else if matches!(state.mode, Mode::LocaleNaming) {
        format!(" new locale for [{full_key}] ")
    } else if matches!(state.mode, Mode::KeyRenaming) {
        let scope = state.selection_scope.label();
        format!(" rename · {full_key} [{scope}] · Enter=move  Ctrl+Enter=copy  Tab=scope ")
    } else if matches!(state.mode, Mode::Deleting) {
        let scope = state.selection_scope.label();
        format!(" delete · {full_key} [{scope}] ")
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
    let full_key_owned = state.cursor_key_for_ops()?;
    let full_key = full_key_owned.as_str();

    if state.cursor_locale.is_none() {
        // Key column: show the full bundle-qualified key path.
        Some((format!(" {full_key} "), full_key.to_string()))
    } else {
        // Locale column: show the full value for this (key, locale) pair.
        let locale_idx = state.effective_locale_idx()?;
        let locale = state.visible_locales.get(locale_idx)?.clone();
        let content = match state.current_cell_value() {
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

fn paste_pane_height(state: &AppState) -> u16 {
    let max_entries = state.paste.history.values().map(|v| v.len()).max().unwrap_or(1);
    // header row + history entries + top/bottom borders
    (max_entries + 3).min(12) as u16
}

fn draw_paste_pane(f: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(" paste ", Style::default().fg(Color::Yellow)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let locale_keys = state.paste_locales();

    if locale_keys.is_empty() {
        f.render_widget(
            Paragraph::new("(empty)").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let col_constraints: Vec<Constraint> = locale_keys.iter().map(|_| Constraint::Fill(1)).collect();
    let col_areas = Layout::horizontal(col_constraints).split(inner);

    for (i, locale) in locale_keys.iter().enumerate() {
        let is_focused = i == state.paste.locale_cursor;
        let history = state.paste.history.get(locale).map(|v| v.as_slice()).unwrap_or(&[]);
        let selected_pos = *state.paste.history_pos.get(locale).unwrap_or(&0);

        let header_style = if is_focused {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(format!("[{locale}]"), header_style)),
        ];

        for (j, entry) in history.iter().enumerate() {
            let is_selected = j == selected_pos;
            let marker = if is_selected { "> " } else { "  " };
            let display: String = entry.replace("\\\n", "").replace('\n', " ");
            // Truncate to fit the column width (rough estimate of 30 chars).
            let truncated = if display.chars().count() > 30 {
                format!("{}…", display.chars().take(29).collect::<String>())
            } else {
                display
            };
            let style = if is_focused && is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(format!("{marker}{truncated}"), style)));
        }

        f.render_widget(Paragraph::new(lines), col_areas[i]);
    }
}

fn draw_filter_bar(f: &mut Frame, area: Rect, state: &AppState) {
    let query = &state.filter_textarea.lines()[0];
    let focused = matches!(state.mode, Mode::Filter);

    // The DSL hint lives in the title so it remains visible while the user types.
    let hint = Span::styled(
        " bundle  /key[?#]  :locale[?!#]  =value  #  ,=OR ",
        Style::default().fg(Color::DarkGray),
    );
    let block = if focused {
        let label = Span::styled(" Filter ", Style::default().fg(Color::Yellow));
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Line::from(vec![label, hint]))
    } else if !query.is_empty() {
        let label = Span::raw(" Filter ");
        Block::default()
            .borders(Borders::ALL)
            .title(Line::from(vec![label, hint]))
    } else {
        let label = Span::styled(" Filter ", Style::default().fg(Color::DarkGray));
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![label, hint]))
    };

    let inner = block.inner(area);
    f.render_widget(block, area);

    if focused {
        let col = state.filter_textarea.cursor().1;
        let byte_pos = query.char_indices().nth(col).map(|(i, _)| i).unwrap_or(query.len());
        let (before, after) = query.split_at(byte_pos);
        f.render_widget(Paragraph::new(format!("{before}_{after}")), inner);
    } else if !query.is_empty() {
        f.render_widget(Paragraph::new(query.as_str()), inner);
    } else {
        f.render_widget(
            Paragraph::new("press / to filter")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
    }
}
fn draw_status(f: &mut Frame, area: Rect, state: &AppState) {
    let mode_label = match state.mode {
        Mode::Normal       => "NORMAL",
        Mode::Editing      => "EDIT  ",
        Mode::Continuation => "CONT  ",
        Mode::KeyNaming    => "NEWKEY",
        Mode::BundleNaming => "BUNDLE",
        Mode::LocaleNaming => "LOCALE",
        Mode::KeyRenaming  => "RENAME",
        Mode::Deleting     => "DELETE",
        Mode::Filter       => "FILTER",
        Mode::Pasting      => "PASTE ",
    };
    let dirty = if state.domain_model.has_changes() { " [+]" } else { "    " };
    // Status message (e.g. rename conflict) overrides the normal hints for one keypress.
    let hints: std::borrow::Cow<str> = if let Some(msg) = &state.status_message {
        format!("  {msg}").into()
    } else {
        match state.mode {
            Mode::Editing      => "  Enter commit  Esc cancel  \\ continuation".into(),
            Mode::Continuation => "  Enter new line  Esc cancel \\".into(),
            Mode::KeyNaming    => "  Enter confirm key name  Esc cancel".into(),
            Mode::BundleNaming => "  Enter create bundle  Esc cancel".into(),
            Mode::LocaleNaming => "  Enter create locale file  Esc cancel".into(),
            Mode::KeyRenaming  => "  Enter move  Ctrl+Enter copy  Tab scope  Esc cancel".into(),
            Mode::Deleting     => "  Enter confirm  Esc cancel".into(),
            Mode::Filter       => "  Enter/Esc/↑↓ exit filter".into(),
            Mode::Pasting      => "  Ctrl+←→ locale  Ctrl+↑↓ history  d remove  p paste last  Ctrl+P paste (stay)  Enter paste all  Ctrl+Enter paste all (stay)  Ctrl+Y yank→locale  Esc cancel".into(),
            Mode::Normal => {
                let scope = state.selection_scope.label();
                format!("  [{scope}] Tab  ↑↓←→/hjkl navigate  Enter edit/rename  n new  d delete  m pin  y yank  p paste  Ctrl+P quick-paste  Space preview  / filter  Ctrl+S save  q quit").into()
            }
        }
    };
    let status = format!(" {mode_label}{dirty}{hints}");
    f.render_widget(
        Paragraph::new(status).style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
    );
}
