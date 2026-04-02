use crate::{
    editor::CellEdit,
    filter,
    messages::Message,
    parser::FileEntry,
    render_model::{self, DisplayRow},
    state::{AppState, Mode},
    writer,
};

/// Pure state transition: (AppState, Message) → AppState.
/// No I/O, no side effects.
///
/// `cursor_col` convention:
///   0        = key / prefix column selected
///   1..=n    = locale column n-1 selected
pub fn update(mut state: AppState, msg: Message) -> AppState {
    let mode = state.mode.clone();

    match (mode, msg) {
        // ── Normal mode ──────────────────────────────────────────────────────
        (Mode::Normal, Message::MoveCursorUp) => {
            state.cursor_row = state.cursor_row.saturating_sub(1);
            clamp_scroll(&mut state);
        }
        (Mode::Normal, Message::MoveCursorDown) => {
            let max = state.display_rows.len().saturating_sub(1);
            if state.cursor_row < max {
                state.cursor_row += 1;
            }
            clamp_scroll(&mut state);
        }
        (Mode::Normal, Message::MoveCursorLeft) => {
            state.cursor_col = state.cursor_col.saturating_sub(1);
        }
        (Mode::Normal, Message::MoveCursorRight) => {
            // col 0 = key column; cols 1..=n_locales = locale columns
            let max = state.visible_locales.len();
            if state.cursor_col < max {
                state.cursor_col += 1;
            }
        }
        (Mode::Normal, Message::PageUp) => {
            state.cursor_row = state.cursor_row.saturating_sub(20);
            clamp_scroll(&mut state);
        }
        (Mode::Normal, Message::PageDown) => {
            let max = state.display_rows.len().saturating_sub(1);
            state.cursor_row = (state.cursor_row + 20).min(max);
            clamp_scroll(&mut state);
        }
        (Mode::Normal, Message::StartEdit) => {
            if state.cursor_col == 0 {
                // Key / prefix column — no action yet.
            } else {
                let current_value = current_cell_value(&state).unwrap_or_default();
                state.edit_buffer = Some(CellEdit::new(current_value));
                state.mode = Mode::Editing;
            }
        }
        (Mode::Normal, Message::FocusFilter) => {
            state.mode = Mode::Filter;
        }

        // ── Editing mode ─────────────────────────────────────────────────────
        (Mode::Editing, Message::TextInput(key)) => {
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.input(tui_textarea::Input::from(key));
            }
        }
        (Mode::Editing, Message::CommitEdit) | (Mode::Editing, Message::StartEdit) => {
            if let Some(edit) = state.edit_buffer.take() {
                if edit.is_modified() {
                    commit_cell_edit(&mut state, edit.current_value());
                }
            }
            state.mode = Mode::Normal;
        }

        // ── Continuation sub-mode ────────────────────────────────────────────
        (Mode::Editing, Message::EnterContinuation) => {
            // Insert `\` into the TextArea (visible to the user) then await Enter.
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.insert_char('\\');
            }
            state.mode = Mode::Continuation;
        }
        (Mode::Continuation, Message::InsertNewline) => {
            // Keep the trailing `\` — it becomes the continuation marker in the
            // .properties file — and open a new line after it.
            // Do NOT move to End first: `EnterContinuation` leaves the cursor right
            // after the `\`, so insert_newline splits at that position, which lets
            // the user break a line in the middle by placing `\` mid-value.
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.insert_newline();
            }
            state.mode = Mode::Editing;
        }
        (Mode::Continuation, Message::CancelContinuation) => {
            // Leave the `\` in the TextArea as a literal character.
            state.mode = Mode::Editing;
        }
        (Mode::Continuation, Message::TextInput(key)) => {
            // Any other key: cancel continuation (\ stays), then process the key.
            state.mode = Mode::Editing;
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.input(tui_textarea::Input::from(key));
            }
        }
        (Mode::Editing, Message::CancelEdit) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
        }

        // ── Filter mode ──────────────────────────────────────────────────────
        (Mode::Filter, Message::FilterInput(key)) => {
            state.filter_textarea.input(tui_textarea::Input::from(key));
            apply_filter(&mut state);
        }
        (Mode::Filter, Message::ClearFilter) => {
            state.filter_textarea = tui_textarea::TextArea::default();
            apply_filter(&mut state);
        }
        (Mode::Filter, Message::CancelEdit) => {
            // Escape cycles Filter → Normal (keeps query).
            state.mode = Mode::Normal;
        }
        (Mode::Filter, Message::CommitEdit) => {
            state.mode = Mode::Normal;
        }

        // Escape in Normal cycles to Filter.
        (Mode::Normal, Message::CancelEdit) => {
            state.mode = Mode::Filter;
        }

        // ── Universal ────────────────────────────────────────────────────────
        (_, Message::SaveFile) => {
            // Flush pending writes. Any that fail are put back so the user can retry.
            // NOTE: this is the one place in update() that performs I/O.
            let writes = std::mem::take(&mut state.pending_writes);
            for (path, first_line, last_line, key, value) in writes {
                if writer::write_change(&path, first_line, last_line, &key, &value).is_err() {
                    state.pending_writes.push((path, first_line, last_line, key, value));
                }
            }
            state.unsaved_changes = !state.pending_writes.is_empty();
        }
        (_, Message::Quit) => {
            state.quitting = true;
        }

        _ => {}
    }

    state
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the full key and current value at the active locale cell, if any.
/// Returns `None` when `cursor_col == 0` (key column) or the row has no value there.
fn current_cell_value(state: &AppState) -> Option<String> {
    if state.cursor_col == 0 {
        return None;
    }
    let locale_idx = state.cursor_col - 1;

    let full_key = match state.display_rows.get(state.cursor_row)? {
        DisplayRow::Key { full_key, .. } => full_key.as_str(),
        // Header rows have no stored value; editing will create the key.
        DisplayRow::Header { prefix } => prefix.as_str(),
    };

    let locale = state.visible_locales.get(locale_idx)?;

    state.workspace.groups.iter()
        .flat_map(|g| g.files.iter())
        .filter(|f| &f.locale == locale)
        .find_map(|f| f.get(full_key))
        .map(|v| v.to_string())
}

