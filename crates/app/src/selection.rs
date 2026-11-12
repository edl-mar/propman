//! What an action works on, and whether an action can be done at all.
//!
//! The selection is the files that a condition picks (what the files have: a locale, a bundle, an
//! encoding), or else the marked items of the overview, in the order of the list, or else the
//! item under the cursor. A condition and marks are never there together: marking makes a
//! condition a fixed set, and a condition narrows marks. A condition that nothing meets is an
//! empty selection and does not fall back to the cursor. An action has a precondition on the
//! selection: editing a file needs exactly one file. `applicable` says whether the precondition
//! holds, so that the keys of an action that cannot be done are not offered.
//!
//! In the command line the steps that are done already are not yet done: `target` and `summary`
//! count with them, and what could be done after them is offered.

use store::{BundleId, FileId};

use crate::commands::{Entry, Predicate, candidates, encoding_name};
use crate::overview::{ItemId, OverviewItem, overview};
use crate::session::Session;
use crate::state::{AppState, Message, View};

/// Whether this file has what the predicate asks for.
pub(crate) fn meets(session: &Session, file: FileId, predicate: Predicate) -> bool {
    let store = session.store();
    match predicate {
        Predicate::Locale(locale) => store
            .file_assignment(file)
            .is_some_and(|assignment| assignment.locale == locale),
        Predicate::Bundle(bundle) => store
            .file_assignment(file)
            .is_some_and(|assignment| assignment.bundle == bundle),
        Predicate::Encoding(encoding) => store.file_encoding(file) == Some(encoding),
    }
}

/// The files that have everything of the condition, in the order of the list.
pub(crate) fn matching_files(state: &AppState, condition: &[Predicate]) -> Vec<ItemId> {
    overview(state.session())
        .iter()
        .filter_map(|item| match item {
            OverviewItem::File { id, .. }
                if condition
                    .iter()
                    .all(|&predicate| meets(state.session(), *id, predicate)) =>
            {
                Some(ItemId::File(*id))
            }
            _ => None,
        })
        .collect()
}

