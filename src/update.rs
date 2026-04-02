use crate::{
    editor::CellEdit,
    filter,
    messages::Message,
    parser::FileEntry,
    render_model::{self, DisplayRow},
    state::{AppState, Mode, PendingChange},
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
                // Header rows have no exact key entry — lock to prefix rename.
                let is_header = matches!(
                    state.display_rows.get(state.cursor_row),
                    Some(DisplayRow::Header { .. })
                );
                state.rename_children = is_header;
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
                let current_value = current_cell_value(&state).unwrap_or_default();
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
                // Within-bundle Header rows have no exact key — always +children.
                state.delete_children = is_header;
                state.edit_buffer = Some(CellEdit::new(key));
                state.mode = Mode::Deleting;
            } else {
                // Locale cell: immediately delete just this one locale's entry.
                if let Some(DisplayRow::Key { full_key, .. }) = state.display_rows.get(state.cursor_row) {
                    let full_key = full_key.clone();
                    let locale_idx = state.cursor_col - 1;
                    if let Some(locale) = state.visible_locales.get(locale_idx).cloned() {
                        if state.workspace.get_value(&full_key, &locale).is_some() {
                            delete_locale_entry(&mut state, &full_key, &locale);
                            apply_filter(&mut state);
                        }
                    }
                }
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
                    let new_value = edit.current_value();
                    // Dispatch: insert if the key is absent from the locale file,
                    // update if it already exists there.
                    if current_cell_value(&state).is_none() {
                        commit_cell_insert(&mut state, new_value);
                    } else {
                        commit_cell_edit(&mut state, new_value);
                    }
                    // Rebuild display: dangling status may have changed (key is no
                    // longer dangling after its first translation), and locale-status
                    // filters (e.g. `:de?`, `*`) should re-evaluate immediately.
                    apply_filter(&mut state);
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
                apply_filter(&mut state);

                // Navigate to the new row if it is visible under the current filter.
                if let Some(row_idx) = state.display_rows.iter().position(|r| {
                    matches!(r, DisplayRow::Key { full_key, .. } if *full_key == new_key)
                }) {
                    state.cursor_row = row_idx;
                    // Place cursor on the first locale column (default) so the user
                    // can immediately hit Enter to start adding a translation.
                    state.cursor_col = state.visible_locales.len().min(1);
                    clamp_scroll(&mut state);
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
        (Mode::KeyRenaming, Message::ToggleRenameScope) => {
            // Only meaningful for Key rows that have children; ignore for headers.
            let is_header = matches!(
                state.display_rows.get(state.cursor_row),
                Some(DisplayRow::Header { .. })
            );
            if !is_header {
                if let Some(DisplayRow::Key { full_key, .. }) = state.display_rows.get(state.cursor_row) {
                    let prefix = format!("{full_key}.");
                    let has_children = state.workspace.merged_keys.iter().any(|k| k.starts_with(&prefix));
                    if has_children {
                        state.rename_children = !state.rename_children;
                    }
                }
            }
        }
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
                if state.rename_children {
                    commit_prefix_rename(&mut state, &old_key, new_key);
                } else {
                    commit_exact_rename(&mut state, &old_key, new_key);
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
        (Mode::Deleting, Message::ToggleDeleteScope) => {
            // Only toggle for Key rows that have children; Header rows are always +children.
            let is_header = matches!(
                state.display_rows.get(state.cursor_row),
                Some(DisplayRow::Header { .. })
            );
            if !is_header {
                if let Some(DisplayRow::Key { full_key, .. }) = state.display_rows.get(state.cursor_row) {
                    let prefix = format!("{full_key}.");
                    if state.workspace.merged_keys.iter().any(|k| k.starts_with(&prefix)) {
                        state.delete_children = !state.delete_children;
                    }
                }
            }
        }
        (Mode::Deleting, Message::CommitDelete) => {
            let key = state.edit_buffer.as_ref()
                .map(|e| e.current_value())
                .unwrap_or_default();
            let delete_children = state.delete_children;

            if delete_children {
                delete_key_prefix(&mut state, &key);
            } else {
                delete_key(&mut state, &key);
            }

            state.edit_buffer = None;
            state.mode = Mode::Normal;
            apply_filter(&mut state);
        }
        (Mode::Deleting, Message::CancelEdit) => {
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
        DisplayRow::Header { prefix, .. } => prefix.as_str(),
    };

    let locale = state.visible_locales.get(locale_idx)?;

    state.workspace.get_value(full_key, locale).map(|v| v.to_string())
}

/// Applies a committed edit to the in-memory workspace and records a pending
/// disk write.
///
/// Only updates existing keys. Editing a `<missing>` cell (key not in the
/// locale file) is a no-op — handled by `commit_cell_insert` instead.
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

    // Split bundle qualifier: files store the real key without bundle prefix.
    let (bundle, real_key) = workspace::split_key(&full_key);
    let real_key = real_key.to_string();

    // Pass 1 (immutable): locate the entry and collect what we need.
    let mut found: Option<(usize, usize, usize)> = None; // (group_idx, file_idx, entry_idx)
    'find: for (gi, group) in state.workspace.groups.iter().enumerate() {
        if !bundle.is_empty() && group.base_name != bundle {
            continue;
        }
        for (fi, file) in group.files.iter().enumerate() {
            if file.locale != locale {
                continue;
            }
            for (ei, entry) in file.entries.iter().enumerate() {
                if let FileEntry::KeyValue { key, .. } = entry {
                    if *key == real_key {
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

    state.pending_writes.push(PendingChange::Update {
        path,
        first_line,
        last_line,
        key: real_key, // write the bare key — files never store bundle prefix
        value: new_value,
    });
    state.unsaved_changes = true;
}

/// Inserts a new key-value entry into the appropriate locale file and queues
/// a disk write. Called when the user commits an edit on a `<missing>` cell.
fn commit_cell_insert(state: &mut AppState, new_value: String) {
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
        // Header rows: insert a translation for the prefix key itself.
        Some(DisplayRow::Header { prefix, .. }) => prefix.clone(),
        _ => return,
    };

    // If this key isn't in merged_keys yet (e.g. translating a Header row whose
    // prefix was never a standalone key in any file), register it now so the
    // Key row appears immediately after apply_filter — without needing a restart.
    if !state.workspace.merged_keys.contains(&full_key) {
        state.workspace.merged_keys.push(full_key.clone());
        state.workspace.merged_keys.sort();
    }

    // Split bundle qualifier: files store the real key without bundle prefix.
    // When a bundle is encoded in full_key, we look in exactly that bundle's group.
    let (bundle, real_key) = workspace::split_key(&full_key);
    let real_key = real_key.to_string();

    // Find the group and file for this locale.  With bundle support the group is
    // unambiguous (bundle name == group.base_name).  For dangling keys we also
    // check sibling keys already in the group as a secondary signal.
    let mut target: Option<(usize, usize)> = None; // (group_idx, file_idx)

    'find: for (gi, group) in state.workspace.groups.iter().enumerate() {
        if !bundle.is_empty() && group.base_name != bundle {
            continue; // Different bundle — skip.
        }
        // Prefer a group where the key already exists in another locale.
        let key_in_group = group.files.iter().any(|f| f.get(&real_key).is_some());
        if !bundle.is_empty() || key_in_group {
            for (fi, file) in group.files.iter().enumerate() {
                if file.locale == locale {
                    target = Some((gi, fi));
                    break 'find;
                }
            }
            if !bundle.is_empty() {
                // The bundle was explicit; no need to continue searching other groups.
                break 'find;
            }
        }
    }

    // Fallback for bare (non-bundle) dangling keys: walk up the dot-boundary prefix
    // chain until a group is found that owns keys sharing the prefix.
    if target.is_none() && bundle.is_empty() {
        let mut end = real_key.len();
        'walk: while let Some(dot) = real_key[..end].rfind('.') {
            let prefix = &real_key[..dot];
            for (gi, group) in state.workspace.groups.iter().enumerate() {
                let has_prefix = group.files.iter().any(|f| {
                    f.entries.iter().any(|e| matches!(e,
                        FileEntry::KeyValue { key, .. } if key.starts_with(prefix)
                    ))
                });
                if has_prefix {
                    for (fi, file) in group.files.iter().enumerate() {
                        if file.locale == locale {
                            target = Some((gi, fi));
                            break 'walk;
                        }
                    }
                    break 'walk;
                }
            }
            end = dot;
        }
    }

    // Last resort: first file that serves this locale.
    if target.is_none() {
        'last: for (gi, group) in state.workspace.groups.iter().enumerate() {
            for (fi, file) in group.files.iter().enumerate() {
                if file.locale == locale {
                    target = Some((gi, fi));
                    break 'last;
                }
            }
        }
    }

    let (gi, fi) = match target {
        Some(t) => t,
        None => return, // No file for this locale — skip.
    };

    let after_line = state.workspace.groups[gi].files[fi]
        .insertion_point_for(&real_key);

    // How many physical lines does the new value occupy?
    let n_lines = new_value.split('\n').count();
    let new_first_line = after_line + 1;
    let new_last_line  = after_line + n_lines;

    // Bump line numbers of all entries that follow the insertion point.
    for entry in &mut state.workspace.groups[gi].files[fi].entries {
        match entry {
            FileEntry::KeyValue { first_line, last_line, .. } => {
                if *first_line > after_line {
                    *first_line += n_lines;
                    *last_line  += n_lines;
                }
            }
            FileEntry::Comment { line, .. } | FileEntry::Blank { line } => {
                if *line > after_line {
                    *line += n_lines;
                }
            }
        }
    }

    // Register the new entry in the in-memory workspace.
    // Files always store the bare real_key — never the bundle-qualified form.
    let path = state.workspace.groups[gi].files[fi].path.clone();
    state.workspace.groups[gi].files[fi].entries.push(FileEntry::KeyValue {
        first_line: new_first_line,
        last_line:  new_last_line,
        key:   real_key.clone(),
        value: new_value.clone(),
    });

    state.pending_writes.push(PendingChange::Insert {
        path,
        after_line,
        key:   real_key,
        value: new_value,
    });
    state.unsaved_changes = true;
}

/// Rename one exact key across all locale files that contain it.
/// Routes to `commit_cross_bundle_rename` when the bundle prefix changes.
/// Sets `state.status_message` and stays in KeyRenaming on conflict.
fn commit_exact_rename(state: &mut AppState, old_key: &str, new_key: String) {
    let (old_bundle, _) = workspace::split_key(old_key);
    let (new_bundle, new_real) = workspace::split_key(&new_key);
    if old_bundle != new_bundle {
        commit_cross_bundle_rename(state, old_key, new_key);
        return;
    }
    if new_real.is_empty() {
        state.status_message = Some("Key must have a name after ':'".to_string());
        return;
    }
    if state.workspace.merged_keys.contains(&new_key) {
        state.status_message = Some(format!("'{new_key}' already exists"));
        return;
    }
    rename_key_in_workspace(state, old_key, &new_key);
    if let Some(pos) = state.workspace.merged_keys.iter().position(|k| k == old_key) {
        state.workspace.merged_keys[pos] = new_key;
        state.workspace.merged_keys.sort();
    }
    state.edit_buffer = None;
    state.mode = Mode::Normal;
    apply_filter(state);
}

/// Rename every key that equals `old_prefix` or starts with `old_prefix.`
/// Routes to `commit_cross_bundle_prefix_rename` when the bundle prefix changes.
fn commit_prefix_rename(state: &mut AppState, old_prefix: &str, new_prefix: String) {
    let (old_bundle, _) = workspace::split_key(old_prefix);
    let (new_bundle, new_real) = workspace::split_key(&new_prefix);
    if old_bundle != new_bundle {
        commit_cross_bundle_prefix_rename(state, old_prefix, new_prefix);
        return;
    }
    if new_real.is_empty() {
        state.status_message = Some("Key must have a name after ':'".to_string());
        return;
    }
    let dot_prefix = format!("{old_prefix}.");
    let keys_to_rename: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| *k == old_prefix || k.starts_with(&dot_prefix))
        .cloned()
        .collect();

    // Conflict check: ensure no renamed key collides with an existing unrelated key.
    for k in &keys_to_rename {
        let new_k = format!("{}{}", new_prefix, &k[old_prefix.len()..]);
        if state.workspace.merged_keys.contains(&new_k) && !keys_to_rename.contains(&new_k) {
            state.status_message = Some(format!("'{new_k}' already exists"));
            return;
        }
    }

    for k in keys_to_rename.clone() {
        let new_k = format!("{}{}", new_prefix, &k[old_prefix.len()..]);
        rename_key_in_workspace(state, &k, &new_k);
    }

    let old_set: std::collections::HashSet<String> = keys_to_rename.iter().cloned().collect();
    state.workspace.merged_keys.retain(|k| !old_set.contains(k));
    for k in &keys_to_rename {
        state.workspace.merged_keys.push(format!("{}{}", new_prefix, &k[old_prefix.len()..]));
    }
    state.workspace.merged_keys.sort();

    state.edit_buffer = None;
    state.mode = Mode::Normal;
    apply_filter(state);
}

