use crate::{
    ops,
    parser::FileEntry,
    state::{AppState, Mode, PendingChange},
    workspace,
};


/// Rename one exact key across all locale files that contain it.
/// Routes to `commit_cross_bundle_rename` when the bundle prefix changes.
/// Sets `state.status_message` and stays in KeyRenaming on conflict.
pub fn commit_exact_rename(state: &mut AppState, old_key: &str, new_key: String) {
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
        // old_key was a real standalone key — replace it in-place.
        state.workspace.merged_keys[pos] = new_key;
    } else {
        // old_key was a pure namespace Header (not a real key in any file).
        // Treat the typed name as a fresh dangling entry instead of renaming.
        state.workspace.merged_keys.push(new_key);
    }
    state.workspace.merged_keys.sort();
    state.edit_buffer = None;
    state.mode = Mode::Normal;
    state.apply_filter();
}

/// Rename every key that equals `old_prefix` or starts with `old_prefix.`
/// Routes to `commit_cross_bundle_prefix_rename` when the bundle prefix changes.
///
/// `all_children`: when `true`, renames hidden children too (`ChildrenAll` scope).
/// When `false`, only renames keys currently visible in `view_rows` (`Children`
/// scope — hidden ones are left untouched).
pub fn commit_prefix_rename(state: &mut AppState, old_prefix: &str, new_prefix: String, all_children: bool) {
    let (old_bundle, _) = workspace::split_key(old_prefix);
    let (new_bundle, new_real) = workspace::split_key(&new_prefix);
    if old_bundle != new_bundle {
        commit_cross_bundle_prefix_rename(state, old_prefix, new_prefix, all_children);
        return;
    }
    if new_real.is_empty() {
        state.status_message = Some("Key must have a name after ':'".to_string());
        return;
    }
    let dot_prefix = format!("{old_prefix}.");

    let visible = if !all_children { state.visible_full_keys() } else { std::collections::HashSet::new() };

    let keys_to_rename: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| {
            (*k == old_prefix || k.starts_with(&dot_prefix))
                && (all_children || visible.contains(k.as_str()))
        })
        .cloned()
        .collect();

    // Conflict check: block if any destination key already exists.
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
    state.apply_filter();
}

/// Snapshot all translations for `old_real` in `old_bundle` and insert them as
/// `new_real` into `dest_bundle`. Returns the list of locales that had no
/// destination file and therefore could not be inserted.
///
/// This is the shared core of cross-bundle move and key copy: both snapshot the
/// source values and insert them at the destination. The only difference between
/// move and copy is whether the caller also deletes the source afterward.
fn snapshot_and_insert(
    state: &mut AppState,
    old_bundle: &str,
    old_real: &str,
    dest_bundle: &str,
    new_real: &str,
) -> Vec<String> {
    let collected: Vec<(String, String)> = state.workspace.groups.iter()
        .filter(|g| old_bundle.is_empty() || g.base_name == old_bundle)
        .flat_map(|g| g.files.iter())
        .filter_map(|f| f.get(old_real).map(|v| (f.locale.clone(), v.to_string())))
        .collect();

    let dest_full_key = if dest_bundle.is_empty() {
        new_real.to_string()
    } else {
        format!("{dest_bundle}:{new_real}")
    };

    let mut missed: Vec<String> = Vec::new();
    for (locale, value) in collected {
        if state.workspace.bundle_has_locale(dest_bundle, &locale) {
            // apply_cell_value handles update-or-insert: if (dest_key, locale)
            // already has a value it is overwritten; otherwise a new entry is inserted.
            ops::insert::apply_cell_value(state, &dest_full_key, &locale, value);
        } else {
            missed.push(locale);
        }
    }
    missed
}

/// Move one key from its current bundle to a different bundle, preserving all
/// locale translations that have a matching locale file in the destination.
fn commit_cross_bundle_rename(state: &mut AppState, old_key: &str, new_key: String) {
    let (old_bundle, old_real) = workspace::split_key(old_key);
    let (new_bundle, new_real) = workspace::split_key(&new_key);

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

    // Snapshot + insert first (source and dest are different files — order is safe).
    let missed = snapshot_and_insert(state, old_bundle, old_real, new_bundle, new_real);

    // Remove source and register the new key.
    ops::delete::delete_key_inner(state, old_key);
    state.workspace.merged_keys.push(new_key.clone());
    state.workspace.merged_keys.sort();

    let base = format!("Moved {old_key} → {new_key}");
    state.status_message = Some(if missed.is_empty() {
        base
    } else {
        format!("{base}  (no file for: {})", missed.join(", "))
    });
    state.edit_buffer = None;
    state.mode = Mode::Normal;
    state.apply_filter();
}