/// The ids of the selection, in the order of the list; empty if the list is.
pub fn selection(state: &AppState) -> Vec<ItemId> {
    let View::Overview(view) = state.view();
    if !view.condition().is_empty() {
        return matching_files(state, view.condition());
    }
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

/// The predicates of the selection steps that are done in the command line but not yet run.
fn predicates_of_the_steps(state: &AppState) -> Vec<Predicate> {
    state
        .command_line()
        .map(|line| {
            line.steps()
                .iter()
                .filter_map(|step| match step.message {
                    Message::Select(predicate) => Some(predicate),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The condition as it will be when the steps of the command line are run, and this one as well.
fn condition_with_the_steps(state: &AppState, more: Option<Predicate>) -> Vec<Predicate> {
    let View::Overview(view) = state.view();
    let mut condition = view.condition().to_vec();
    condition.extend(predicates_of_the_steps(state));
    condition.extend(more);
    condition
}

/// The files that a condition picks, among the marked ones if there are any: a condition narrows
/// what is marked.
fn picked(state: &AppState, condition: &[Predicate]) -> Vec<ItemId> {
    let View::Overview(view) = state.view();
    let matching = matching_files(state, condition);
    if view.marked().is_empty() {
        matching
    } else {
        matching
            .into_iter()
            .filter(|id| view.is_marked(id))
            .collect()
    }
}

/// The selection as it will be when the steps of the command line are run.
fn selection_after_the_steps(state: &AppState) -> Vec<ItemId> {
    if predicates_of_the_steps(state).is_empty() {
        selection(state)
    } else {
        picked(state, &condition_with_the_steps(state, None))
    }
}

/// Whether the message can be done now. Everything that is no action on a selection can.
pub fn applicable(state: &AppState, message: Message) -> bool {
    let View::Overview(view) = state.view();
    match message {
        Message::Edit => matches!(selection(state).as_slice(), [ItemId::File(_)]),
        Message::ToggleMark => !overview(state.session()).is_empty(),
        Message::ClearSelection => !view.marked().is_empty() || !view.condition().is_empty(),
        Message::SetEncoding(_) => selected_files(state).is_some(),
        Message::SetExcluded(_) => selected_bundles(state).is_some(),
        Message::PreviousSegment => state
            .command_line()
            .is_some_and(|line| !line.segments().is_empty() || !line.steps().is_empty()),
        // a condition is for files: marks that are not files cannot be narrowed by it
        Message::Select(_) => view.marked().iter().all(|id| matches!(id, ItemId::File(_))),
        _ => true,
    }
}

/// Whether the message can be done in the command line, where the steps that are done already
/// are not yet done: after a selection step the selection will be files.
pub(crate) fn applicable_in_line(state: &AppState, message: Message) -> bool {
    let after_a_step = state
        .command_line()
        .is_some_and(|line| !line.steps().is_empty());
    if after_a_step {
        match message {
            Message::SetEncoding(_) | Message::Edit | Message::Select(_) => return true,
            Message::SetExcluded(_) => return false,
            _ => {}
        }
    }
    applicable(state, message)
}

/// The files among these ids, if it is files and nothing else.
fn only_files(ids: &[ItemId]) -> Option<Vec<FileId>> {
    let files: Vec<FileId> = ids
        .iter()
        .filter_map(|id| match id {
            ItemId::File(file) => Some(*file),
            _ => None,
        })
        .collect();
    (!files.is_empty() && files.len() == ids.len()).then_some(files)
}

/// The files of the selection, if it is files and nothing else.
pub(crate) fn selected_files(state: &AppState) -> Option<Vec<FileId>> {
    only_files(&selection(state))
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

/// A predicate as the user would type it: `locale de`, `locale (default)`, `bundle m`,
/// `encoding UTF-8`.
fn predicate_text(state: &AppState, predicate: Predicate) -> String {
    let store = state.session().store();
    match predicate {
        Predicate::Locale(None) => "locale (default)".to_owned(),
        Predicate::Locale(Some(locale)) => {
            format!("locale {}", store.text(locale).unwrap_or("?"))
        }
        Predicate::Bundle(bundle) => {
            format!("bundle {}", store.bundle_name(bundle).unwrap_or("?"))
        }
        Predicate::Encoding(encoding) => format!("encoding {}", encoding_name(encoding)),
    }
}

fn condition_text(state: &AppState, condition: &[Predicate]) -> String {
    condition
        .iter()
        .map(|&predicate| predicate_text(state, predicate))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What the selection is, as text for the command line: the item under the cursor by kind and
/// name (`file messages.properties`), the marked ones by number (`2 files (marked)`), or what a
/// condition picks (`3 files (locale de)`). `None` for an empty list.
pub fn target(state: &AppState) -> Option<String> {
    let View::Overview(view) = state.view();
    let items = overview(state.session());
    let chosen = selection_after_the_steps(state);
    let condition = condition_with_the_steps(state, None);
    if !condition.is_empty() && view.marked().is_empty() {
        return Some(format!(
            "{} ({})",
            counted(chosen.len(), "file", "files"),
            condition_text(state, &condition)
        ));
    }
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
/// changes 1 of 2 files`, or `sel locale de selects 2 files`. It counts with the steps that are
/// done in the line. `None` unless a value that completes the line is highlighted.
pub fn summary(state: &AppState) -> Option<String> {
    let line = state.command_line()?;
    if line.segments().is_empty() {
        return None;
    }
    let found = candidates(state);
    let candidate = found.get(line.highlighted())?;
    let Entry::Choice {
        complete: Some(message),
    } = candidate.entry
    else {
        return None;
    };
    let what = format!("{} {}", line.segments().join(" "), candidate.name);
    let (changed, total, singular, plural) = match message {
        Message::Select(predicate) => {
            let count = picked(state, &condition_with_the_steps(state, Some(predicate))).len();
            return Some(format!(
                "{what} selects {}",
                counted(count, "file", "files")
            ));
        }
        Message::SetEncoding(encoding) => {
            let files = only_files(&selection_after_the_steps(state))?;
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
        "{what} changes {changed} of {}",
        counted(total, singular, plural)
    ))
}

/// The items that are selected on purpose: the ones that a condition picks, or the marked ones.
/// Unlike `selection` this is empty if there is neither, and does not fall back to the item
/// under the cursor.
pub fn selected_items(state: &AppState) -> Vec<ItemId> {
    let View::Overview(view) = state.view();
    if !view.condition().is_empty() {
        return matching_files(state, view.condition());
    }
    overview(state.session())
        .iter()
        .map(OverviewItem::id)
        .filter(|id| view.is_marked(id))
        .collect()
}

/// What is selected on purpose, as text for the line with the count: `2 files marked`, or
/// `4 files selected: locale de` for a condition. `None` if nothing is.
pub fn selection_text(state: &AppState) -> Option<String> {
    let View::Overview(view) = state.view();
    if !view.condition().is_empty() {
        let count = matching_files(state, view.condition()).len();
        return Some(format!(
            "{} selected: {}",
            counted(count, "file", "files"),
            condition_text(state, view.condition())
        ));
    }
    let marked = view.marked();
    let (singular, plural) = match marked.first()? {
        ItemId::Folder(_) => ("folder", "folders"),
        ItemId::Bundle(_) => ("bundle", "bundles"),
        ItemId::File(_) => ("file", "files"),
    };
    Some(format!(
        "{} marked",
        counted(marked.len(), singular, plural)
    ))
}