/// Inserts `real_key = value` into a specific locale file in-memory and queues
/// a `PendingChange::Insert`.  Adjusts line numbers of all subsequent entries.
/// `real_key` must be the bare key (no bundle prefix).
fn insert_into_file(state: &mut AppState, gi: usize, fi: usize, real_key: &str, value: &str) {
    let after_line = state.workspace.groups[gi].files[fi].insertion_point_for(real_key);
    let n_lines = value.split('\n').count();
    let new_first_line = after_line + 1;
    let new_last_line  = after_line + n_lines;

    for entry in &mut state.workspace.groups[gi].files[fi].entries {
        match entry {
            FileEntry::KeyValue { first_line, last_line, .. } => {
                if *first_line > after_line {
                    *first_line += n_lines;
                    *last_line  += n_lines;
                }
            }
            FileEntry::Comment { line, .. } | FileEntry::Blank { line } => {
                if *line > after_line {
                    *line += n_lines;
                }
            }
        }
    }

    let path = state.workspace.groups[gi].files[fi].path.clone();
    state.workspace.groups[gi].files[fi].entries.push(FileEntry::KeyValue {
        first_line: new_first_line,
        last_line:  new_last_line,
        key:   real_key.to_string(),
        value: value.to_string(),
    });

    state.pending_writes.push(PendingChange::Insert {
        path,
        after_line,
        key:   real_key.to_string(),
        value: value.to_string(),
    });
    state.unsaved_changes = true;
}

