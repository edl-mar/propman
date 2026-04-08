use crate::{
    editor::CellEdit,
    messages::Message,
    ops,
    state::{AppState, Mode, PendingChange, SelectionScope},
    workspace,
    writer,
};

/// Recompute `temp_pins` for the key at the current cursor row.
///
/// - Clears any existing temp pins and rebuilds display without them first.
/// - If scope is `ChildrenAll`, finds every child of the cursor key that is
///   NOT currently visible in `display_rows` and adds them to `temp_pins`,
///   then triggers another `apply_filter` so they surface in the table.
/// - For any other scope, just clears temp pins (one `apply_filter` call).
fn refresh_temp_pins(state: &mut AppState) {
    state.temp_pins.clear();
    state.apply_filter(); // Rebuild with no old pins; gives us the "true" visible set.

    if state.selection_scope != SelectionScope::ChildrenAll {
        return;
    }

    let key = match state.cursor.full_key() {
        Some(k) => k,
        None => return, // Bundle header — no children to pin.
    };

    let dot_key = format!("{key}.");
    let currently_visible: std::collections::HashSet<String> = state.render_model.bundles.iter()
        .flat_map(|b| b.entries.iter().map(move |e| {
            let pfx = if b.name.is_empty() { String::new() } else { format!("{}:", b.name) };
            format!("{pfx}{}", e.segments.join("."))
        }))
        .collect();

    state.temp_pins = state.workspace.merged_keys.iter()
        .filter(|k| {
            (*k == &key || k.starts_with(&dot_key))
                && !currently_visible.contains(*k)
        })
        .cloned()
        .collect();

    if !state.temp_pins.is_empty() {
        state.apply_filter(); // Rebuild again to include the newly temp-pinned rows.
    }
}