/// Applies a committed edit to the in-memory workspace and records a pending
/// disk write.
///
/// Only updates existing keys. Editing a `<missing>` cell (key not in the
/// locale file) is a no-op — new-key creation is not yet implemented.
fn commit_cell_edit(state: &mut AppState, new_value: String) {
    if state.cursor_col == 0 {
        return;
    }
    let locale_idx = state.cursor_col - 1;
    let locale = match state.visible_locales.get(locale_idx) {
        Some(l) => l.clone(),
        None => return,
    };
    let full_key = match state.display_rows.get(state.cursor_row) {
        Some(DisplayRow::Key { full_key, .. }) => full_key.clone(),
        _ => return, // Header row — no-op.
    };

    // Pass 1 (immutable): locate the entry and collect what we need.
    let mut found: Option<(usize, usize, usize)> = None; // (group_idx, file_idx, entry_idx)
    'find: for (gi, group) in state.workspace.groups.iter().enumerate() {
        for (fi, file) in group.files.iter().enumerate() {
            if file.locale != locale {
                continue;
            }
            for (ei, entry) in file.entries.iter().enumerate() {
                if let FileEntry::KeyValue { key, .. } = entry {
                    if *key == full_key {
                        found = Some((gi, fi, ei));
                        break 'find;
                    }
                }
            }
        }
    }

    let (gi, fi, ei) = match found {
        Some(idx) => idx,
        None => return, // Key absent from locale file — skip.
    };

    // Collect path and line range before the mutable borrow.
    let path = state.workspace.groups[gi].files[fi].path.clone();
    let (first_line, last_line) = match &state.workspace.groups[gi].files[fi].entries[ei] {
        FileEntry::KeyValue { first_line, last_line, .. } => (*first_line, *last_line),
        _ => return,
    };

    // Pass 2 (mutable): update the value in-memory (physical format, with any
    // `\`+newline continuation markers intact so the editor can re-open correctly).
    // The display layer strips `\<newline>` before rendering cell text.
    if let FileEntry::KeyValue { value, .. } =
        &mut state.workspace.groups[gi].files[fi].entries[ei]
    {
        *value = new_value.clone();
    }

    state.pending_writes.push((path, first_line, last_line, full_key, new_value));
    state.unsaved_changes = true;
}

/// Re-evaluates the filter query, rebuilds `display_rows` and `visible_locales`,
/// then clamps the cursor to the new bounds.
fn apply_filter(state: &mut AppState) {
    let query = state.filter_textarea.lines()[0].clone();
    let (filtered, visible) = if query.trim().is_empty() {
        (
            state.workspace.merged_keys.clone(),
            state.workspace.all_locales(),
        )
    } else {
        let expr = filter::parse(&query);
        let filtered = state.workspace.merged_keys.iter()
            .filter(|key| filter::evaluate(&expr, key, &state.workspace))
            .cloned()
            .collect();
        let visible = filter::visible_locales(&expr, &state.workspace);
        // (expr borrows nothing from state so this is fine)
        (filtered, visible)
    };
    state.display_rows = render_model::build_display_rows(&filtered);
    state.visible_locales = visible;
    state.cursor_col = state.cursor_col.min(state.visible_locales.len());
    let max_row = state.display_rows.len().saturating_sub(1);
    state.cursor_row = state.cursor_row.min(max_row);
    clamp_scroll(state);
}

/// Keeps `scroll_offset` in sync so the cursor stays visible.
/// Uses a hardcoded viewport estimate; the renderer will clip naturally anyway.
fn clamp_scroll(state: &mut AppState) {
    const VIEWPORT: usize = 20;
    if state.cursor_row < state.scroll_offset {
        state.scroll_offset = state.cursor_row;
    } else if state.cursor_row >= state.scroll_offset + VIEWPORT {
        state.scroll_offset = state.cursor_row - VIEWPORT + 1;
    }
}