/// Move one key from its current bundle to a different bundle, preserving all
/// locale translations that have a matching locale file in the destination.
///
/// Sets `status_message` with a summary (and lists any locales that could not
/// be moved because the destination has no file for them).
fn commit_cross_bundle_rename(state: &mut AppState, old_key: &str, new_key: String) {
    let (old_bundle, old_real) = workspace::split_key(old_key);
    let (new_bundle, new_real) = workspace::split_key(&new_key);

    // Destination bundle must exist.
    if !state.workspace.is_bundle_name(new_bundle) {
        state.status_message = Some(format!("Bundle '{new_bundle}' does not exist"));
        return;
    }
    if new_real.is_empty() {
        state.status_message = Some("Key must have a name after ':'".to_string());
        return;
    }
    if state.workspace.merged_keys.contains(&new_key) {
        state.status_message = Some(format!("'{new_key}' already exists"));
        return;
    }

    // Snapshot all (locale, value) pairs from the source bundle before deleting.
    let collected: Vec<(String, String)> = state.workspace.groups.iter()
        .filter(|g| old_bundle.is_empty() || g.base_name == old_bundle)
        .flat_map(|g| g.files.iter())
        .filter_map(|f| f.get(old_real).map(|v| (f.locale.clone(), v.to_string())))
        .collect();

    // Remove from source.
    delete_key_inner(state, old_key);

    // Register the new bundle-qualified key.
    state.workspace.merged_keys.push(new_key.clone());
    state.workspace.merged_keys.sort();

    // Insert into destination for each locale; track any that have no target file.
    let mut missed: Vec<String> = Vec::new();
    for (locale, value) in &collected {
        let target = state.workspace.groups.iter().enumerate()
            .find(|(_, g)| g.base_name == new_bundle)
            .and_then(|(gi, g)| {
                g.files.iter().enumerate()
                    .find(|(_, f)| &f.locale == locale)
                    .map(|(fi, _)| (gi, fi))
            });
        match target {
            Some((gi, fi)) => insert_into_file(state, gi, fi, new_real, value),
            None => missed.push(locale.clone()),
        }
    }

    let base = format!("Moved {old_key} → {new_key}");
    state.status_message = Some(if missed.is_empty() {
        base
    } else {
        format!("{base}  (no file for: {})", missed.join(", "))
    });
    state.edit_buffer = None;
    state.mode = Mode::Normal;
    apply_filter(state);
}

