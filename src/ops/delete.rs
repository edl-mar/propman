use crate::{
    parser::FileEntry,
    state::{AppState, PendingChange},
    workspace,
};

/// Core deletion: removes `full_key` from every locale file in its bundle and
/// from `merged_keys`.  Sets `unsaved_changes` but does NOT set `status_message`
/// — callers are responsible for the message so batch operations can summarise.
///
/// Dangling keys are dropped from `merged_keys` only (no file writes needed).
pub fn delete_key_inner(state: &mut AppState, full_key: &str) {
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
            full_key: full_key.to_string(),
        });
    }

    state.workspace.merged_keys.retain(|k| k != full_key);
    // Key is gone — remove from dirty too (no point tracking a deleted key).
    state.dirty_keys.remove(full_key);
    if !found.is_empty() {
        state.unsaved_changes = true;
    }
}

/// Deletes one key from all locale files and sets the status message.
pub fn delete_key(state: &mut AppState, full_key: &str) {
    delete_key_inner(state, full_key);
    state.status_message = Some(format!("Deleted {full_key}"));
}

/// Deletes every key that equals `prefix` or starts with `prefix.` from all
/// locale files, then sets a summary status message.
///
/// `all_children`: when `true`, deletes hidden (filter-invisible) children too
/// (`ChildrenAll` scope).  When `false`, only deletes keys currently visible in
/// `display_rows` (`Children` scope — hidden ones are silently left untouched).
pub fn delete_key_prefix(state: &mut AppState, prefix: &str, all_children: bool) {
    let dot_prefix = format!("{prefix}.");

    let visible: std::collections::HashSet<String> = if !all_children {
        state.render_model.bundles.iter()
            .flat_map(|b| b.entries.iter().map(move |e| {
                let key = e.segments.join(".");
                if b.name.is_empty() { key } else { format!("{}:{}", b.name, key) }
            }))
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    let keys: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| {
            (*k == prefix || k.starts_with(&dot_prefix))
                && (all_children || visible.contains(k.as_str()))
        })
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
pub fn delete_locale_entry(state: &mut AppState, full_key: &str, locale: &str) {
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

    state.dirty_keys.insert(full_key.to_string());
    state.pending_writes.push(PendingChange::Delete {
        path,
        first_line: fl,
        last_line: ll,
        full_key: full_key.to_string(),
    });
    state.unsaved_changes = true;
    state.status_message = Some(format!("Deleted [{locale}] entry for {full_key}"));
}
