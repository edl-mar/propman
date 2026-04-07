use crate::{
    filter::ColumnDirective,
    messages::Message,
    render_model::DisplayRow,
    state::{AppState, CursorSection, Mode, SelectionScope},
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

fn draw(f: &mut Frame, state: &AppState) {
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
        draw_table(f, chunks[0], state);
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
        draw_table(f, chunks[0], state);
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
        draw_table(f, chunks[0], state);
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
        .get(state.cursor_section.locale_idx().unwrap_or(0))
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
    let full_key = match state.display_rows.get(state.cursor_row)? {
        DisplayRow::Key { full_key, .. } => full_key.as_str(),
        DisplayRow::Header { prefix, .. } => prefix.as_str(),
    };

    if state.cursor_section.is_key() {
        // Key column: show the full bundle-qualified key path.
        Some((format!(" {full_key} "), full_key.to_string()))
    } else {
        // Locale column: show the full value for this (key, locale) pair.
        // Header locale cells have no value and produce no preview.
        if matches!(state.display_rows.get(state.cursor_row), Some(DisplayRow::Header { .. })) {
            return None;
        }
        let locale = state.visible_locales.get(state.cursor_section.locale_idx()?)?;
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

fn paste_pane_height(state: &AppState) -> u16 {
    let max_entries = state.clipboard.values().map(|v| v.len()).max().unwrap_or(1);
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
        let is_focused = i == state.paste_locale_cursor;
        let history = state.clipboard.get(locale).map(|v| v.as_slice()).unwrap_or(&[]);
        let selected_pos = *state.paste_history_pos.get(locale).unwrap_or(&0);

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

/// Build per-segment styled spans for the key column.
///
/// Each dot-segment of the display string is checked against `scope_anchor`:
/// a segment at absolute key K is highlighted (using `select_style`) when
/// K == anchor or K starts with `{anchor}.` (i.e. it is a descendant-or-equal).
/// All other segments use `base_style`.
///
/// When `scope_anchor` is None or the row is not in the current selection,
/// a single span is returned (fast path — avoids per-segment work).
fn make_key_spans(
    indent: &str,
    flags: &str,
    display: &str,
    full_key: &str,   // full_key for Key rows, prefix for Header rows
    is_header: bool,
    scope_anchor: Option<&str>,
    row_in_selection: bool,
    select_style: Style,
    base_style: Style,
) -> Vec<Span<'static>> {
    let trailer = if is_header { ":" } else { ": " };

    // Fast path — no per-segment work needed.
    let Some(anchor) = scope_anchor.filter(|_| row_in_selection) else {
        let style = if row_in_selection { select_style } else { base_style };
        return vec![
            Span::raw(indent.to_string()),
            Span::styled(format!("{flags}{display}{trailer}"), style),
        ];
    };

    // Per-segment path.
    let colon_pos = full_key.find(':');
    let real = colon_pos.map_or(full_key, |i| &full_key[i + 1..]);
    let bundle_prefix = colon_pos.map_or("", |i| &full_key[..=i]);
    let all: Vec<&str> = real.split('.').collect();
    let total = all.len();

    let has_leading_dot = display.starts_with('.');
    let clean = display.trim_start_matches('.');
    let names: Vec<&str> = clean.split('.').collect();
    let shown = names.len();
    let ctx = total - shown; // index in all[] of the first displayed segment

    let anchor_dot = format!("{anchor}.");

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw(indent.to_string()));

    if !flags.is_empty() {
        spans.push(Span::styled(flags.to_string(), base_style));
    }

    for (i, name) in names.iter().enumerate() {
        let abs_key = format!("{}{}", bundle_prefix, all[..=ctx + i].join("."));
        let highlighted = abs_key == anchor || abs_key.starts_with(&anchor_dot);
        let sep = if i == 0 && !has_leading_dot { "" } else { "." };
        let style = if highlighted { select_style } else { base_style };
        spans.push(Span::styled(format!("{sep}{name}"), style));
    }

    // Trailer gets select_style when the row's full key is at/below the anchor.
    let full_highlighted = full_key == anchor || full_key.starts_with(&anchor_dot);
    spans.push(Span::styled(
        trailer.to_string(),
        if full_highlighted { select_style } else { base_style },
    ));

    spans
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

    // Build a (full_key, locale) set of cells with pending writes for dirty cell indicators.
    let path_to_locale: std::collections::HashMap<&std::path::Path, &str> = state.workspace.groups.iter()
        .flat_map(|g| g.files.iter())
        .map(|f| (f.path.as_path(), f.locale.as_str()))
        .collect();
    let dirty_cells: std::collections::HashSet<(&str, &str)> = state.pending_writes.iter()
        .filter_map(|c| {
            use crate::state::PendingChange;
            let (path, full_key) = match c {
                PendingChange::Update { path, full_key, .. } => (path.as_path(), full_key.as_str()),
                PendingChange::Insert { path, full_key, .. } => (path.as_path(), full_key.as_str()),
                PendingChange::Delete { path, full_key, .. } => (path.as_path(), full_key.as_str()),
            };
            path_to_locale.get(path).map(|locale| (full_key, *locale))
        })
        .collect();

    // scope_anchor: the tree node the key-segment cursor points to.
    // Always computed in Normal/KeyRenaming/Deleting so that per-segment highlighting
    // is active even at k=0 (cursor rests on the deepest/last segment by default).
    let scope_anchor: Option<String> = if matches!(state.mode, Mode::Normal | Mode::KeyRenaming | Mode::Deleting) {
        match state.display_rows.get(state.cursor_row) {
            Some(DisplayRow::Key { .. } | DisplayRow::Header { .. }) => state.key_seg_anchor(),
            None => None,
        }
    } else {
        None
    };
    // scope_prefix ("{anchor}.") drives +children sibling/child highlighting.
    let scope_prefix: Option<String> = if matches!(state.selection_scope, SelectionScope::Children | SelectionScope::ChildrenAll) {
        scope_anchor.as_ref().map(|a| format!("{a}."))
    } else {
        None
    };
    // cursor_full_key_str is used in the on_path check inside the render loop.
    let cursor_full_key_str: Option<&str> = match state.display_rows.get(state.cursor_row) {
        Some(DisplayRow::Key    { full_key, .. }) => Some(full_key.as_str()),
        Some(DisplayRow::Header { prefix,   .. }) => Some(prefix.as_str()),
        _ => None,
    };

    for (row_idx, display_row) in state
        .display_rows
        .iter()
        .enumerate()
        .skip(state.scroll_offset)
        .take(viewport)
    {
        let is_selected_row = row_idx == state.cursor_row;

        let row_key = match display_row {
            DisplayRow::Key    { full_key, .. } => full_key.as_str(),
            DisplayRow::Header { prefix,   .. } => prefix.as_str(),
        };

        // in_scope: [+children] is active and this row is a descendant of the anchor.
        let in_scope = scope_prefix.as_ref().map_or(false, |p| row_key.starts_with(p.as_str()));

        // on_path: this row is an ancestor of the cursor that lies on the path between
        // the cursor and the anchor (inclusive).  Naturally false when k=0 because no
        // ancestor can equal-or-start-with the cursor's full key.
        let on_path = !is_selected_row
            && cursor_full_key_str.map_or(false, |ck| {
                let anchor = match scope_anchor.as_deref() { Some(a) => a, None => return false };
                // Row must be a proper ancestor of the cursor key.
                ck.starts_with(&format!("{row_key}."))
                // Row must also be at-or-below the anchor (within the selected subtree).
                && (row_key == anchor || row_key.starts_with(&format!("{anchor}.")))
            });

        // Resolve display string, flags, full key, and indentation.
        let (indent, flags, display_ref, full_key, is_header) = match display_row {
            DisplayRow::Header { display, prefix, depth } => {
                ("  ".repeat(*depth), String::new(), display.as_str(), prefix.as_str(), true)
            }
            DisplayRow::Key { display, full_key, depth } => {
                let dangling = if state.workspace.is_dangling(full_key) { "*" } else { "" };
                let dirty   = if state.dirty_keys.contains(full_key.as_str()) { "#" } else { "" };
                let pinned  = if state.pinned_keys.contains(full_key.as_str()) { "@" } else { "" };
                (
                    "  ".repeat(*depth),
                    format!("{dangling}{dirty}{pinned}"),
                    display.as_str(),
                    full_key.as_str(),
                    false,
                )
            }
        };

        // Pin/dirty flags used for cell styling below.
        let is_temp_pinned = state.temp_pins.iter().any(|k| k == full_key);
        let is_perm_pinned = !is_header && state.pinned_keys.contains(full_key);
        let is_dirty_row   = !is_header && state.dirty_keys.contains(full_key);

        // Bundle-level headers (depth 0, prefix == bundle name) never have
        // locale columns — the bundle name is not itself a translatable key.
        let is_bundle_header = is_header && state.workspace.is_bundle_name(full_key);

        // The "select" style for highlighted segments and the "base" style for
        // everything else.  select_style varies by role (cursor vs on_path/in_scope);
        // base_style encodes row type (header, dirty, pinned, …).
        let select_style = if is_selected_row && state.cursor_section.is_key() {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else if is_selected_row {
            // Cursor on a locale column — key text is bold but not reversed.
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            // on_path or in_scope: white-background highlight without BOLD so
            // the cursor row still stands out as the "active" entry.
            Style::default().add_modifier(Modifier::REVERSED)
        };

        let base_style = if is_header {
            Style::default().fg(Color::DarkGray)
        } else if is_temp_pinned {
            Style::default().fg(Color::Green).add_modifier(Modifier::DIM)
        } else if is_perm_pinned {
            Style::default().fg(Color::Yellow)
        } else if is_dirty_row {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let row_in_selection = is_selected_row || on_path || in_scope;

        let mut spans = make_key_spans(
            &indent,
            &flags,
            display_ref,
            full_key,
            is_header,
            scope_anchor.as_deref(),
            row_in_selection,
            select_style,
            base_style,
        );

        if is_bundle_header {
            // Render a [locale] tag for each locale that belongs to this bundle.
            // These cells are navigable (cursor can land on them via ←→).
            for (col_idx, locale) in locales.iter().enumerate() {
                if !state.workspace.bundle_has_locale(full_key, locale) {
                    continue;
                }
                let is_cursor_cell = is_selected_row
                    && state.cursor_section == CursorSection::Locale(col_idx);
                let style = if is_cursor_cell {
                    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                spans.push(Span::styled(format!("[{locale}] "), style));
            }
            lines.push(Line::from(spans));
            continue;
        }

        // The bundle this row belongs to (used to skip locales with no file).
        let (row_bundle, _) = crate::workspace::split_key(full_key);

        // Locale columns — each col_idx maps to CursorSection::Locale(col_idx).
        for (col_idx, locale) in locales.iter().enumerate() {
            // Skip locales that have no backing file in this row's bundle.
            // Those cells can never hold a value and are confusing to show.
            if !state.workspace.bundle_has_locale(row_bundle, locale) {
                continue;
            }

            let is_cursor_cell = is_selected_row
                && state.cursor_section == CursorSection::Locale(col_idx);

            // Headers have no stored values — their cells are empty/creatable.
            // Bundle headers (prefix == bundle name) are also always empty.
            let value = if is_header {
                None
            } else {
                state.workspace.get_value(full_key, locale)
            };

            // Per-row column directives (:? = only missing, :! = only present)
            if !is_header {
                match state.column_directive {
                    ColumnDirective::MissingOnly if value.is_some() => continue,
                    ColumnDirective::PresentOnly if value.is_none() => continue,
                    _ => {}
                }
            }

            let is_dirty_cell = dirty_cells.contains(&(full_key, locale.as_str()));
            let tag_style = if is_selected_row {
                Style::default()
            } else if is_temp_pinned {
                Style::default().fg(Color::Green).add_modifier(Modifier::DIM)
            } else if in_scope {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM)
            } else if is_perm_pinned {
                Style::default().fg(Color::Yellow)
            } else if is_dirty_cell {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let locale_tag = if is_dirty_cell { format!("#[{locale}] ") } else { format!("[{locale}] ") };
            spans.push(Span::styled(locale_tag, tag_style));

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
        Mode::BundleNaming => "BUNDLE",
        Mode::LocaleNaming => "LOCALE",
        Mode::KeyRenaming  => "RENAME",
        Mode::Deleting     => "DELETE",
        Mode::Filter       => "FILTER",
        Mode::Pasting      => "PASTE ",
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