/// Move every key that equals `old_prefix` or starts with `old_prefix.` from
/// its current bundle to a different bundle.
fn commit_cross_bundle_prefix_rename(state: &mut AppState, old_prefix: &str, new_prefix: String, all_children: bool) {
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

    let visible = if !all_children { state.visible_full_keys() } else { std::collections::HashSet::new() };

    let keys_to_move: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| {
            (*k == old_prefix || k.starts_with(&dot_prefix))
                && (all_children || visible.contains(k.as_str()))
        })
        .cloned()
        .collect();

    // Conflict check: block if any destination key already exists.
    let move_set: std::collections::HashSet<String> = keys_to_move.iter().cloned().collect();
    for k in &keys_to_move {
        let new_k = format!("{new_prefix}{}", &k[old_prefix.len()..]);
        if state.workspace.merged_keys.contains(&new_k) && !move_set.contains(&new_k) {
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

        let missed = snapshot_and_insert(state, old_bundle, old_real, new_bundle, new_real_part);
        all_missed.extend(missed);

        ops::delete::delete_key_inner(state, old_k);
        state.workspace.merged_keys.push(new_k);
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
    state.apply_filter();
}

/// Copy one exact key to `new_key`, keeping the source untouched.
/// Works for same-bundle and cross-bundle destinations.
pub fn commit_exact_copy(state: &mut AppState, old_key: &str, new_key: String) {
    let (old_bundle, old_real) = workspace::split_key(old_key);
    let (new_bundle, new_real) = workspace::split_key(&new_key);

    if new_real.is_empty() {
        state.status_message = Some("Key must have a name after ':'".to_string());
        return;
    }
    if state.workspace.merged_keys.contains(&new_key) {
        state.status_message = Some(format!("'{new_key}' already exists"));
        return;
    }
    if old_bundle != new_bundle && !new_bundle.is_empty() && !state.workspace.is_bundle_name(new_bundle) {
        state.status_message = Some(format!("Bundle '{new_bundle}' does not exist"));
        return;
    }

    let dest_bundle = if new_bundle.is_empty() { old_bundle } else { new_bundle };
    let missed = snapshot_and_insert(state, old_bundle, old_real, dest_bundle, new_real);

    state.workspace.merged_keys.push(new_key.clone());
    state.workspace.merged_keys.sort();

    let base = format!("Copied {old_key} → {new_key}");
    state.status_message = Some(if missed.is_empty() {
        base
    } else {
        format!("{base}  (no file for: {})", missed.join(", "))
    });
    state.edit_buffer = None;
    state.mode = Mode::Normal;
    state.apply_filter();
}

/// Copy every key that equals `old_prefix` or starts with `old_prefix.` to
/// `new_prefix`, keeping all source keys untouched.
pub fn commit_prefix_copy(state: &mut AppState, old_prefix: &str, new_prefix: String, all_children: bool) {
    let (old_bundle, _) = workspace::split_key(old_prefix);
    let (new_bundle, new_real) = workspace::split_key(&new_prefix);

    if new_real.is_empty() {
        state.status_message = Some("Key must have a name after ':'".to_string());
        return;
    }
    if old_bundle != new_bundle && !new_bundle.is_empty() && !state.workspace.is_bundle_name(new_bundle) {
        state.status_message = Some(format!("Bundle '{new_bundle}' does not exist"));
        return;
    }

    let dot_prefix = format!("{old_prefix}.");
    let visible = if !all_children { state.visible_full_keys() } else { std::collections::HashSet::new() };

    let keys_to_copy: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| {
            (*k == old_prefix || k.starts_with(&dot_prefix))
                && (all_children || visible.contains(k.as_str()))
        })
        .cloned()
        .collect();

    // Conflict check: block if any destination key already exists.
    let copy_set: std::collections::HashSet<String> = keys_to_copy.iter().cloned().collect();
    for k in &keys_to_copy {
        let new_k = format!("{new_prefix}{}", &k[old_prefix.len()..]);
        if state.workspace.merged_keys.contains(&new_k) && !copy_set.contains(&new_k) {
            state.status_message = Some(format!("'{new_k}' already exists"));
            return;
        }
    }

    let dest_bundle = if new_bundle.is_empty() { old_bundle } else { new_bundle };
    let mut all_missed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let count = keys_to_copy.len();

    for old_k in &keys_to_copy {
        let new_k = format!("{new_prefix}{}", &old_k[old_prefix.len()..]);
        let (_, old_real) = workspace::split_key(old_k);
        let (_, new_real_part) = workspace::split_key(&new_k);

        let missed = snapshot_and_insert(state, old_bundle, old_real, dest_bundle, new_real_part);
        all_missed.extend(missed);

        state.workspace.merged_keys.push(new_k);
    }

    state.workspace.merged_keys.sort();

    let base = format!("Copied {count} key(s): {old_prefix} → {new_prefix}");
    state.status_message = Some(if all_missed.is_empty() {
        base
    } else {
        let mut missed_vec: Vec<_> = all_missed.into_iter().collect();
        missed_vec.sort();
        format!("{base}  (no file for: {})", missed_vec.join(", "))
    });
    state.edit_buffer = None;
    state.mode = Mode::Normal;
    state.apply_filter();
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
    state.dirty_keys.insert(new_key.to_string());
    for (path, first_line, last_line, value) in found {
        state.pending_writes.push(PendingChange::Update {
            path,
            first_line,
            last_line,
            key: new_real.to_string(), // always write the bare key to the file
            value,
            full_key: new_key.to_string(),
        });
        state.unsaved_changes = true;
    }
}
