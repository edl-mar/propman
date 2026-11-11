//! What an action works on, and whether an action can be done at all.
//!
//! The selection is the marked items of the overview, in the order of the list, and if none is
//! marked, the item under the cursor. An action has a precondition on it: editing a file needs a
//! selection of exactly one file. `applicable` says whether the precondition holds, so that the
//! keys of an action that cannot be done are not offered.

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
        _ => true,
    }
}
