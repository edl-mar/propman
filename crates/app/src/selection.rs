//! What an action works on, and whether an action can be done at all.
//!
//! The selection is the marked items of the overview, in the order of the list, and if none is
//! marked, the item under the cursor. An action has a precondition on it: editing a file needs a
//! selection of exactly one file. `applicable` says whether the precondition holds, so that the
//! keys of an action that cannot be done are not offered.

use store::{BundleId, FileId};

use crate::commands::{Action, Entry, candidates};
use crate::overview::{ItemId, OverviewItem, overview};
use crate::state::{AppState, Message, View};

/// The ids of the selection, in the order of the list; empty if the list is.
pub fn selection(state: &AppState) -> Vec<ItemId> {
    let View::Overview(view) = state.view();
    let ids: Vec<ItemId> = overview(state.session())
        .iter()
        .map(OverviewItem::id)
        .collect();
    let marked: Vec<ItemId> = ids
        .iter()
        .filter(|id| view.is_marked(id))
        .cloned()
        .collect();
    if marked.is_empty() {
        ids.get(view.index()).cloned().into_iter().collect()
    } else {
        marked
    }
}

/// Whether the message can be done now. Everything that is no action on a selection can.
pub fn applicable(state: &AppState, message: Message) -> bool {
    let View::Overview(view) = state.view();
    match message {
        Message::Edit => matches!(selection(state).as_slice(), [ItemId::File(_)]),
        Message::ToggleMark => !overview(state.session()).is_empty(),
        Message::ClearMarks => !view.marked().is_empty(),
        Message::SetEncoding(_) => selected_files(state).is_some(),
        Message::SetExcluded(_) => selected_bundles(state).is_some(),
        Message::PreviousSegment => state
            .command_line()
            .is_some_and(|line| line.chosen().is_some()),
        _ => true,
    }
}

/// The files of the selection, if it is files and nothing else.
pub(crate) fn selected_files(state: &AppState) -> Option<Vec<FileId>> {
    let ids = selection(state);
    let files: Vec<FileId> = ids
        .iter()
        .filter_map(|id| match id {
            ItemId::File(file) => Some(*file),
            _ => None,
        })
        .collect();
    (!files.is_empty() && files.len() == ids.len()).then_some(files)
}

/// The bundles of the selection, if it is bundles and nothing else.
pub(crate) fn selected_bundles(state: &AppState) -> Option<Vec<BundleId>> {
    let ids = selection(state);
    let bundles: Vec<BundleId> = ids
        .iter()
        .filter_map(|id| match id {
            ItemId::Bundle(bundle) => Some(*bundle),
            _ => None,
        })
        .collect();
    (!bundles.is_empty() && bundles.len() == ids.len()).then_some(bundles)
}

/// "1 file" or "3 files", for a kind of item.
fn counted(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

/// What the selection is, as text for the command line: the item under the cursor by kind and
/// name (`file messages.properties`), or the marked ones by number (`2 files (marked)`). `None`
/// for an empty list.
pub fn target(state: &AppState) -> Option<String> {
    let View::Overview(view) = state.view();
    let items = overview(state.session());
    let chosen = selection(state);
    let kind = |id: &ItemId| match id {
        ItemId::Folder(_) => ("folder", "folders"),
        ItemId::Bundle(_) => ("bundle", "bundles"),
        ItemId::File(_) => ("file", "files"),
    };
    match chosen.as_slice() {
        [] => None,
        [id] if view.marked().is_empty() => {
            let name = items
                .iter()
                .find(|item| item.id() == *id)
                .map(|item| match item {
                    OverviewItem::Folder { path, .. } => path.clone(),
                    OverviewItem::Bundle { name, .. } | OverviewItem::File { name, .. } => {
                        name.clone()
                    }
                })?;
            Some(format!("{} {name}", kind(id).0))
        }
        [first, ..] => {
            let (singular, plural) = kind(first);
            Some(format!(
                "{} (marked)",
                counted(chosen.len(), singular, plural)
            ))
        }
    }
}

/// What the highlighted value of the command line would do, as text: `encoding Windows-1252
/// changes 1 of 2 files`. `None` unless a command with values is chosen and a value is
/// highlighted.
pub fn summary(state: &AppState) -> Option<String> {
    let line = state.command_line()?;
    let command = line.chosen()?;
    if !matches!(command.action, Action::Choose(_)) {
        return None;
    }
    let found = candidates(state);
    let Entry::Value(value) = found.get(line.highlighted())?.entry else {
        return None;
    };
    let (changed, total, singular, plural) = match value.message {
        Message::SetEncoding(encoding) => {
            let files = selected_files(state)?;
            let changed = files
                .iter()
                .filter(|&&file| {
                    state
                        .session()
                        .store()
                        .file_encoding(file)
                        .is_some_and(|current| current != encoding)
                })
                .count();
            (changed, files.len(), "file", "files")
        }
        Message::SetExcluded(excluded) => {
            let bundles = selected_bundles(state)?;
            let changed = bundles
                .iter()
                .filter(|&&bundle| {
                    state
                        .session()
                        .store()
                        .bundle_excluded(bundle)
                        .is_some_and(|current| current != excluded)
                })
                .count();
            (changed, bundles.len(), "bundle", "bundles")
        }
        _ => return None,
    };
    Some(format!(
        "{} {} changes {changed} of {}",
        command.name,
        value.name,
        counted(total, singular, plural)
    ))
}
