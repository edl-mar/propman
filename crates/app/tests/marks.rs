//! Specification of marking items of the overview by hand, of the selection that an action works
//! on, and of whether an action can be done (see `app::selection`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{
    AppState, ItemId, Message, OverviewItem, OverviewState, Session, View, applicable, overview,
    selection, update,
};
use common::{TempDir, fixtures};

fn state_for(folder: &std::path::Path) -> AppState {
    let (session, problems) = Session::open(folder);
    AppState::new(session, problems)
}

/// The folder `project`: the list has 18 items, among them the bundle `messages` (1) with the
/// files `messages.properties` (2), `_de` (3), `_fr` (4) and `_it` (5), the bundle `stripping`
/// (6) with its file (7), and the folder `sub_folder` (15).
fn project() -> AppState {
    send(
        state_for(&fixtures().join("project")),
        &[Message::Viewport(30)],
    )
}

fn empty() -> (AppState, TempDir) {
    let dir = TempDir::new();
    (state_for(dir.path()), dir)
}

fn send(state: AppState, messages: &[Message]) -> AppState {
    messages
        .iter()
        .fold(state, |state, &message| update(state, message).0)
}

fn ids(state: &AppState) -> Vec<ItemId> {
    overview(state.session())
        .iter()
        .map(OverviewItem::id)
        .collect()
}

fn view(state: &AppState) -> OverviewState {
    match state.view() {
        View::Overview(overview) => overview.clone(),
    }
}

/// The cursor on the item at this index.
fn at(state: AppState, index: usize) -> AppState {
    let mut state = send(state, &[Message::First]);
    for _ in 0..index {
        state = send(state, &[Message::Down]);
    }
    state
}

/// Marks the item at this index; the cursor stays there.
fn mark(state: AppState, index: usize) -> AppState {
    send(at(state, index), &[Message::ToggleMark])
}

// ── Marking ──────────────────────────────────────────────────────────────────

#[test]
fn nothing_is_marked_at_the_start() {
    assert!(view(&project()).marked().is_empty());
}

#[test]
fn the_message_marks_the_item_under_the_cursor_and_takes_the_mark_away_again() {
    let state = at(project(), 2);
    let id = ids(&state)[2].clone();

    let state = send(state, &[Message::ToggleMark]);
    assert_eq!(view(&state).marked(), std::slice::from_ref(&id));
    assert!(view(&state).is_marked(&id));

    let state = send(state, &[Message::ToggleMark]);
    assert!(view(&state).marked().is_empty());
    assert!(!view(&state).is_marked(&id));
}

#[test]
fn marking_does_not_move_the_cursor() {
    let state = mark(project(), 3);
    assert_eq!(view(&state).index(), 3);
}

#[test]
fn marks_stay_while_the_cursor_moves() {
    for message in [
        Message::Down,
        Message::Up,
        Message::First,
        Message::Last,
        Message::PageDown,
        Message::TogglePreview,
        Message::PreviewDown,
    ] {
        // a new session for every message: ids of different sessions are not the same ids
        let state = mark(mark(project(), 3), 5);
        let wanted = vec![ids(&state)[3].clone(), ids(&state)[5].clone()];
        let state = send(state, &[message]);
        let mut marked = view(&state).marked().to_vec();
        marked.sort_by_key(|id| ids(&state).iter().position(|other| other == id));
        assert_eq!(marked, wanted, "{message:?}");
    }
}

#[test]
fn marks_are_of_one_kind_and_marking_another_kind_starts_a_new_set() {
    let state = mark(mark(project(), 3), 4);
    assert_eq!(view(&state).marked().len(), 2, "two files");

    let state = mark(state, 1);
    let bundle = ids(&state)[1].clone();
    assert_eq!(
        view(&state).marked(),
        [bundle],
        "the bundle replaces the files"
    );
}

#[test]
fn clearing_takes_all_marks_away_and_does_nothing_without_marks() {
    let state = send(mark(mark(project(), 3), 4), &[Message::ClearSelection]);
    assert!(view(&state).marked().is_empty());
    let state = send(state, &[Message::ClearSelection]);
    assert!(view(&state).marked().is_empty());
}

#[test]
fn nothing_can_be_marked_in_an_empty_list() {
    let (state, _dir) = empty();
    let state = send(state, &[Message::Viewport(10), Message::ToggleMark]);
    assert!(view(&state).marked().is_empty());
}

// ── The selection ────────────────────────────────────────────────────────────

#[test]
fn without_marks_the_selection_is_the_item_under_the_cursor() {
    let state = at(project(), 4);
    assert_eq!(selection(&state), [ids(&state)[4].clone()]);
}

#[test]
fn marked_items_are_the_selection_in_the_order_of_the_list_and_beat_the_cursor() {
    let state = mark(mark(project(), 4), 2);
    let state = at(state, 7);
    assert_eq!(
        selection(&state),
        [ids(&state)[2].clone(), ids(&state)[4].clone()]
    );
}

#[test]
fn the_selection_of_an_empty_list_is_empty() {
    let (state, _dir) = empty();
    assert_eq!(selection(&state), []);
}

// ── What can be done ─────────────────────────────────────────────────────────

#[test]
fn editing_can_be_done_with_exactly_one_file_as_the_selection() {
    let can = |state: &AppState| applicable(state, Message::Edit);

    assert!(can(&at(project(), 2)), "a file under the cursor");
    assert!(
        can(&at(mark(project(), 3), 1)),
        "one file marked, the cursor is on a bundle"
    );
    assert!(!can(&at(project(), 0)), "a folder");
    assert!(!can(&at(project(), 1)), "a bundle");
    assert!(!can(&at(mark(mark(project(), 3), 4), 3)), "two files");
    assert!(
        !can(&at(mark(project(), 1), 2)),
        "a bundle is marked, the cursor is on a file"
    );
    let (empty, _dir) = empty();
    assert!(!can(&empty), "nothing to edit");
}

#[test]
fn marking_needs_an_item_and_clearing_needs_a_mark() {
    assert!(applicable(&project(), Message::ToggleMark));
    assert!(!applicable(&project(), Message::ClearSelection));
    assert!(applicable(&mark(project(), 2), Message::ClearSelection));

    let (empty, _dir) = empty();
    assert!(!applicable(&empty, Message::ToggleMark));
}

#[test]
fn everything_else_can_always_be_done() {
    let (empty, _dir) = empty();
    for message in [
        Message::Quit,
        Message::Up,
        Message::Down,
        Message::PageUp,
        Message::PageDown,
        Message::First,
        Message::Last,
        Message::Viewport(5),
        Message::TogglePreview,
        Message::PreviewUp,
        Message::PreviewDown,
        Message::PreviewHalfPageUp,
        Message::PreviewHalfPageDown,
        Message::DismissAlert,
    ] {
        assert!(applicable(&empty, message), "{message:?}");
    }
}
