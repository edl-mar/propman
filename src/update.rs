use crate::{
    editor::CellEdit,
    messages::Message,
    ops,
    state::{AppState, Mode, PendingChange, SelectionScope},
    workspace,
    writer,
};

/// Recompute `temp_pins` for the current cursor and call `apply_filter` at
/// most **once** if the set actually changed.
///
/// Algorithm: use `view_rows` directly — `is_temp_pinned` already marks rows
/// that are surfaced beyond the natural filter.
///
/// 1. Find the *anchor*: the cursor row if it is naturally visible
///    (`!is_temp_pinned`), otherwise scan backward for the nearest naturally-
///    visible row (the row that "owns" the temp-pinned group the cursor is in).
/// 2. Compute the desired temp_pins: workspace keys that are children of the
///    anchor AND absent from the natural view (non-temp-pinned rows).
/// 3. If the set is unchanged, do nothing — this avoids any `apply_filter`
///    call and leaves the cursor exactly where it is.
fn refresh_temp_pins(state: &mut AppState) {
    if state.selection_scope != SelectionScope::ChildrenAll {
        if !state.temp_pins.is_empty() {
            state.temp_pins.clear();
            state.apply_filter();
        }
        return;
    }

    // ── Step 1: find the anchor row ──────────────────────────────────────────
    let anchor_idx = {
        let cur = state.cursor_row;
        if state.view_rows.get(cur).map_or(true, |r| !r.identity.is_temp_pinned) {
            cur
        } else {
            // Cursor is on a temp-pinned row — owner is the nearest non-temp row
            // scanning backward.
            state.view_rows[..cur].iter().enumerate().rev()
                .find(|(_, r)| !r.identity.is_temp_pinned)
                .map(|(i, _)| i)
                .unwrap_or(cur)
        }
    };

    let anchor = match state.view_rows.get(anchor_idx) {
        Some(r) => r,
        None => return,
    };

    // Bundle headers don't have meaningful leaf-key children; skip.
    if !anchor.identity.is_leaf && anchor.identity.prefix == anchor.identity.bundle {
        if !state.temp_pins.is_empty() {
            state.temp_pins.clear();
            state.apply_filter();
        }
        return;
    }

    // Anchor key: full_key for leaf rows, prefix for group-header rows.
    let anchor_key = anchor.identity.full_key.as_deref()
        .unwrap_or(anchor.identity.prefix.as_str())
        .to_string();
    let dot_key = format!("{anchor_key}.");

    // ── Step 2: compute desired temp_pins ────────────────────────────────────
    // Naturally-visible keys = full_keys of non-temp-pinned rows in view_rows.
    // This is what the filter, dirty-bypass, and pin-bypass already surface.
    let naturally_visible: std::collections::HashSet<&str> = state.view_rows.iter()
        .filter(|r| !r.identity.is_temp_pinned)
        .filter_map(|r| r.identity.full_key.as_deref())
        .collect();

    let new_pins: Vec<String> = state.workspace.merged_keys.iter()
        .filter(|k| {
            (*k == &anchor_key || k.starts_with(&dot_key))
                && !naturally_visible.contains(k.as_str())
        })
        .cloned()
        .collect();

    // ── Step 3: rebuild only if the set changed ───────────────────────────────
    if new_pins == state.temp_pins {
        return;
    }
    state.temp_pins = new_pins;
    state.apply_filter();
}

/// Move the cursor up one visual row and keep the viewport in sync.
fn cursor_up(state: &mut AppState) {
    state.move_up(); // move_up calls clamp_scroll + sync_cursor internally
}

/// Move the cursor down one visual row and keep the viewport in sync.
fn cursor_down(state: &mut AppState) {
    state.move_down();
}

/// Ctrl+Up / Ctrl+Down: jump to the nearest sibling at the current anchor level.
/// Reduces depth and retries if nothing is found. Falls back to plain row movement.
fn sibling_nav(state: &mut AppState, forward: bool) {
    if let Some(row_idx) = state.find_depth_neighbor(forward) {
        state.cursor_row    = row_idx;
        state.cursor_segment = 0;
        state.clamp_scroll();
        refresh_temp_pins(state);
    } else {
        if forward { cursor_down(state) } else { cursor_up(state) }
        refresh_temp_pins(state);
    }
}