/// Move every key that equals `old_prefix` or starts with `old_prefix.` from
/// its current bundle to a different bundle.
fn commit_cross_bundle_prefix_rename(state: &mut AppState, old_prefix: &str, new_prefix: String) {
    let (old_bundle, _) = workspace::split_key(old_prefix);
    let (new_bundle, new_real) = workspace::split_key(&new_prefix);

    if !state.workspace.is_bundle_name(new_bundle) {
        state.status_message = Some(format!("Bundle '{new_bundle}' does not exist"));
        return;
    }
    if new_real.is_empty() {
        state.status_message = Some("Key must have a name after ':'".to_string());
        return;
    }

    let dot_prefix = format!("{old_prefix}.");
    let keys_to_move: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| *k == old_prefix || k.starts_with(&dot_prefix))
        .cloned()
        .collect();

    // Conflict check: none of the new keys may already exist.
    for k in &keys_to_move {
        let new_k = format!("{new_prefix}{}", &k[old_prefix.len()..]);
        if state.workspace.merged_keys.contains(&new_k) {
            state.status_message = Some(format!("'{new_k}' already exists"));
            return;
        }
    }

    let mut all_missed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let count = keys_to_move.len();

    for old_k in &keys_to_move {
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        let (_, old_real) = workspace::split_key(old_k);
        let (_, new_real_part) = workspace::split_key(&new_k);
        let new_real_owned = new_real_part.to_string();

        // Snapshot translations before deletion.
        let collected: Vec<(String, String)> = state.workspace.groups.iter()
            .filter(|g| old_bundle.is_empty() || g.base_name == old_bundle)
            .flat_map(|g| g.files.iter())
            .filter_map(|f| f.get(old_real).map(|v| (f.locale.clone(), v.to_string())))
            .collect();

        delete_key_inner(state, old_k);

        state.workspace.merged_keys.push(new_k);

        for (locale, value) in &collected {
            let target = state.workspace.groups.iter().enumerate()
                .find(|(_, g)| g.base_name == new_bundle)
                .and_then(|(gi, g)| {
                    g.files.iter().enumerate()
                        .find(|(_, f)| &f.locale == locale)
                        .map(|(fi, _)| (gi, fi))
                });
            match target {
                Some((gi, fi)) => insert_into_file(state, gi, fi, &new_real_owned, value),
                None => { all_missed.insert(locale.clone()); }
            }
        }
    }

    state.workspace.merged_keys.sort();

    let base = format!("Moved {count} key(s): {old_prefix} → {new_prefix}");
    state.status_message = Some(if all_missed.is_empty() {
        base
    } else {
        let mut missed_vec: Vec<_> = all_missed.into_iter().collect();
        missed_vec.sort();
        format!("{base}  (no file for: {})", missed_vec.join(", "))
    });
    state.edit_buffer = None;
    state.mode = Mode::Normal;
    apply_filter(state);
}