/// Ctrl+Up / Ctrl+Down: jump to the nearest sibling at the current anchor level.
/// If no sibling exists at that level, walk one level up (same logic as Left —
/// Navigate to the previous/next row at the same absolute segment depth as the
/// current anchor, regardless of parent ancestry ("cousin" navigation).
/// Falls back to plain row movement when nothing is found at any depth.
fn sibling_nav(state: &mut AppState, forward: bool) {
    if let Some((bundle, segs)) = state.find_depth_neighbor(forward) {
        state.cursor.bundle   = bundle;
        state.cursor.segments = segs;
        state.cursor.locale   = None; // sibling nav resets to key column
        state.clamp_scroll();
        refresh_temp_pins(state);
    } else {
        state.cursor.locale = None;
        let cur = state.cursor_visual_row();
        let max = state.total_visual_rows().saturating_sub(1);
        let new_row = if forward { (cur + 1).min(max) } else { cur.saturating_sub(1) };
        state.set_visual_row(new_row);
        state.clamp_scroll();
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
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(1));
            state.cursor.locale = None; // reset to key column on row movement
            state.clamp_scroll();
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::MoveCursorDown) => {
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 1).min(max));
            state.cursor.locale = None;
            state.clamp_scroll();
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::SiblingUp) => {
            sibling_nav(&mut state, false);
        }
        (Mode::Normal, Message::SiblingDown) => {
            sibling_nav(&mut state, true);
        }
        (Mode::Normal, Message::GoToFirstChild) => {
            if let Some((bundle, segs)) = state.find_first_child_position() {
                state.cursor.bundle   = bundle;
                state.cursor.segments = segs;
                state.cursor.locale   = None;
                state.clamp_scroll();
                refresh_temp_pins(&mut state);
            }
        }
        (Mode::Normal, Message::MoveCursorLeft) => {
            if state.cursor.locale.is_some() {
                // Locale column → step left, skipping missing locales.
                // Moving left from the first locale lands on the key column.
                let bundle = state.current_row_bundle().to_string();
                let cur_idx = state.effective_locale_idx().unwrap_or(0);
                let mut found = false;
                if cur_idx > 0 {
                    let mut i = cur_idx - 1;
                    loop {
                        if bundle.is_empty() || state.workspace.bundle_has_locale(&bundle, &state.visible_locales[i]) {
                            state.set_locale_cursor(i);
                            found = true;
                            break;
                        }
                        if i == 0 { break; }
                        i -= 1;
                    }
                }
                if !found {
                    state.cursor.locale = None;
                }
            } else {
                // Key column: pop one segment toward root.
                //
                // When the parent is an exact visual row (e.g. a group header),
                // the cursor lands on it.  When the parent is a chain-collapsed
                // intermediate node with no visual row, the cursor becomes a
                // "prefix anchor": cursor_visual_row() and the renderer both
                // handle this case by showing the effective descendant entry
                // with the anchor segment highlighted.
                if !state.cursor.segments.is_empty() {
                    state.cursor.segments.pop();
                    state.cursor.locale = None;
                    state.clamp_scroll();
                    refresh_temp_pins(&mut state);
                }
            }
        }
        (Mode::Normal, Message::MoveCursorRight) => {
            if state.cursor.locale.is_none() {
                if state.cursor_is_prefix_anchor() {
                    // Prefix-anchor mode: push one segment toward the leaf.
                    // This is the inverse of Left (pop toward root).
                    let cur_segs = state.cursor.segments.clone();
                    let positions = state.visual_positions();
                    if let Some(pos) = positions.iter().find(|p| {
                        p.bundle == state.cursor.bundle
                            && p.segments.starts_with(cur_segs.as_slice())
                            && p.segments.len() > cur_segs.len()
                    }) {
                        // Push exactly the next segment of the effective entry.
                        let next = pos.segments[cur_segs.len()].clone();
                        state.cursor.segments.push(next);
                        state.clamp_scroll();
                        refresh_temp_pins(&mut state);
                    }
                } else {
                    // Exact row: move to first available locale column.
                    let bundle = state.current_row_bundle().to_string();
                    let max = state.visible_locales.len();
                    let mut i = 0;
                    while i < max && !bundle.is_empty() && !state.workspace.bundle_has_locale(&bundle, &state.visible_locales[i]) {
                        i += 1;
                    }
                    if i < max {
                        state.set_locale_cursor(i);
                    }
                }
            } else {
                // Locale column: step right, skipping missing locales.
                let bundle = state.current_row_bundle().to_string();
                let cur_idx = state.effective_locale_idx().unwrap_or(0);
                let max = state.visible_locales.len();
                let mut i = cur_idx + 1;
                while i < max && !bundle.is_empty() && !state.workspace.bundle_has_locale(&bundle, &state.visible_locales[i]) {
                    i += 1;
                }
                if i < max {
                    state.set_locale_cursor(i);
                }
            }
        }
        (Mode::Pasting, Message::MoveCursorLeft) => {
            let bundle = state.current_row_bundle().to_string();
            let cur_idx = match state.effective_locale_idx() {
                Some(i) => i,
                None => return state,
            };
            let mut found = false;
            if cur_idx > 0 {
                let mut i = cur_idx - 1;
                loop {
                    if bundle.is_empty() || state.workspace.bundle_has_locale(&bundle, &state.visible_locales[i]) {
                        state.set_locale_cursor(i);
                        found = true;
                        break;
                    }
                    if i == 0 { break; }
                    i -= 1;
                }
            }
            if !found {
                state.cursor.locale = None;
            }
        }
        (Mode::Pasting, Message::MoveCursorRight) => {
            let bundle = state.current_row_bundle().to_string();
            let max = state.visible_locales.len();
            let start = match state.effective_locale_idx() {
                None => 0,
                Some(idx) => idx + 1,
            };
            let mut i = start;
            while i < max && !bundle.is_empty() && !state.workspace.bundle_has_locale(&bundle, &state.visible_locales[i]) {
                i += 1;
            }
            if i < max {
                state.set_locale_cursor(i);
            }
        }
        (Mode::Normal, Message::PageUp) => {
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(20));
            state.cursor.locale = None;
            state.clamp_scroll();
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::PageDown) => {
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 20).min(max));
            state.cursor.locale = None;
            state.clamp_scroll();
            refresh_temp_pins(&mut state);
        }
        (_, Message::JumpToPrevBundle) => {
            let positions = state.visual_positions();
            let cur = state.cursor_visual_row();
            let target = (0..cur).rev()
                .find(|&i| positions[i].segments.is_empty() && positions[i].bundle.is_some());
            if let Some(row) = target {
                state.set_visual_row(row);
                state.cursor.locale = None;
                state.clamp_scroll();
            }
        }
        (_, Message::JumpToNextBundle) => {
            let positions = state.visual_positions();
            let cur = state.cursor_visual_row();
            let target = (cur + 1..positions.len())
                .find(|&i| positions[i].segments.is_empty() && positions[i].bundle.is_some());
            if let Some(row) = target {
                state.set_visual_row(row);
                state.cursor.locale = None;
                state.clamp_scroll();
            }
        }

        (_, Message::CycleScope) => {
            state.selection_scope = state.selection_scope.cycle();
            refresh_temp_pins(&mut state);
        }
        (Mode::Normal, Message::StartEdit) => {
            if state.cursor.is_key_col() {
                // Block rename of bundle-level header rows.
                if state.cursor.is_bundle_header() {
                    return state;
                }
                let old_key = match state.cursor.full_key() {
                    Some(k) => k,
                    None => return state,
                };
                // Group header rows have no exact key — force Children scope.
                let is_group_header = !state.render_model.bundles.iter()
                    .flat_map(|b| b.entries.iter())
                    .any(|e| e.segments == state.cursor.segments);
                if is_group_header {
                    state.selection_scope = SelectionScope::Children;
                }
                state.edit_buffer = Some(CellEdit::new(old_key));
                state.mode = Mode::KeyRenaming;
                refresh_temp_pins(&mut state);
            } else {
                // Block value editing on bundle-level headers.
                if state.cursor.is_bundle_header() {
                    return state;
                }
                let current_value = state.current_cell_value().unwrap_or_default();
                state.edit_buffer = Some(CellEdit::new(current_value));
                state.mode = Mode::Editing;
            }
        }
        (Mode::Normal, Message::DeleteKey) => {
            if state.cursor.is_key_col() {
                // Block bundle-level headers.
                if state.cursor.is_bundle_header() {
                    return state;
                }
                let key = match state.cursor.full_key() {
                    Some(k) => k,
                    None => return state,
                };
                // Group header rows have no exact key — force Children scope.
                let is_group_header = !state.render_model.bundles.iter()
                    .flat_map(|b| b.entries.iter())
                    .any(|e| e.segments == state.cursor.segments);
                if is_group_header {
                    state.selection_scope = SelectionScope::Children;
                }
                state.edit_buffer = Some(CellEdit::new(key));
                state.mode = Mode::Deleting;
                refresh_temp_pins(&mut state);
            } else {
                // Locale cell: yank then immediately delete (vim-style).
                if let (Some(full_key), Some(locale_idx)) = (
                    state.cursor.full_key(),
                    state.effective_locale_idx(),
                ) {
                    if let Some(locale) = state.visible_locales.get(locale_idx).cloned() {
                        if state.workspace.get_value(&full_key, &locale).is_some() {
                            yank_cell(&mut state);
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
            let key = match state.cursor.full_key() {
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
            match yank_cell(&mut state) {
                Some(locale) => {
                    let value = state.clipboard_last.as_deref().unwrap_or("");
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
            match yank_cell(&mut state) {
                Some(locale) => {
                    // Pre-select the locale that was just yanked.
                    let locale_keys = state.paste_locales();
                    let n = locale_keys.len();
                    if let Some(idx) = locale_keys.iter().position(|l| l == &locale) {
                        state.paste_locale_cursor = idx;
                    }
                    state.paste_locale_cursor = state.paste_locale_cursor.min(n.saturating_sub(1));
                    state.mode = Mode::Pasting;
                }
                None => {
                    state.status_message = Some("Nothing to yank".to_string());
                }
            }
        }
        (Mode::Pasting, Message::PageUp) => {
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(20));
            state.clamp_scroll();
        }
        (Mode::Pasting, Message::PageDown) => {
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 20).min(max));
            state.clamp_scroll();
        }
        (Mode::Pasting, Message::YankCell) => {
            match yank_cell(&mut state) {
                Some(locale) => {
                    // Shift panel focus to the locale that was just yanked.
                    let locale_keys = state.paste_locales();
                    let n = locale_keys.len();
                    if let Some(idx) = locale_keys.iter().position(|l| l == &locale) {
                        state.paste_locale_cursor = idx;
                    }
                    state.paste_locale_cursor = state.paste_locale_cursor.min(n.saturating_sub(1));
                    let value = state.clipboard_last.as_deref().unwrap_or("");
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
            let target_locale = match locale_keys.into_iter().nth(state.paste_locale_cursor) {
                Some(l) => l,
                None => return state,
            };
            let history = state.clipboard.entry(target_locale.clone()).or_insert_with(Vec::new);
            history.retain(|v| v != &value);
            history.insert(0, value.clone());
            history.truncate(10);
            state.paste_history_pos.insert(target_locale.clone(), 0);
            state.clipboard_last = Some(value.clone());
            let preview: String = value.replace("\\\n", "").replace('\n', " ");
            let truncated = if preview.chars().count() > 40 {
                format!("{}…", preview.chars().take(40).collect::<String>())
            } else { preview };
            state.status_message = Some(format!(
                "Yanked [{cursor_locale}] → [{target_locale}]: \"{truncated}\""
            ));
        }
        (Mode::Normal, Message::OpenPaste) => {
            if state.clipboard.is_empty() {
                state.status_message = Some("Clipboard is empty".to_string());
            } else {
                let locale_keys = state.paste_locales();
                let n = locale_keys.len();
                if let Some(locale) = state.cursor.locale.as_ref() {
                    if let Some(pos) = locale_keys.iter().position(|l| l == locale) {
                        state.paste_locale_cursor = pos;
                    }
                }
                state.paste_locale_cursor = state.paste_locale_cursor.min(n.saturating_sub(1));
                state.mode = Mode::Pasting;
            }
        }
        (Mode::Normal, Message::QuickPaste) => {
            match state.clipboard_last.clone() {
                None => {
                    state.status_message = Some("Clipboard is empty".to_string());
                }
                Some(value) => {
                    if state.cursor.is_key_col() {
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
            match state.clipboard_last.clone() {
                None => {
                    state.status_message = Some("Clipboard is empty".to_string());
                }
                Some(value) => {
                    if state.cursor.is_key_col() {
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
            match state.clipboard_last.clone() {
                None => {
                    state.status_message = Some("Clipboard is empty".to_string());
                }
                Some(value) => {
                    if state.cursor.is_key_col() {
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
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(1));
            state.clamp_scroll();
        }
        (Mode::Pasting, Message::MoveCursorDown) => {
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 1).min(max));
            state.clamp_scroll();
        }
        (Mode::Pasting, Message::PasteNavLeft) => {
            if state.paste_locale_cursor > 0 {
                state.paste_locale_cursor -= 1;
            }
        }
        (Mode::Pasting, Message::PasteNavRight) => {
            let n = state.paste_locales().len();
            if state.paste_locale_cursor + 1 < n {
                state.paste_locale_cursor += 1;
            }
        }
        (Mode::Pasting, Message::PasteNavUp) => {
            let locale_keys = state.paste_locales();
            if let Some(locale) = locale_keys.into_iter().nth(state.paste_locale_cursor) {
                let pos = state.paste_history_pos.entry(locale).or_insert(0);
                if *pos > 0 {
                    *pos -= 1;
                }
            }
        }
        (Mode::Pasting, Message::PasteNavDown) => {
            let locale_keys = state.paste_locales();
            if let Some(locale) = locale_keys.into_iter().nth(state.paste_locale_cursor) {
                let history_len = state.clipboard.get(&locale).map(|v| v.len()).unwrap_or(0);
                let pos = state.paste_history_pos.entry(locale).or_insert(0);
                if *pos + 1 < history_len {
                    *pos += 1;
                }
            }
        }
        (Mode::Pasting, Message::RemovePasteEntry) => {
            let locale_keys = state.paste_locales();
            if let Some(locale) = locale_keys.into_iter().nth(state.paste_locale_cursor) {
                let pos = *state.paste_history_pos.get(&locale).unwrap_or(&0);
                if let Some(history) = state.clipboard.get_mut(&locale) {
                    if pos < history.len() {
                        history.remove(pos);
                        let new_len = history.len();
                        if new_len == 0 {
                            state.clipboard.remove(&locale);
                            state.paste_history_pos.remove(&locale);
                            let remaining = state.clipboard.len();
                            if remaining == 0 {
                                state.mode = Mode::Normal;
                                state.status_message = Some("Clipboard is empty".to_string());
                            } else if state.paste_locale_cursor >= remaining {
                                state.paste_locale_cursor = remaining - 1;
                            }
                        } else {
                            let p = state.paste_history_pos.entry(locale).or_insert(0);
                            if *p >= new_len {
                                *p = new_len - 1;
                            }
                        }
                    }
                }
            }
        }
        (Mode::Pasting, Message::CommitPasteStay) |
        (Mode::Pasting, Message::CommitPaste) => {
            // Paste all locales' selected history entries into the cursor row's key.
            let full_key = match state.cursor.full_key() {
                Some(k) if !state.cursor.is_bundle_header() => k,
                _ => {
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
                        let pos = *state.paste_history_pos.get(locale).unwrap_or(&0);
                        state.clipboard.get(locale).and_then(|h| h.get(pos)).cloned()
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
            state.cursor.bundle   = Some(bundle_name.clone());
            state.cursor.segments = vec![];
            state.cursor.locale   = None;
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
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(1));
            state.cursor.locale = None;
            state.clamp_scroll();
        }
        (Mode::BundleNaming, Message::MoveCursorDown) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 1).min(max));
            state.cursor.locale = None;
            state.clamp_scroll();
        }

        (Mode::Normal, Message::NewKey) => {
            let locale = state.cursor.locale.clone();

            // Bundle-level header, key column: open locale-naming to add a new locale file.
            if state.cursor.is_bundle_header() && locale.is_none() {
                state.edit_buffer = Some(CellEdit::new(String::new()));
                state.mode = Mode::LocaleNaming;
                return state;
            }

            // Build the pre-fill.  Locale-targeted: "bundle:locale:prefix." form.
            let bundle_str = state.cursor.bundle.as_deref().unwrap_or("").to_string();
            let segs = &state.cursor.segments;
            let pre = if state.cursor.is_bundle_header() {
                // Bundle header, locale column
                let locale_str = locale.as_deref().unwrap_or("default");
                format!("{bundle_str}:{locale_str}:")
            } else {
                // Entry or group-header row
                let real_key = segs.join(".");
                // Remove the last segment to get the key prefix ("parent.")
                let key_prefix = match real_key.rfind('.') {
                    Some(i) => format!("{}.", &real_key[..i]),
                    None    => String::new(),
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
                let (new_bundle, new_real) = workspace::split_key(&new_key);
                let new_bundle_opt = if new_bundle.is_empty() { None } else { Some(new_bundle.to_string()) };
                let new_segs: Vec<String> = new_real.split('.').map(|s| s.to_string()).collect();
                state.cursor.bundle   = new_bundle_opt;
                state.cursor.segments = new_segs;
                // Place cursor on the target locale column (3-segment) or first locale.
                let locale = target_locale.clone()
                    .or_else(|| state.visible_locales.first().cloned());
                state.cursor.locale = locale;
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
            let bundle = if state.cursor.is_bundle_header() {
                match &state.cursor.bundle {
                    Some(b) => b.clone(),
                    None => {
                        state.status_message = Some("No bundle selected".to_string());
                        return state;
                    }
                }
            } else {
                state.status_message = Some("No bundle selected".to_string());
                return state;
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
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(1));
            state.cursor.locale = None;
            state.clamp_scroll();
        }
        (Mode::LocaleNaming, Message::MoveCursorDown) => {
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 1).min(max));
            state.cursor.locale = None;
            state.clamp_scroll();
        }

        // ── KeyRenaming mode ─────────────────────────────────────────────────
        (Mode::KeyRenaming, Message::CommitKeyRename) => {
            let new_key = state.edit_buffer.as_ref()
                .map(|e| e.current_value().trim().to_string())
                .unwrap_or_default();

            let old_key = match state.cursor.full_key() {
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

            let old_key = match state.cursor.full_key() {
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
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(1));
            state.cursor.locale = None;
            state.clamp_scroll();
        }
        (
            Mode::Editing | Mode::KeyNaming | Mode::KeyRenaming | Mode::Deleting,
            Message::MoveCursorDown,
        ) => {
            state.temp_pins.clear();
            state.edit_buffer = None;
            state.mode = Mode::Normal;
            state.apply_filter();
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 1).min(max));
            state.cursor.locale = None;
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
            let cur = state.cursor_visual_row();
            state.set_visual_row(cur.saturating_sub(1));
            state.cursor.locale = None;
            state.clamp_scroll();
        }
        (Mode::Filter, Message::MoveCursorDown) => {
            state.mode = Mode::Normal;
            let cur = state.cursor_visual_row();
            let max = state.total_visual_rows().saturating_sub(1);
            state.set_visual_row((cur + 1).min(max));
            state.cursor.locale = None;
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

/// Yanks the value at the current cursor cell into the per-locale clipboard history.
/// Returns `Some(locale)` on success, `None` when on the key column or the cell is empty.
/// Updates `clipboard_last` and clamps `paste_history_pos` on success.
fn yank_cell(state: &mut AppState) -> Option<String> {
    let locale_idx = state.effective_locale_idx()?;
    let locale = state.visible_locales.get(locale_idx).cloned()?;
    let value  = state.current_cell_value()?;

    let history = state.clipboard.entry(locale.clone()).or_insert_with(Vec::new);
    history.retain(|v| v != &value);
    history.insert(0, value.clone());
    history.truncate(10);
    // Always point > to the freshly yanked entry so p/Enter use it immediately.
    state.paste_history_pos.insert(locale.clone(), 0);
    state.clipboard_last = Some(value);

    Some(locale)
}

