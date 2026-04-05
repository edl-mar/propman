use crate::{
    parser::FileEntry,
    render_model::DisplayRow,
    state::{AppState, PendingChange},
    workspace,
};

/// Inserts `real_key = value` into a specific locale file in-memory and queues
/// a `PendingChange::Insert`.  Adjusts line numbers of all subsequent entries.
/// `real_key` must be the bare key (no bundle prefix).
pub fn insert_into_file(state: &mut AppState, gi: usize, fi: usize, real_key: &str, value: &str) {
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

    let bundle = &state.workspace.groups[gi].base_name;
    let full_key = if bundle.is_empty() {
        real_key.to_string()
    } else {
        format!("{bundle}:{real_key}")
    };
    state.dirty_keys.insert(full_key.clone());
    state.pending_writes.push(PendingChange::Insert {
        path,
        after_line,
        key:   real_key.to_string(),
        value: value.to_string(),
        full_key,
    });
    state.unsaved_changes = true;
}

/// Apply `value` to `(full_key, locale)` without going through the cursor.
/// Updates an existing entry if the key is present in the locale file;
/// inserts a new one otherwise (locale file must exist in the bundle).
/// No-ops silently if the bundle has no file for `locale`.
pub fn apply_cell_value(state: &mut AppState, full_key: &str, locale: &str, value: String) {
    let (bundle, real_key) = workspace::split_key(full_key);
    let real_key = real_key.to_string();

    // Locate the entry (if it exists) and the file index.
    let mut existing: Option<(usize, usize, usize)> = None; // (gi, fi, ei)
    let mut file_idx: Option<(usize, usize)> = None;         // (gi, fi)

    for (gi, group) in state.workspace.groups.iter().enumerate() {
        if !bundle.is_empty() && group.base_name != bundle {
            continue;
        }
        for (fi, file) in group.files.iter().enumerate() {
            if file.locale != locale {
                continue;
            }
            file_idx = Some((gi, fi));
            for (ei, entry) in file.entries.iter().enumerate() {
                if let FileEntry::KeyValue { key, .. } = entry {
                    if *key == real_key {
                        existing = Some((gi, fi, ei));
                    }
                }
            }
        }
    }

    match existing {
        Some((gi, fi, ei)) => {
            // Update the existing entry in-memory and queue a write.
            let path = state.workspace.groups[gi].files[fi].path.clone();
            let (first_line, last_line) = match &state.workspace.groups[gi].files[fi].entries[ei] {
                FileEntry::KeyValue { first_line, last_line, .. } => (*first_line, *last_line),
                _ => return,
            };
            if let FileEntry::KeyValue { value: v, .. } =
                &mut state.workspace.groups[gi].files[fi].entries[ei]
            {
                *v = value.clone();
            }
            state.dirty_keys.insert(full_key.to_string());
            state.pending_writes.push(crate::state::PendingChange::Update {
                path,
                first_line,
                last_line,
                key: real_key,
                value,
                full_key: full_key.to_string(),
            });
            state.unsaved_changes = true;
        }
        None => {
            // Insert into the locale file if one exists.
            if let Some((gi, fi)) = file_idx {
                insert_into_file(state, gi, fi, &real_key, &value);
            }
            // If no locale file exists for this bundle, silently skip.
        }
    }
}

/// Applies a committed edit to the in-memory workspace and records a pending
/// disk write.
///
/// Only updates existing keys. Editing a `<missing>` cell (key not in the
/// locale file) is a no-op — handled by `commit_cell_insert` instead.
pub fn commit_cell_edit(state: &mut AppState, new_value: String) {
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

    state.dirty_keys.insert(full_key.clone());
    state.pending_writes.push(PendingChange::Update {
        path,
        first_line,
        last_line,
        key: real_key, // write the bare key — files never store bundle prefix
        value: new_value,
        full_key,
    });
    state.unsaved_changes = true;
}

/// Inserts a new key-value entry into the appropriate locale file and queues
/// a disk write. Called when the user commits an edit on a `<missing>` cell.
pub fn commit_cell_insert(state: &mut AppState, new_value: String) {
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

    // Last resort: first file that serves this locale (bare/non-bundle keys only).
    // For bundle-qualified keys the target bundle has no file for this locale;
    // inserting into a different bundle's file would be silently wrong — the
    // value would never be found by get_value() which searches the key's bundle.
    if target.is_none() && bundle.is_empty() {
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
        None => {
            // No file for this locale.  For bundle-qualified keys this means the
            // bundle has no [locale] file — inform the user instead of silently
            // no-op'ing (or worse, inserting into a different bundle's file).
            if !bundle.is_empty() {
                state.status_message = Some(
                    format!("No [{locale}] file in bundle '{bundle}' — create it first")
                );
            }
            return;
        }
    };

    insert_into_file(state, gi, fi, &real_key, &new_value);
}
