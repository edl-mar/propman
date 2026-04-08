use crate::{
    filter::ColumnDirective,
    messages::Message,
    render_model::{
        Group,
        entry_visual_depth, group_effective_path, group_visual_depth,
        qualify, relative_display, trim_ctx_to_ancestor,
    },
    state::{AppState, Mode, SelectionScope},
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

    let full_key_owned = state.cursor.full_key().unwrap_or_default();
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
    let full_key_owned = state.cursor.full_key()?;
    let full_key = full_key_owned.as_str();

    if state.cursor.is_key_col() {
        // Key column: show the full bundle-qualified key path.
        Some((format!(" {full_key} "), full_key.to_string()))
    } else {
        // Locale column: show the full value for this (key, locale) pair.
        // Bundle header locale cells have no stored value.
        if state.cursor.segments.is_empty() {
            return None;
        }
        let locale_idx = state.effective_locale_idx()?;
        let locale = state.visible_locales.get(locale_idx)?;
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

    // ── Dirty-cell set: (full_key, locale) pairs with pending writes ──────────
    let path_to_locale: std::collections::HashMap<&std::path::Path, &str> = state
        .workspace.groups.iter()
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

    // ── Cursor / selection state ──────────────────────────────────────────────
    // scope_anchor = the cursor's bundle-qualified key (or bundle name for headers).
    let scope_anchor: Option<String> =
        if matches!(state.mode, Mode::Normal | Mode::KeyRenaming | Mode::Deleting) {
            if state.cursor.segments.is_empty() {
                state.cursor.bundle.clone()
            } else {
                state.cursor.full_key()
            }
        } else {
            None
        };
    let scope_prefix: Option<String> =
        if matches!(state.selection_scope, SelectionScope::Children | SelectionScope::ChildrenAll) {
            scope_anchor.as_ref().map(|a| format!("{a}."))
        } else {
            None
        };
    // cursor_full_key_str is the same as scope_anchor in the new cursor model.
    let cursor_full_key_owned: Option<String> = scope_anchor.clone();
    let cursor_full_key_str: Option<&str> = cursor_full_key_owned.as_deref();

    // ── Iterate the hierarchical render model ─────────────────────────────────
    // `visual_row` counts every emitted row; used only for scroll offset comparison.
    let mut visual_row: usize = 0;

    'outer: for bundle in &state.render_model.bundles {
        let bundle_offset = if bundle.name.is_empty() { 0usize } else { 1 };
        // bundle_opt: None for the bundle-less group, Some(name) for named bundles.
        let bundle_opt: Option<&str> = if bundle.name.is_empty() { None } else { Some(bundle.name.as_str()) };
        let cursor_bundle = state.cursor.bundle.as_deref();

        // ── Bundle-level header ───────────────────────────────────────────────
        if !bundle.name.is_empty() {
            let is_selected = cursor_bundle == bundle_opt && state.cursor.segments.is_empty();
            if visual_row >= state.scroll_offset {
                let row_key = bundle.name.as_str();
                let in_scope = scope_prefix.as_ref().map_or(false, |p| row_key.starts_with(p.as_str()));
                let row_in_selection = is_selected || in_scope;
                let select_style = bundle_header_select_style(is_selected, state.cursor.is_key_col());
                let base_style   = Style::default().fg(Color::DarkGray);
                let indent = String::new();
                let mut spans = make_key_spans(
                    &indent, "", &bundle.name, row_key, true,
                    scope_anchor.as_deref(), row_in_selection, select_style, base_style,
                );
                // Locale tags: one per locale in this bundle.
                for locale in locales.iter() {
                    if !state.workspace.bundle_has_locale(&bundle.name, locale) { continue; }
                    let is_cursor_cell = is_selected
                        && state.cursor.locale.as_deref() == Some(locale.as_str());
                    let style = if is_cursor_cell {
                        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    spans.push(Span::styled(format!("[{locale}] "), style));
                }
                lines.push(Line::from(spans));
                if lines.len() >= viewport { break 'outer; }
            }
            visual_row += 1;
        }

        // ── Entries within this bundle ────────────────────────────────────────
        let mut ctx_stack: Vec<String> = Vec::new(); // context for display strings

        for (i, entry) in bundle.entries.iter().enumerate() {
            let rk = entry.real_key();
            let full_key_owned = qualify(&bundle.name, &rk);
            let full_key: &str = &full_key_owned;

            // ── Headers for branch groups starting at this entry ──────────────
            let mut header_groups: Vec<(&str, &Group)> = bundle.groups.iter()
                .filter(|(prefix, g)| {
                    g.first_entry == i
                        && g.is_branch
                        && prefix.split('.').count() == g.depth + 1
                        && prefix.as_str() != rk
                        && g.label != rk
                })
                .map(|(p, g)| (p.as_str(), g))
                .collect();
            header_groups.sort_by_key(|(_, g)| g.depth);

            for (prefix, group) in &header_groups {
                let ep = group_effective_path(prefix, &group.label);
                trim_ctx_to_ancestor(&mut ctx_stack, &ep);

                let header_segs: Vec<String> = ep.split('.').map(|s| s.to_string()).collect();
                let is_selected = cursor_bundle == bundle_opt && state.cursor.segments == header_segs;
                if visual_row >= state.scroll_offset {
                    let qualified_prefix = qualify(&bundle.name, &ep);
                    let row_key = qualified_prefix.as_str();
                    let display = relative_display(&ep, ctx_stack.last().map(|s| s.as_str()));
                    let depth   = group_visual_depth(prefix, &bundle.groups) + bundle_offset;
                    let indent  = "  ".repeat(depth);

                    let in_scope = scope_prefix.as_ref().map_or(false, |p| row_key.starts_with(p.as_str()));
                    let on_path  = on_path_check(is_selected, row_key, cursor_full_key_str, &scope_anchor);
                    let row_in_selection = is_selected || on_path || in_scope;

                    let select_style = non_bundle_select_style(is_selected, state.cursor.is_key_col());
                    let base_style   = Style::default().fg(Color::DarkGray);

                    let mut spans = make_key_spans(
                        &indent, "", &display, row_key, true,
                        scope_anchor.as_deref(), row_in_selection, select_style, base_style,
                    );
                    // Within-bundle headers: render locale cells (they have no value but
                    // can be committed-to via Header insert).
                    render_locale_cells(
                        &mut spans, locales, row_key, true, false,
                        is_selected, in_scope, false, false,
                        state.cursor.locale.as_deref(), &state.mode, &state.column_directive,
                        &dirty_cells, &bundle.name, &bundle.locales,
                        None, // no Entry (header row)
                    );
                    lines.push(Line::from(spans));
                    if lines.len() >= viewport { break 'outer; }
                }
                visual_row += 1;
                ctx_stack.push(ep);
            }

            // ── Entry (key) row ───────────────────────────────────────────────
            trim_ctx_to_ancestor(&mut ctx_stack, &rk);

            let is_selected = cursor_bundle == bundle_opt && state.cursor.segments == entry.segments;
            if visual_row >= state.scroll_offset {
                let display = relative_display(&rk, ctx_stack.last().map(|s| s.as_str()));
                let depth   = entry_visual_depth(&entry.segments, &bundle.groups) + bundle_offset;
                let indent  = "  ".repeat(depth);

                let dangling = if entry.cells.iter().all(|c| c.value.is_none()) && !entry.is_dirty {
                    if state.workspace.is_dangling(full_key) { "*" } else { "" }
                } else { "" };
                let dirty_flag  = if entry.is_dirty  { "#" } else { "" };
                let pinned_flag = if entry.is_pinned  { "@" } else { "" };
                let flags = format!("{dangling}{dirty_flag}{pinned_flag}");

                let in_scope = scope_prefix.as_ref().map_or(false, |p| full_key.starts_with(p.as_str()));
                let on_path  = on_path_check(is_selected, full_key, cursor_full_key_str, &scope_anchor);
                let row_in_selection = is_selected || on_path || in_scope;

                let select_style = non_bundle_select_style(is_selected, state.cursor.is_key_col());
                let base_style = if entry.is_temp_pinned {
                    Style::default().fg(Color::Green).add_modifier(Modifier::DIM)
                } else if entry.is_pinned || entry.is_dirty {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                };

                let mut spans = make_key_spans(
                    &indent, &flags, &display, full_key, false,
                    scope_anchor.as_deref(), row_in_selection, select_style, base_style,
                );
                render_locale_cells(
                    &mut spans, locales, full_key, false, entry.is_temp_pinned,
                    is_selected, in_scope, entry.is_pinned, entry.is_dirty,
                    state.cursor.locale.as_deref(), &state.mode, &state.column_directive,
                    &dirty_cells, &bundle.name, &bundle.locales,
                    Some(entry),
                );
                lines.push(Line::from(spans));
                if lines.len() >= viewport { break 'outer; }
            }
            visual_row += 1;

            // Push this entry as context if it has children.
            if bundle.entries.get(i + 1)
                .map_or(false, |next| next.real_key().starts_with(&format!("{rk}.")))
            {
                ctx_stack.push(rk);
            }
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

// ── draw_table helpers ────────────────────────────────────────────────────────

fn bundle_header_select_style(is_selected: bool, is_key_col: bool) -> Style {
    if is_selected && is_key_col {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else if is_selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    }
}

fn non_bundle_select_style(is_selected: bool, is_key_col: bool) -> Style {
    if is_selected && is_key_col {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else if is_selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    }
}

fn on_path_check(
    is_selected: bool,
    row_key: &str,
    cursor_full_key_str: Option<&str>,
    scope_anchor: &Option<String>,
) -> bool {
    if is_selected { return false; }
    let Some(ck) = cursor_full_key_str else { return false; };
    let Some(anchor) = scope_anchor.as_deref() else { return false; };
    ck.starts_with(&format!("{row_key}."))
        && (row_key == anchor || row_key.starts_with(&format!("{anchor}.")))
}

/// Appends locale cell spans to `spans` for a single row.
#[allow(clippy::too_many_arguments)]
fn render_locale_cells(
    spans: &mut Vec<Span<'static>>,
    locales: &[String],
    full_key: &str,
    is_header: bool,
    is_temp_pinned: bool,
    is_selected: bool,
    in_scope: bool,
    is_perm_pinned: bool,
    is_dirty_row: bool,
    cursor_locale: Option<&str>,
    mode: &Mode,
    column_directive: &ColumnDirective,
    dirty_cells: &std::collections::HashSet<(&str, &str)>,
    bundle_name: &str,
    bundle_locales: &[String],
    entry: Option<&crate::render_model::Entry>,
) {
    for locale in locales.iter() {
        // Skip locales not present in this bundle.
        if !bundle_locales.contains(locale) { continue; }

        let is_cursor_cell = is_selected && cursor_locale == Some(locale.as_str());

        // Look up the value: from Entry cells for key rows, None for headers.
        let value: Option<&str> = if is_header {
            None
        } else if let Some(e) = entry {
            let cell_idx = bundle_locales.iter().position(|l| l == locale);
            cell_idx.and_then(|idx| e.cells.get(idx)).and_then(|c| c.value.as_deref())
        } else {
            None
        };

        // Per-row column directives.
        if !is_header {
            match column_directive {
                ColumnDirective::MissingOnly if value.is_some() => continue,
                ColumnDirective::PresentOnly if value.is_none() => continue,
                _ => {}
            }
        }

        let is_dirty_cell = if let Some(e) = entry {
            let cell_idx = bundle_locales.iter().position(|l| l == locale);
            cell_idx.and_then(|idx| e.cells.get(idx)).map_or(false, |c| c.is_dirty)
        } else {
            dirty_cells.contains(&(full_key, locale.as_str()))
        };

        let tag_style = if is_selected {
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

        // Strip `\`+newline continuation markers for display.
        let owned;
        let display_str: &str = if let Some(v) = value {
            owned = v.replace("\\\n", "");
            &owned
        } else {
            ""
        };

        let (text, style) = if is_cursor_cell {
            (display_str.to_string(), Style::default().add_modifier(Modifier::REVERSED))
        } else if value.is_none() && !is_header {
            ("<missing>".to_string(), Style::default().fg(Color::Red))
        } else {
            (display_str.to_string(), Style::default())
        };
        let _ = mode; // mode was used in old code for Editing detection; no longer needed here
        spans.push(Span::styled(format!("{text}  "), style));
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