/// Pure state transition: (AppState, Message) → AppState.
/// No I/O, no side effects.
pub fn update(mut state: AppState, msg: Message) -> AppState {
    // Clear any one-shot status message on every new keypress.
    state.status_message = None;

    let mode = state.mode.clone();
    let stay_in_paste = matches!(&msg, Message::CommitPasteStay);

    match (mode, msg) {
        // ── Normal mode ──────────────────────────────────────────────────────
        (Mode::Normal, Message::MoveCursorUp) => {
            cursor_up(&mut state);
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::MoveCursorDown) => {
            cursor_down(&mut state);
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::SiblingUp) => {
            sibling_nav(&mut state, false);
        }
        (Mode::Normal, Message::SiblingDown) => {
            sibling_nav(&mut state, true);
        }
        (Mode::Normal, Message::GoToFirstChild) => {
            if state.move_to_first_child() { refresh_temp_pins(&mut state); }
        }
        (Mode::Normal, Message::MoveCursorLeft) => {
            if state.move_cursor_left() { refresh_temp_pins(&mut state); }
        }
        (Mode::Normal, Message::MoveCursorRight) => {
            state.move_cursor_right();
        }
        (Mode::Pasting, Message::MoveCursorLeft) => {
            // In paste mode the key column is navigable but Left from it is a no-op.
            if state.cursor_locale.is_some() { state.move_cursor_left(); }
        }
        (Mode::Pasting, Message::MoveCursorRight) => {
            state.move_cursor_right();
        }
        (Mode::Normal, Message::PageUp) => {
            state.page_up();
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::PageDown) => {
            state.page_down();
            refresh_temp_pins(&mut state);
        }
        (_, Message::JumpToPrevBundle) => {
            state.jump_to_prev_bundle();
        }
        (_, Message::JumpToNextBundle) => {
            state.jump_to_next_bundle();
        }

        (_, Message::CycleScope) => {
            state.selection_scope = state.selection_scope.cycle();
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::StartEdit) => {
            if state.cursor_locale.is_none() {
                // Key column.
                let is_bundle_header = state.view_rows.get(state.cursor_row)
                    .map_or(false, |r| !r.identity.is_leaf && r.identity.prefix == r.identity.bundle);
                // Block rename of bundle-level header rows.
                if is_bundle_header {
                    return state;
                }
                let old_key = match state.cursor_key_for_ops() {
                    Some(k) => k,
                    None => return state,
                };
                // Group header rows have no exact key — force Children scope.
                let is_group_header = state.view_rows.get(state.cursor_row)
                    .map_or(false, |r| !r.identity.is_leaf && r.identity.prefix != r.identity.bundle);
                if is_group_header {
                    state.selection_scope = SelectionScope::Children;
                }
                state.edit_buffer = Some(CellEdit::new(old_key));
                state.mode = Mode::KeyRenaming;
                refresh_temp_pins(&mut state);
            } else {
                // Locale column: block value editing on bundle-level headers.
                let is_bundle_header = state.view_rows.get(state.cursor_row)
                    .map_or(false, |r| !r.identity.is_leaf && r.identity.prefix == r.identity.bundle);
                if is_bundle_header {
                    return state;
                }
                let current_value = state.current_cell_value().unwrap_or_default();
                state.edit_buffer = Some(CellEdit::new(current_value));
                state.mode = Mode::Editing;
            }
        }
        (Mode::Normal, Message::DeleteKey) => {
            if state.cursor_locale.is_none() {
                // Key column.
                let is_bundle_header = state.view_rows.get(state.cursor_row)
                    .map_or(false, |r| !r.identity.is_leaf && r.identity.prefix == r.identity.bundle);
                // Block bundle-level headers.
                if is_bundle_header {
                    return state;
                }
                let key = match state.cursor_key_for_ops() {
                    Some(k) => k,
                    None => return state,
                };
                // Group header rows have no exact key — force Children scope.
                let is_group_header = state.view_rows.get(state.cursor_row)
                    .map_or(false, |r| !r.identity.is_leaf && r.identity.prefix != r.identity.bundle);
                if is_group_header {
                    state.selection_scope = SelectionScope::Children;
                }
                state.edit_buffer = Some(CellEdit::new(key));
                state.mode = Mode::Deleting;
                refresh_temp_pins(&mut state);
            } else {
                // Locale cell: yank then immediately delete (vim-style).
                if let (Some(full_key), Some(locale_idx)) = (
                    state.view_rows.get(state.cursor_row).and_then(|r| r.identity.full_key.clone()),
                    state.effective_locale_idx(),
                ) {
                    if let Some(locale) = state.visible_locales.get(locale_idx).cloned() {
                        if state.current_cell_value().is_some() {
                            state.yank_cell();
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
        (Mode::Normal, Message::TogglePin) => {
            let key = match state.view_rows.get(state.cursor_row).and_then(|r| r.identity.full_key.clone()) {
                Some(k) => k,
                None => {
                    // Bundle header — pin by bundle name (not a key operation).
                    return state;
                }
            };

            let pin = !state.pinned_keys.contains(&key);
            let dot_prefix = format!("{key}.");
            let affected: Vec<String> = match state.selection_scope {
                SelectionScope::Exact => vec![key],
                SelectionScope::Children => {
                    let mut keys = vec![key.clone()];
                    keys.extend(state.visible_full_keys().into_iter().filter(|k| k.starts_with(&dot_prefix)));
                    keys
                }
                SelectionScope::ChildrenAll => {
                    let mut keys = vec![key.clone()];
                    keys.extend(
                        state.workspace.merged_keys.iter()
                            .filter(|k| k.starts_with(&dot_prefix))
                            .cloned()
                    );
                    keys
                }
            };

            let count = affected.len();
            let label = affected.first().cloned().unwrap_or_default();
            for k in affected {
                if pin { state.pinned_keys.insert(k); } else { state.pinned_keys.remove(&k); }
            }

            let action = if pin { "Pinned" } else { "Unpinned" };
            state.status_message = Some(if count == 1 {
                format!("{action} {label}")
            } else {
                format!("{action} {count} keys")
            });

            state.apply_filter();
        }
        (Mode::Normal, Message::YankCell) => {
            match state.yank_cell() {
                Some(locale) => {
                    let value = state.paste.last.as_deref().unwrap_or("");
                    let preview: String = value.replace("\\\n", "").replace('\n', " ");
                    let truncated = if preview.chars().count() > 40 {
                        format!("{}…", preview.chars().take(40).collect::<String>())
                    } else {
                        preview
                    };
                    state.status_message = Some(format!("Yanked [{locale}]: \"{truncated}\""));
                }
                None => {
                    state.status_message = Some("Nothing to yank".to_string());
                }
            }
        }
        (Mode::Normal, Message::YankAndOpenPaste) => {
            match state.yank_cell() {
                Some(locale) => {
                    let locale_keys = state.paste_locales();
                    state.paste.focus_on_locale(Some(&locale), &locale_keys);
                    state.mode = Mode::Pasting;
                }
                None => {
                    state.status_message = Some("Nothing to yank".to_string());
                }
            }
        }
        (Mode::Pasting, Message::PageUp) => {
            let n = state.page_size();
            for _ in 0..n { state.move_up(); }
        }
        (Mode::Pasting, Message::PageDown) => {
            let n = state.page_size();
            for _ in 0..n { state.move_down(); }
        }
        (Mode::Pasting, Message::YankCell) => {
            match state.yank_cell() {
                Some(locale) => {
                    // Shift panel focus to the locale that was just yanked.
                    let locale_keys = state.paste_locales();
                    state.paste.focus_on_locale(Some(&locale), &locale_keys);
                    let value = state.paste.last.as_deref().unwrap_or("");
                    let preview: String = value.replace("\\\n", "").replace('\n', " ");
                    let truncated = if preview.chars().count() > 40 {
                        format!("{}…", preview.chars().take(40).collect::<String>())
                    } else {
                        preview
                    };
                    state.status_message = Some(format!("Yanked [{locale}]: \"{truncated}\""));
                }
                None => {
                    state.status_message = Some("Nothing to yank".to_string());
                }
            }
        }
        (Mode::Pasting, Message::YankToFocusedLocale) => {
            let locale_idx = match state.effective_locale_idx() {
                Some(i) => i,
                None => {
                    state.status_message = Some("Nothing to yank".to_string());
                    return state;
                }
            };
            let value = match state.current_cell_value() {
                Some(v) => v,
                None => {
                    state.status_message = Some("Nothing to yank".to_string());
                    return state;
                }
            };
            let cursor_locale = state.visible_locales.get(locale_idx).cloned().unwrap_or_default();
            let locale_keys = state.paste_locales();
            let target_locale = match locale_keys.into_iter().nth(state.paste.locale_cursor) {
                Some(l) => l,
                None => return state,
            };
            // Push into the panel-focused locale's history (not the table-cursor locale).
            state.paste.yank(target_locale.clone(), value.clone());
            let preview: String = value.replace("\\\n", "").replace('\n', " ");
            let truncated = if preview.chars().count() > 40 {
                format!("{}…", preview.chars().take(40).collect::<String>())
            } else { preview };
            state.status_message = Some(format!(
                "Yanked [{cursor_locale}] → [{target_locale}]: \"{truncated}\""
            ));
        }
        (Mode::Normal, Message::OpenPaste) => {
            if state.paste.history.is_empty() {
                state.status_message = Some("Clipboard is empty".to_string());
            } else {
                let locale_keys = state.paste_locales();
                let cursor_locale = state.cursor_locale.as_deref();
                state.paste.focus_on_locale(cursor_locale, &locale_keys);
                state.mode = Mode::Pasting;
            }
        }
        (Mode::Normal, Message::QuickPaste) => {
            match state.paste.last.clone() {
                None => {
                    state.status_message = Some("Clipboard is empty".to_string());
                }
                Some(value) => {
                    if state.cursor_locale.is_none() {
                        state.status_message = Some("Select a locale cell to quick-paste".to_string());
                    } else if state.current_cell_value().is_some() {
                        ops::insert::commit_cell_edit(&mut state, value);
                        state.apply_filter();
                    } else {
                        ops::insert::commit_cell_insert(&mut state, value);
                        state.apply_filter();
                    }
                }
            }
        }

        // ── Paste mode ───────────────────────────────────────────────────────
        (Mode::Pasting, Message::QuickPaste) => {
            match state.paste.last.clone() {
                None => {
                    state.status_message = Some("Clipboard is empty".to_string());
                }
                Some(value) => {
                    if state.cursor_locale.is_none() {
                        state.status_message = Some("Select a locale cell to paste".to_string());
                    } else {
                        if state.current_cell_value().is_some() {
                            ops::insert::commit_cell_edit(&mut state, value);
                        } else {
                            ops::insert::commit_cell_insert(&mut state, value);
                        }
                        state.apply_filter();
                        state.mode = Mode::Normal;
                    }
                }
            }
        }
        (Mode::Pasting, Message::PasteHere) => {
            // Paste clipboard_last into current cell without leaving paste mode.
            match state.paste.last.clone() {
                None => {
                    state.status_message = Some("Clipboard is empty".to_string());
                }
                Some(value) => {
                    if state.cursor_locale.is_none() {
                        state.status_message = Some("Select a locale cell to paste".to_string());
                    } else {
                        if state.current_cell_value().is_some() {
                            ops::insert::commit_cell_edit(&mut state, value);
                        } else {
                            ops::insert::commit_cell_insert(&mut state, value);
                        }
                        state.apply_filter();
                    }
                }
            }
        }
        (Mode::Pasting, Message::CancelEdit) => {
            state.mode = Mode::Normal;
        }
        (Mode::Pasting, Message::MoveCursorUp) => {
            cursor_up(&mut state);
        }
        (Mode::Pasting, Message::MoveCursorDown) => {
            cursor_down(&mut state);
        }
        (Mode::Pasting, Message::PasteNavLeft) => {
            state.paste.nav_left();
        }
        (Mode::Pasting, Message::PasteNavRight) => {
            let n = state.paste_locales().len();
            state.paste.nav_right(n);
        }
        (Mode::Pasting, Message::PasteNavUp) => {
            let locale_keys = state.paste_locales();
            if let Some(locale) = locale_keys.into_iter().nth(state.paste.locale_cursor) {
                state.paste.nav_up(&locale);
            }
        }
        (Mode::Pasting, Message::PasteNavDown) => {
            let locale_keys = state.paste_locales();
            if let Some(locale) = locale_keys.into_iter().nth(state.paste.locale_cursor) {
                state.paste.nav_down(&locale);
            }
        }
        (Mode::Pasting, Message::RemovePasteEntry) => {
            let locale_keys = state.paste_locales();
            if let Some(locale) = locale_keys.into_iter().nth(state.paste.locale_cursor) {
                if state.paste.remove_entry(&locale) {
                    state.mode = Mode::Normal;
                    state.status_message = Some("Clipboard is empty".to_string());
                }
            }
        }
        (Mode::Pasting, Message::CommitPasteStay) |
        (Mode::Pasting, Message::CommitPaste) => {
            // Paste all locales' selected history entries into the cursor row's key.
            let full_key = match state.view_rows.get(state.cursor_row).and_then(|r| r.identity.full_key.clone()) {
                Some(k) => k,
                None => {
                    state.status_message = Some("Select a key row to paste".to_string());
                    return state;
                }
            };
            let to_paste: Vec<(String, String)> = {
                let locale_keys = state.paste_locales();
                let (bundle, _) = workspace::split_key(&full_key);
                locale_keys.iter()
                    .filter(|locale| state.workspace.bundle_has_locale(bundle, locale))
                    .filter_map(|locale| {
                        let pos = *state.paste.history_pos.get(locale).unwrap_or(&0);
                        state.paste.history.get(locale).and_then(|h| h.get(pos)).cloned()
                            .map(|v| (locale.clone(), v))
                    })
                    .collect()
            };
            let count = to_paste.len();
            for (locale, value) in to_paste {
                ops::insert::apply_cell_value(&mut state, &full_key, &locale, value);
            }
            state.status_message = Some(format!("Pasted {count} locale(s) into {full_key}"));
            state.apply_filter();
            if !stay_in_paste {
                state.mode = Mode::Normal;
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
        (Mode::Normal, Message::NewBundle) => {
            state.edit_buffer = Some(CellEdit::new(String::new()));
            state.mode = Mode::BundleNaming;
        }

        // ── BundleNaming mode ────────────────────────────────────────────────
        (Mode::BundleNaming, Message::CommitBundleName) => {
            let bundle_name = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            if bundle_name.is_empty() {
                return state;
            }

            // Reject if a bundle with this name already exists.
            if state.workspace.groups.iter().any(|g| g.base_name == bundle_name) {
                state.status_message = Some(format!("Bundle '{bundle_name}' already exists"));
                return state;
            }

            // Determine directory and first locale from existing bundles.
            let (dir, first_locale) = {
                let existing = state.workspace.groups.iter()
                    .find(|g| !g.base_name.is_empty() && !g.files.is_empty());
                let dir = existing
                    .and_then(|g| g.files.first())
                    .and_then(|f| f.path.parent())
                    .map(|p| p.to_path_buf());
                let locale = existing
                    .and_then(|g| g.files.first())
                    .map(|f| f.locale.clone())
                    .unwrap_or_else(|| "default".to_string());
                (dir, locale)
            };

            let dir = match dir {
                Some(d) => d,
                None => std::env::current_dir().unwrap_or_default(),
            };

            let filename = format!("{}_{}.properties", bundle_name, first_locale);
            let new_path = dir.join(&filename);

            if let Err(e) = std::fs::File::create(&new_path) {
                state.status_message = Some(format!("Failed to create file: {e}"));
                return state;
            }

            // Register the new group in the workspace.
            state.workspace.groups.push(crate::workspace::FileGroup {
                base_name: bundle_name.clone(),
                files: vec![crate::workspace::PropertiesFile {
                    path: new_path,
                    locale: first_locale.clone(),
                    entries: Vec::new(),
                }],
            });

            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();

            // Navigate to the new bundle header.
            if let Some(idx) = state.view_rows.iter().position(|r| {
                !r.identity.is_leaf && r.identity.bundle == bundle_name && r.identity.prefix == bundle_name
            }) {
                state.cursor_row = idx;
            }
            state.cursor_locale = None;
            state.clamp_scroll();

            state.status_message = Some(format!("Created bundle '{bundle_name}' ({filename})"));
        }
        (Mode::BundleNaming, Message::CancelEdit) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
        }
        (Mode::BundleNaming, Message::TextInput(key)) => {
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.input(tui_textarea::Input::from(key));
            }
        }
        (Mode::BundleNaming, Message::MoveCursorUp) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            cursor_up(&mut state);
        }
        (Mode::BundleNaming, Message::MoveCursorDown) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            cursor_down(&mut state);
        }

        (Mode::Normal, Message::NewKey) => {
            let locale = state.cursor_locale.clone();

            // Bundle-level header, key column: open locale-naming to add a new locale file.
            let row = state.view_rows.get(state.cursor_row);
            let id = row.map(|r| &r.identity);
            let bundle_str = id.map_or("", |id| id.bundle.as_str()).to_string();
            let is_bundle_hdr = id.map_or(false, |id| !id.is_leaf && id.prefix == id.bundle);
            let is_group_hdr  = id.map_or(false, |id| !id.is_leaf && id.prefix != id.bundle);

            if is_bundle_hdr && locale.is_none() {
                state.edit_buffer = Some(CellEdit::new(String::new()));
                state.mode = Mode::LocaleNaming;
                return state;
            }

            let pre = if is_bundle_hdr {
                let locale_str = locale.as_deref().unwrap_or("default");
                format!("{bundle_str}:{locale_str}:")
            } else {
                let key_str = id.map(|id| {
                    id.full_key.as_deref().unwrap_or(&id.prefix).to_string()
                }).unwrap_or_default();
                let (_, real_key) = workspace::split_key(&key_str);
                let key_prefix = if is_group_hdr {
                    format!("{real_key}.")
                } else {
                    match real_key.rfind('.') {
                        Some(i) => format!("{}.", &real_key[..i]),
                        None    => String::new(),
                    }
                };
                if let Some(ref loc) = locale {
                    if !bundle_str.is_empty() {
                        format!("{bundle_str}:{loc}:{key_prefix}")
                    } else {
                        key_prefix
                    }
                } else if !bundle_str.is_empty() {
                    format!("{bundle_str}:{key_prefix}")
                } else {
                    key_prefix
                }
            };
            state.edit_buffer = Some(CellEdit::new(pre));
            state.mode = Mode::KeyNaming;
        }

        // ── KeyNaming mode ───────────────────────────────────────────────────
        (Mode::KeyNaming, Message::CommitKeyName) => {
            let input = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            // Parse input. Three forms are accepted:
            //   bundle:key                — 2-segment, no locale targeting
            //   bundle:locale:key         — 3-segment, locale targeted, open edit after
            //   bundle:locale:key=value   — 3-segment with inline value, write immediately
            let parts: Vec<&str> = input.splitn(3, ':').collect();
            let (new_key, target_locale, inline_value): (String, Option<String>, Option<String>) =
                if parts.len() == 3 && !parts[0].is_empty() && !parts[1].is_empty() && !parts[2].is_empty() {
                    // Split the key segment on the first '=' to extract an optional inline value.
                    let (key_part, val_opt) = match parts[2].find('=') {
                        Some(idx) => (&parts[2][..idx], Some(parts[2][idx + 1..].to_string())),
                        None      => (parts[2], None),
                    };
                    if key_part.is_empty() {
                        (input.clone(), None, None) // malformed — fall through to invalid
                    } else {
                        (format!("{}:{}", parts[0], key_part), Some(parts[1].to_string()), val_opt)
                    }
                } else {
                    (input.clone(), None, None)
                };

            // Valid key: non-empty, not already known, and either:
            //   - contains a '.' (bare key: "app.title")
            //   - is bundle-qualified with a non-empty real key ("messages:app")
            let (_, real_part) = workspace::split_key(&new_key);
            let is_valid = !new_key.is_empty()
                && !state.workspace.merged_keys.contains(&new_key)
                && (new_key.contains('.') || (!real_part.is_empty() && new_key.contains(':')));

            if is_valid {
                state.edit_buffer = None;

                // If a locale was specified, ensure the locale file exists in this bundle.
                if let Some(ref locale) = target_locale {
                    let (bundle, _) = workspace::split_key(&new_key);
                    if !bundle.is_empty() && !state.workspace.bundle_has_locale(bundle, locale) {
                        let bundle = bundle.to_string();
                        if let Err(msg) = ensure_locale_file(&mut state, &bundle, locale) {
                            state.status_message = Some(msg);
                            state.mode = Mode::Normal;
                            return state;
                        }
                    }
                }

                // Register in the workspace and rebuild the display.
                state.workspace.merged_keys.push(new_key.clone());
                state.workspace.merged_keys.sort();

                // If the immediate parent is a dangling placeholder (in merged_keys
                // but no file entry), it is now a pure namespace — drop it.
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

                // Navigate to the new key.
                if let Some(idx) = state.view_rows.iter().position(|r| {
                    r.identity.full_key.as_deref() == Some(new_key.as_str())
                }) {
                    state.cursor_row = idx;
                }
                state.cursor_segment = 0;
                state.cursor_locale  = target_locale.clone()
                    .or_else(|| state.visible_locales.first().cloned());
                state.clamp_scroll();

                if let Some(value) = inline_value {
                    // Inline value supplied: write it directly into the target locale.
                    // `target_locale` is always Some when inline_value is Some (3-segment).
                    if let Some(ref locale) = target_locale {
                        ops::insert::apply_cell_value(&mut state, &new_key, locale, value);
                        state.apply_filter();
                    }
                    state.mode = Mode::Normal;
                } else if target_locale.is_some() {
                    // Locale-targeted creation, no value: open the editor so the
                    // user can immediately type the translation.
                    state.edit_buffer = Some(CellEdit::new(String::new()));
                    state.mode = Mode::Editing;
                } else {
                    state.mode = Mode::Normal;
                }
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

        // ── LocaleNaming mode ────────────────────────────────────────────────
        (Mode::LocaleNaming, Message::CommitLocaleName) => {
            let locale_name = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            if locale_name.is_empty() {
                return state; // Stay in LocaleNaming.
            }

            // Cursor must still be on the bundle header.
            let bundle = match state.view_rows.get(state.cursor_row) {
                Some(r) if !r.identity.is_leaf
                    && r.identity.prefix == r.identity.bundle
                    && !r.identity.bundle.is_empty() =>
                {
                    r.identity.bundle.clone()
                }
                _ => {
                    state.status_message = Some("No bundle selected".to_string());
                    return state;
                }
            };

            // Reject if the locale already exists in this bundle.
            let already_exists = state.workspace.groups.iter()
                .any(|g| g.base_name == bundle
                    && g.files.iter().any(|f| f.locale == locale_name));
            if already_exists {
                state.status_message = Some(
                    format!("[{locale_name}] already exists in bundle '{bundle}'")
                );
                return state;
            }

            // Derive the target directory from the bundle's first existing file.
            let dir = state.workspace.groups.iter()
                .find(|g| g.base_name == bundle)
                .and_then(|g| g.files.first())
                .and_then(|f| f.path.parent())
                .map(|p| p.to_path_buf());

            let dir = match dir {
                Some(d) => d,
                None => {
                    state.status_message = Some(
                        format!("Cannot find directory for bundle '{bundle}'")
                    );
                    return state;
                }
            };

            let filename = format!("{}_{}.properties", bundle, locale_name);
            let new_path = dir.join(&filename);

            if let Err(e) = std::fs::File::create(&new_path) {
                state.status_message = Some(format!("Failed to create file: {e}"));
                return state;
            }

            // Register the new file in the workspace.
            if let Some(group) = state.workspace.groups.iter_mut()
                .find(|g| g.base_name == bundle)
            {
                group.files.push(crate::workspace::PropertiesFile {
                    path: new_path,
                    locale: locale_name.clone(),
                    entries: Vec::new(),
                });
                // Keep files sorted: "default" first, then alphabetically.
                group.files.sort_by(|a, b| {
                    match (a.locale.as_str(), b.locale.as_str()) {
                        ("default", _) => std::cmp::Ordering::Less,
                        (_, "default") => std::cmp::Ordering::Greater,
                        (a, b)        => a.cmp(b),
                    }
                });
            }

            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
            state.status_message = Some(
                format!("Created {filename}")
            );
        }
        (Mode::LocaleNaming, Message::CancelEdit) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
        }
        (Mode::LocaleNaming, Message::TextInput(key)) => {
            if let Some(edit) = state.edit_buffer.as_mut() {
                edit.textarea.input(tui_textarea::Input::from(key));
            }
        }
        (Mode::LocaleNaming, Message::MoveCursorUp) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            cursor_up(&mut state);
        }
        (Mode::LocaleNaming, Message::MoveCursorDown) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            cursor_down(&mut state);
        }

        // ── KeyRenaming mode ─────────────────────────────────────────────────
        (Mode::KeyRenaming, Message::CommitKeyRename) => {
            let new_key = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            let old_key = match state.cursor_key_for_ops() {
                Some(k) => k,
                None => { state.edit_buffer = None; state.mode = Mode::Normal; return state; }
            };

            // Validate: non-empty, has a dot or colon, not the same as before.
            if new_key.is_empty() || (!new_key.contains('.') && !new_key.contains(':')) {
                state.status_message = Some("Key must contain at least one '.'".to_string());
                // Stay in KeyRenaming so the user can fix it.
            } else if new_key != old_key {
                match state.selection_scope {
                    SelectionScope::Children => {
                        // Only filter-visible children; hidden ones ignored.
                        // Clear temp_pins before the op so they don't stay visible.
                        state.temp_pins.clear();
                        ops::rename::commit_prefix_rename(&mut state, &old_key, new_key, false);
                    }
                    SelectionScope::ChildrenAll => {
                        // All children including hidden (temp-pinned).
                        // Dirty tracking is automatic (rename_key_in_workspace marks new keys dirty).
                        // Use `#` in the filter to review changed entries after the op.
                        state.temp_pins.clear();
                        ops::rename::commit_prefix_rename(&mut state, &old_key, new_key, true);
                    }
                    SelectionScope::Exact => {
                        state.temp_pins.clear();
                        ops::rename::commit_exact_rename(&mut state, &old_key, new_key);
                    }
                }
            } else {
                // No change — just close.
                state.temp_pins.clear();
                state.edit_buffer = None;
                state.mode = Mode::Normal;
                state.apply_filter();
            }
        }
        (Mode::KeyRenaming, Message::CommitKeyCopy) => {
            let new_key = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            let old_key = match state.cursor_key_for_ops() {
                Some(k) => k,
                None => { state.edit_buffer = None; state.mode = Mode::Normal; return state; }
            };

            if new_key.is_empty() || (!new_key.contains('.') && !new_key.contains(':')) {
                state.status_message = Some("Key must contain at least one '.'".to_string());
            } else if new_key == old_key {
                state.status_message = Some("Copy destination is the same as source".to_string());
            } else {
                match state.selection_scope {
                    SelectionScope::Exact => {
                        state.temp_pins.clear();
                        ops::rename::commit_exact_copy(&mut state, &old_key, new_key);
                    }
                    SelectionScope::Children => {
                        state.temp_pins.clear();
                        ops::rename::commit_prefix_copy(&mut state, &old_key, new_key, false);
                    }
                    SelectionScope::ChildrenAll => {
                        state.temp_pins.clear();
                        ops::rename::commit_prefix_copy(&mut state, &old_key, new_key, true);
                    }
                }
            }
        }
        (Mode::KeyRenaming, Message::CancelEdit) => {
            state.temp_pins.clear();
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
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

            state.temp_pins.clear(); // Discard temp pins before the op.
            match state.selection_scope {
                SelectionScope::Children   => ops::delete::delete_key_prefix(&mut state, &key, false),
                SelectionScope::ChildrenAll => ops::delete::delete_key_prefix(&mut state, &key, true),
                SelectionScope::Exact      => ops::delete::delete_key(&mut state, &key),
            }

            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
        }
        (Mode::Deleting, Message::CancelEdit) => {
            state.temp_pins.clear();
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
        }

        // Up/Down in any edit mode: cancel and immediately move (mirrors Filter).
        (
            Mode::Editing | Mode::KeyNaming | Mode::KeyRenaming | Mode::Deleting,
            Message::MoveCursorUp,
        ) => {
            state.temp_pins.clear();
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
            cursor_up(&mut state);
        }
        (
            Mode::Editing | Mode::KeyNaming | Mode::KeyRenaming | Mode::Deleting,
            Message::MoveCursorDown,
        ) => {
            state.temp_pins.clear();
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
            cursor_down(&mut state);
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
            cursor_up(&mut state);
        }
        (Mode::Filter, Message::MoveCursorDown) => {
            state.mode = Mode::Normal;
            cursor_down(&mut state);
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
                    PendingChange::Update { path, first_line, last_line, key, value, .. } =>
                        writer::write_change(path, *first_line, *last_line, key, value),
                    PendingChange::Insert { path, after_line, key, value, .. } =>
                        writer::write_insert(path, *after_line, key, value),
                    PendingChange::Delete { path, first_line, last_line, .. } =>
                        writer::write_delete(path, *first_line, *last_line),
                };
                if result.is_err() {
                    state.pending_writes.push(change);
                }
            }
            state.unsaved_changes = !state.pending_writes.is_empty();
            // Rebuild dirty_keys to reflect only writes that are still pending.
            state.dirty_keys = state.pending_writes.iter()
                .map(|c| match c {
                    PendingChange::Update { full_key, .. } => full_key,
                    PendingChange::Insert { full_key, .. } => full_key,
                    PendingChange::Delete { full_key, .. } => full_key,
                })
                .cloned()
                .collect();
        }
        (_, Message::Quit) => {
            state.quitting = true;
        }

        _ => {}
    }

    state
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Ensures the locale file for `bundle`+`locale` exists, creating it if not.
/// Returns `Ok(filename)` on success (created or already present), `Err(msg)` on failure.
/// Does nothing and returns `Ok("")` if the locale already exists.
fn ensure_locale_file(state: &mut AppState, bundle: &str, locale: &str) -> Result<String, String> {
    if state.workspace.bundle_has_locale(bundle, locale) {
        return Ok(String::new()); // Already exists — nothing to do.
    }

    let dir = state.workspace.groups.iter()
        .find(|g| g.base_name == bundle)
        .and_then(|g| g.files.first())
        .and_then(|f| f.path.parent())
        .map(|p| p.to_path_buf());

    let dir = match dir {
        Some(d) => d,
        None => return Err(format!("Cannot find directory for bundle '{bundle}'")),
    };

    // "default" maps to `bundle.properties` (no underscore suffix).
    let filename = if locale == "default" {
        format!("{bundle}.properties")
    } else {
        format!("{bundle}_{locale}.properties")
    };
    let new_path = dir.join(&filename);

    if let Err(e) = std::fs::File::create(&new_path) {
        return Err(format!("Failed to create file: {e}"));
    }

    if let Some(group) = state.workspace.groups.iter_mut().find(|g| g.base_name == bundle) {
        group.files.push(crate::workspace::PropertiesFile {
            path: new_path,
            locale: locale.to_string(),
            entries: Vec::new(),
        });
        group.files.sort_by(|a, b| match (a.locale.as_str(), b.locale.as_str()) {
            ("default", _) => std::cmp::Ordering::Less,
            (_, "default") => std::cmp::Ordering::Greater,
            (a, b)         => a.cmp(b),
        });
    }

    Ok(filename)
}