/// Rewrite every `old_key` entry in every locale file to `new_key`, keeping
/// the value unchanged.  Updates the in-memory workspace and queues
/// `PendingChange::Update` entries for the next Ctrl+S flush.
///
/// Both keys must be in the same bundle (cross-bundle renames are blocked before
/// reaching here).  Files store the bare real key — the bundle prefix is stripped
/// before any file-level operation.
fn rename_key_in_workspace(state: &mut AppState, old_key: &str, new_key: &str) {
    let (old_bundle, old_real) = workspace::split_key(old_key);
    let (_new_bundle, new_real) = workspace::split_key(new_key);

    // Pass 1 (immutable): collect all matching entries in the right bundle.
    let mut found: Vec<(std::path::PathBuf, usize, usize, String)> = Vec::new();
    for group in &state.workspace.groups {
        if !old_bundle.is_empty() && group.base_name != old_bundle {
            continue;
        }
        for file in &group.files {
            for entry in &file.entries {
                if let FileEntry::KeyValue { key, value, first_line, last_line } = entry {
                    if key == old_real {
                        found.push((file.path.clone(), *first_line, *last_line, value.clone()));
                    }
                }
            }
        }
    }

    // Pass 2 (mutable): update key names in-memory.
    for group in &mut state.workspace.groups {
        if !old_bundle.is_empty() && group.base_name != old_bundle {
            continue;
        }
        for file in &mut group.files {
            for entry in &mut file.entries {
                if let FileEntry::KeyValue { key, .. } = entry {
                    if key == old_real {
                        *key = new_real.to_string();
                    }
                }
            }
        }
    }

    // Queue a pending write for each affected file entry.
    for (path, first_line, last_line, value) in found {
        state.pending_writes.push(PendingChange::Update {
            path,
            first_line,
            last_line,
            key: new_real.to_string(), // always write the bare key to the file
            value,
        });
        state.unsaved_changes = true;
    }
}

