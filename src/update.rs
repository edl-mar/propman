use crate::{
    editor::CellEdit,
    messages::Message,
    ops,
    render_model::DisplayRow,
    state::{AppState, Mode, PendingChange, SelectionScope},
    workspace,
    writer,
};

/// Pure state transition: (AppState, Message) → AppState.
/// No I/O, no side effects.
///
/// `cursor_col` convention:
///   0        = key / prefix column selected
///   1..=n    = locale column n-1 selected
pub fn update(mut state: AppState, msg: Message) -> AppState {
    // Clear any one-shot status message on every new keypress.
    state.status_message = None;

    let mode = state.mode.clone();

    match (mode, msg) {
        // ── Normal mode ──────────────────────────────────────────────────────
        (Mode::Normal, Message::MoveCursorUp) => {
            state.cursor_row = state.cursor_row.saturating_sub(1);
            state.clamp_cursor_col();
            state.clamp_scroll();
        }
        (Mode::Normal, Message::MoveCursorDown) => {
            let max = state.display_rows.len().saturating_sub(1);
            if state.cursor_row < max {
                state.cursor_row += 1;
            }
            state.clamp_cursor_col();
            state.clamp_scroll();
        }
        (Mode::Normal, Message::MoveCursorLeft) => {
            // Step left, skipping locale columns that have no file in this row's bundle.
            let bundle = state.current_row_bundle().to_string();
            let mut col = state.cursor_col.saturating_sub(1);
            while col > 0 && !state.workspace.bundle_has_locale(&bundle, &state.visible_locales[col - 1]) {
                col -= 1;
            }
            state.cursor_col = col;
        }
        (Mode::Normal, Message::MoveCursorRight) => {
            // Step right, skipping locale columns that have no file in this row's bundle.
            let bundle = state.current_row_bundle().to_string();
            let max = state.visible_locales.len();
            let mut col = state.cursor_col + 1;
            while col <= max && !state.workspace.bundle_has_locale(&bundle, &state.visible_locales[col - 1]) {
                col += 1;
            }
            if col <= max {
                state.cursor_col = col;
            }
        }
        (Mode::Normal, Message::PageUp) => {
            state.cursor_row = state.cursor_row.saturating_sub(20);
            state.clamp_cursor_col();
            state.clamp_scroll();
        }
        (Mode::Normal, Message::PageDown) => {
            let max = state.display_rows.len().saturating_sub(1);
            state.cursor_row = (state.cursor_row + 20).min(max);
            state.clamp_cursor_col();
            state.clamp_scroll();
        }
        (_, Message::CycleScope) => {
            state.selection_scope = state.selection_scope.cycle();
        }
        (Mode::Normal, Message::StartEdit) => {
            if state.cursor_col == 0 {
                // Key column: open the rename editor pre-filled with the current key.
                let row = match state.display_rows.get(state.cursor_row) {
                    Some(r) => r,
                    None => return state,
                };
                // Block rename of bundle-level Header rows (renaming a bundle would
                // require renaming the file on disk, which we don't support yet).
                if let DisplayRow::Header { prefix, .. } = row {
                    if state.workspace.is_bundle_name(prefix) {
                        return state;
                    }
                }
                let old_key = match row {
                    DisplayRow::Key { full_key, .. } => full_key.clone(),
                    DisplayRow::Header { prefix, .. } => prefix.clone(),
                };
                // Header rows have no exact key — force Children scope for them.
                let is_header = matches!(
                    state.display_rows.get(state.cursor_row),
                    Some(DisplayRow::Header { .. })
                );
                if is_header {
                    state.selection_scope = SelectionScope::Children;
                }
                state.edit_buffer = Some(CellEdit::new(old_key));
                state.mode = Mode::KeyRenaming;
            } else {
                // Block value editing on bundle-level Header rows (they have no key).
                if let Some(DisplayRow::Header { prefix, .. }) = state.display_rows.get(state.cursor_row) {
                    if state.workspace.is_bundle_name(prefix) {
                        return state;
                    }
                }
                // Both Key and within-bundle Header rows open a value editor.
                let current_value = state.current_cell_value().unwrap_or_default();
                state.edit_buffer = Some(CellEdit::new(current_value));
                state.mode = Mode::Editing;
            }
        }
        (Mode::Normal, Message::DeleteKey) => {
            if state.cursor_col == 0 {
                // Key column: enter Deleting mode for confirmation (with optional Tab toggle).
                let row = match state.display_rows.get(state.cursor_row) {
                    Some(r) => r,
                    None => return state,
                };
                // Block bundle-level headers (the bundle name is not a key).
                if let DisplayRow::Header { prefix, .. } = row {
                    if state.workspace.is_bundle_name(prefix) {
                        return state;
                    }
                }
                let (key, is_header) = match row {
                    DisplayRow::Key { full_key, .. } => (full_key.clone(), false),
                    DisplayRow::Header { prefix, .. } => (prefix.clone(), true),
                };
                // Within-bundle Header rows have no exact key — force Children scope.
                if is_header {
                    state.selection_scope = SelectionScope::Children;
                }
                state.edit_buffer = Some(CellEdit::new(key));
                state.mode = Mode::Deleting;
            } else {
                // Locale cell: immediately delete just this one locale's entry.
                if let Some(DisplayRow::Key { full_key, .. }) = state.display_rows.get(state.cursor_row) {
                    let full_key = full_key.clone();
                    let locale_idx = state.cursor_col - 1;
                    if let Some(locale) = state.visible_locales.get(locale_idx).cloned() {
                        if state.workspace.get_value(&full_key, &locale).is_some() {
                            ops::delete::delete_locale_entry(&mut state, &full_key, &locale);
                            state.apply_filter();
                        }
                    }
                }
            }
        }
        (Mode::Normal, Message::TogglePreview) => {
            state.show_preview = !state.show_preview;
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
                    let new_value = edit.current_value();
                    // Dispatch: insert if the key is absent from the locale file,
                    // update if it already exists there.
                    if state.current_cell_value().is_none() {
                        ops::insert::commit_cell_insert(&mut state, new_value);
                    } else {
                        ops::insert::commit_cell_edit(&mut state, new_value);
                    }
                    // Rebuild display: dangling status may have changed (key is no
                    // longer dangling after its first translation), and locale-status
                    // filters (e.g. `:de?`, `*`) should re-evaluate immediately.
                    state.apply_filter();
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

        // ── New key (n) ──────────────────────────────────────────────────────
        (Mode::Normal, Message::NewKey) => {
            // Pre-fill the key-naming editor with the parent prefix of the
            // current row so the user types only the new suffix.
            let prefix = match state.display_rows.get(state.cursor_row) {
                Some(DisplayRow::Header { prefix, .. }) => {
                    if state.workspace.is_bundle_name(prefix) {
                        // Bundle header: pre-fill "bundle:" so the user types the real key.
                        format!("{prefix}:")
                    } else {
                        // Within-bundle header: "bundle:app.confirm" → "bundle:app.confirm."
                        format!("{prefix}.")
                    }
                }
                Some(DisplayRow::Key { full_key, .. }) => {
                    // "messages:app.error" → rfind('.') finds '.' in "app.error"
                    //   → "messages:app."  (correctly bundle-qualified)
                    // If no dot: "messages:app" → fall back to "messages:"
                    match full_key.rfind('.') {
                        Some(i) => format!("{}.", &full_key[..i]),
                        None => {
                            // No dot in real key — pre-fill with bundle prefix if present.
                            match full_key.find(':') {
                                Some(i) => format!("{}:", &full_key[..i]),
                                None    => String::new(),
                            }
                        }
                    }
                }
                _ => String::new(),
            };
            state.edit_buffer = Some(CellEdit::new(prefix));
            state.mode = Mode::KeyNaming;
        }

        // ── KeyNaming mode ───────────────────────────────────────────────────
        (Mode::KeyNaming, Message::CommitKeyName) => {
            let new_key = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            // Valid key: non-empty, not already known, and either:
            //   - contains a '.' (bare key: "app.title")
            //   - is bundle-qualified with a non-empty real key ("messages:app")
            let (_, real_part) = workspace::split_key(&new_key);
            let is_valid = !new_key.is_empty()
                && !state.workspace.merged_keys.contains(&new_key)
                && (new_key.contains('.') || (!real_part.is_empty() && new_key.contains(':')));

            if is_valid {
                state.edit_buffer = None;

                // Register in the workspace and rebuild the display.
                state.workspace.merged_keys.push(new_key.clone());
                state.workspace.merged_keys.sort();

                // If the immediate parent is a dangling placeholder (in merged_keys
                // but no file entry), it is now a pure namespace — drop it.
                // e.g. creating "app.confirm2.child" removes dangling "app.confirm2".
                let (bundle, real) = workspace::split_key(&new_key);
                if let Some(dot) = real.rfind('.') {
                    let parent_key = if bundle.is_empty() {
                        real[..dot].to_string()
                    } else {
                        format!("{bundle}:{}", &real[..dot])
                    };
                    if state.workspace.merged_keys.contains(&parent_key)
                        && state.workspace.is_dangling(&parent_key)
                    {
                        state.workspace.merged_keys.retain(|k| k != &parent_key);
                    }
                }

                state.apply_filter();

                // Navigate to the new row if it is visible under the current filter.
                if let Some(row_idx) = state.display_rows.iter().position(|r| {
                    matches!(r, DisplayRow::Key { full_key, .. } if *full_key == new_key)
                }) {
                    state.cursor_row = row_idx;
                    // Place cursor on the first locale column (default) so the user
                    // can immediately hit Enter to start adding a translation.
                    state.cursor_col = state.visible_locales.len().min(1);
                    state.clamp_scroll();
                }
                state.mode = Mode::Normal;
            }
            // else: invalid key — stay in KeyNaming so the user can correct it.
        }
        (Mode::KeyNaming, Message::CancelEdit) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
        }
        (Mode::KeyNaming, Message::TextInput(key)) => {
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.input(tui_textarea::Input::from(key));
            }
        }

        // ── KeyRenaming mode ─────────────────────────────────────────────────
        (Mode::KeyRenaming, Message::CommitKeyRename) => {
            let new_key = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            let old_key = match state.display_rows.get(state.cursor_row) {
                Some(DisplayRow::Key { full_key, .. }) => full_key.clone(),
                Some(DisplayRow::Header { prefix, .. }) => prefix.clone(),
                _ => { state.edit_buffer = None; state.mode = Mode::Normal; return state; }
            };

            // Validate: non-empty, has a dot or colon, not the same as before.
            if new_key.is_empty() || (!new_key.contains('.') && !new_key.contains(':')) {
                state.status_message = Some("Key must contain at least one '.'".to_string());
                // Stay in KeyRenaming so the user can fix it.
            } else if new_key != old_key {
                if state.selection_scope == SelectionScope::Children {
                    ops::rename::commit_prefix_rename(&mut state, &old_key, new_key);
                } else {
                    ops::rename::commit_exact_rename(&mut state, &old_key, new_key);
                }
            } else {
                // No change — just close.
                state.edit_buffer = None;
                state.mode = Mode::Normal;
            }
        }
        (Mode::KeyRenaming, Message::CancelEdit) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
        }
        (Mode::KeyRenaming, Message::TextInput(key)) => {
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.input(tui_textarea::Input::from(key));
            }
        }

        // ── Deleting mode ────────────────────────────────────────────────────
        (Mode::Deleting, Message::CommitDelete) => {
            let key = state.edit_buffer.as_ref()
                .map(|e| e.current_value())
                .unwrap_or_default();

            if state.selection_scope == SelectionScope::Children {
                ops::delete::delete_key_prefix(&mut state, &key);
            } else {
                ops::delete::delete_key(&mut state, &key);
            }

            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
        }
        (Mode::Deleting, Message::CancelEdit) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
        }

        // Up/Down in any edit mode: cancel and immediately move (mirrors Filter).
        (
            Mode::Editing | Mode::KeyNaming | Mode::KeyRenaming | Mode::Deleting,
            Message::MoveCursorUp,
        ) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.cursor_row = state.cursor_row.saturating_sub(1);
            state.clamp_cursor_col();
            state.clamp_scroll();
        }
        (
            Mode::Editing | Mode::KeyNaming | Mode::KeyRenaming | Mode::Deleting,
            Message::MoveCursorDown,
        ) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            let max = state.display_rows.len().saturating_sub(1);
            if state.cursor_row < max {
                state.cursor_row += 1;
            }
            state.clamp_cursor_col();
            state.clamp_scroll();
        }

        // ── Filter mode ──────────────────────────────────────────────────────
        (Mode::Filter, Message::FilterInput(key)) => {
            state.filter_textarea.input(tui_textarea::Input::from(key));
            state.apply_filter();
        }
        (Mode::Filter, Message::ClearFilter) => {
            state.filter_textarea = tui_textarea::TextArea::default();
            state.apply_filter();
        }
        (Mode::Filter, Message::CancelEdit) => {
            // Escape cycles Filter → Normal (keeps query).
            state.mode = Mode::Normal;
        }
        (Mode::Filter, Message::CommitEdit) => {
            state.mode = Mode::Normal;
        }
        // Up/Down exit filter mode and immediately move the cursor.
        (Mode::Filter, Message::MoveCursorUp) => {
            state.mode = Mode::Normal;
            state.cursor_row = state.cursor_row.saturating_sub(1);
            state.clamp_cursor_col();
            state.clamp_scroll();
        }
        (Mode::Filter, Message::MoveCursorDown) => {
            state.mode = Mode::Normal;
            let max = state.display_rows.len().saturating_sub(1);
            if state.cursor_row < max {
                state.cursor_row += 1;
            }
            state.clamp_cursor_col();
            state.clamp_scroll();
        }

        // Escape in Normal cycles to Filter.
        (Mode::Normal, Message::CancelEdit) => {
            state.mode = Mode::Filter;
        }

        // ── Universal ────────────────────────────────────────────────────────
        (_, Message::SaveFile) => {
            // Flush pending writes. Any that fail are put back so the user can retry.
            // NOTE: this is the one place in update() that performs I/O.
            let changes = std::mem::take(&mut state.pending_writes);
            for change in changes {
                let result = match &change {
                    PendingChange::Update { path, first_line, last_line, key, value } =>
                        writer::write_change(path, *first_line, *last_line, key, value),
                    PendingChange::Insert { path, after_line, key, value } =>
                        writer::write_insert(path, *after_line, key, value),
                    PendingChange::Delete { path, first_line, last_line } =>
                        writer::write_delete(path, *first_line, *last_line),
                };
                if result.is_err() {
                    state.pending_writes.push(change);
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