/// Core deletion: removes `full_key` from every locale file in its bundle and
/// from `merged_keys`.  Sets `unsaved_changes` but does NOT set `status_message`
/// — callers are responsible for the message so batch operations can summarise.
///
/// Dangling keys are dropped from `merged_keys` only (no file writes needed).
fn delete_key_inner(state: &mut AppState, full_key: &str) {
    let (bundle, real_key) = workspace::split_key(full_key);

    if state.workspace.is_dangling(full_key) {
        state.workspace.merged_keys.retain(|k| k != full_key);
        return;
    }

    // Pass 1 (immutable): collect every locale file entry that matches.
    let mut found: Vec<(usize, usize, usize, usize, std::path::PathBuf)> =
        Vec::new(); // (gi, fi, first_line, last_line, path)
    for (gi, group) in state.workspace.groups.iter().enumerate() {
        if !bundle.is_empty() && group.base_name != bundle {
            continue;
        }
        for (fi, file) in group.files.iter().enumerate() {
            for entry in &file.entries {
                if let FileEntry::KeyValue { key, first_line, last_line, .. } = entry {
                    if key == real_key {
                        found.push((gi, fi, *first_line, *last_line, file.path.clone()));
                    }
                }
            }
        }
    }

    // Pass 2 (mutable): remove entry and shift line numbers in each locale file.
    for (gi, fi, fl, ll, path) in &found {
        let n_lines = ll - fl + 1;

        state.workspace.groups[*gi].files[*fi].entries.retain(|e| {
            !matches!(e, FileEntry::KeyValue { key, .. } if key == real_key)
        });

        for entry in &mut state.workspace.groups[*gi].files[*fi].entries {
            match entry {
                FileEntry::KeyValue { first_line, last_line, .. } => {
                    if *first_line > *ll {
                        *first_line -= n_lines;
                        *last_line  -= n_lines;
                    }
                }
                FileEntry::Comment { line, .. } | FileEntry::Blank { line } => {
                    if *line > *ll {
                        *line -= n_lines;
                    }
                }
            }
        }

        state.pending_writes.push(PendingChange::Delete {
            path: path.clone(),
            first_line: *fl,
            last_line:  *ll,
        });
    }

    state.workspace.merged_keys.retain(|k| k != full_key);
    if !found.is_empty() {
        state.unsaved_changes = true;
    }
}

/// Deletes one key from all locale files and sets the status message.
fn delete_key(state: &mut AppState, full_key: &str) {
    delete_key_inner(state, full_key);
    state.status_message = Some(format!("Deleted {full_key}"));
}

/// Deletes every key that equals `prefix` or starts with `prefix.` from all
/// locale files, then sets a summary status message.
fn delete_key_prefix(state: &mut AppState, prefix: &str) {
    let dot_prefix = format!("{prefix}.");
    let keys: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| *k == prefix || k.starts_with(&dot_prefix))
        .cloned()
        .collect();

    let count = keys.len();
    for key in &keys {
        delete_key_inner(state, key);
    }
    state.status_message = Some(format!("Deleted {count} key(s) under {prefix}"));
}

/// Deletes `full_key`'s entry from a single locale file, leaving all other
/// locales untouched.  The key stays in `merged_keys` — its cells for other
/// locales continue to show values; the deleted locale shows `<missing>`.
fn delete_locale_entry(state: &mut AppState, full_key: &str, locale: &str) {
    let (bundle, real_key) = workspace::split_key(full_key);

    // Find the specific locale file entry.
    let mut found: Option<(usize, usize, usize, usize, std::path::PathBuf)> = None;
    'find: for (gi, group) in state.workspace.groups.iter().enumerate() {
        if !bundle.is_empty() && group.base_name != bundle {
            continue;
        }
        for (fi, file) in group.files.iter().enumerate() {
            if file.locale != locale {
                continue;
            }
            for entry in &file.entries {
                if let FileEntry::KeyValue { key, first_line, last_line, .. } = entry {
                    if key == real_key {
                        found = Some((gi, fi, *first_line, *last_line, file.path.clone()));
                        break 'find;
                    }
                }
            }
        }
    }

    let (gi, fi, fl, ll, path) = match found {
        Some(f) => f,
        None => return,
    };

    let n_lines = ll - fl + 1;

    state.workspace.groups[gi].files[fi].entries.retain(|e| {
        !matches!(e, FileEntry::KeyValue { key, .. } if key == real_key)
    });

    for entry in &mut state.workspace.groups[gi].files[fi].entries {
        match entry {
            FileEntry::KeyValue { first_line, last_line, .. } => {
                if *first_line > ll {
                    *first_line -= n_lines;
                    *last_line  -= n_lines;
                }
            }
            FileEntry::Comment { line, .. } | FileEntry::Blank { line } => {
                if *line > ll {
                    *line -= n_lines;
                }
            }
        }
    }

    state.pending_writes.push(PendingChange::Delete { path, first_line: fl, last_line: ll });
    state.unsaved_changes = true;
    state.status_message = Some(format!("Deleted [{locale}] entry for {full_key}"));
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
