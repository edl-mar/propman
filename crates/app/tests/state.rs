//! Specification of the state of the user interface and its `update` (see `app::AppState`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{AppState, Message, OverviewState, Session, View, update};
use common::{TempDir, fixtures};

fn state() -> AppState {
    let (session, problems) = Session::open(&fixtures().join("project"));
    AppState::new(session, problems)
}

#[test]
fn a_new_state_does_not_ask_to_quit() {
    assert!(!state().should_quit());
}

#[test]
fn the_state_gives_access_to_the_session_and_the_problems() {
    let state = state();
    assert_eq!(state.session().store().files().len(), 12);
    assert!(state.problems().is_empty());
}

#[test]
fn problems_from_loading_are_kept_in_the_state() {
    let dir = TempDir::new();
    let (session, problems) = Session::open(&dir.path().join("does_not_exist"));
    let state = AppState::new(session, problems);
    assert_eq!(state.problems().len(), 1);
}

#[test]
fn moving_and_scrolling_ask_for_no_effect() {
    for message in [
        Message::Up,
        Message::Down,
        Message::PageUp,
        Message::PageDown,
        Message::First,
        Message::Last,
        Message::Viewport(10),
        Message::TogglePreview,
        Message::PreviewUp,
        Message::PreviewDown,
        Message::PreviewHalfPageUp,
        Message::PreviewHalfPageDown,
    ] {
        assert_eq!(update(state(), message).1, None, "{message:?}");
    }
}

#[test]
fn quit_asks_to_quit() {
    let (state, effect) = update(state(), Message::Quit);
    assert_eq!(effect, None);
    assert!(state.should_quit());
}

#[test]
fn update_keeps_the_session_and_the_problems() {
    let dir = TempDir::new();
    let (session, problems) = Session::open(&dir.path().join("does_not_exist"));
    let state = AppState::new(session, problems);

    let (state, _) = update(state, Message::Quit);

    assert_eq!(state.problems().len(), 1);
    assert!(state.session().store().files().is_empty());
}

#[test]
fn quitting_twice_is_the_same_as_quitting_once() {
    let state = update(update(state(), Message::Quit).0, Message::Quit).0;
    assert!(state.should_quit());
    assert_eq!(state.session().store().files().len(), 12);
}

// ── The selection and the scroll position of the overview ────────────────────

/// The overview of the fixtures has 18 items: 2 folders, 4 bundles and 12 files.
const ITEMS: usize = 18;

fn overview_state(state: &AppState) -> OverviewState {
    match state.view() {
        View::Overview(overview) => overview.clone(),
    }
}

fn press(state: AppState, messages: &[Message]) -> AppState {
    messages
        .iter()
        .fold(state, |state, &message| update(state, message).0)
}

fn empty_state() -> (AppState, TempDir) {
    let dir = TempDir::new();
    let (session, problems) = Session::open(dir.path());
    (AppState::new(session, problems), dir)
}

#[test]
fn the_overview_starts_at_the_first_item_with_an_unknown_height() {
    let overview = overview_state(&state());
    assert_eq!(
        (overview.selected(), overview.scroll(), overview.height()),
        (0, 0, 0)
    );
}

#[test]
fn down_moves_the_selection_and_up_moves_it_back() {
    let state = press(state(), &[Message::Down, Message::Down, Message::Down]);
    assert_eq!(overview_state(&state).selected(), 3);
    let state = press(state, &[Message::Up]);
    assert_eq!(overview_state(&state).selected(), 2);
}

#[test]
fn the_selection_stops_at_the_ends() {
    let state = press(state(), &[Message::Up]);
    assert_eq!(overview_state(&state).selected(), 0);

    let downs = vec![Message::Down; ITEMS + 10];
    let state = press(state, &downs);
    assert_eq!(overview_state(&state).selected(), ITEMS - 1);
}

#[test]
fn first_and_last_jump_to_the_ends() {
    let state = press(state(), &[Message::Last]);
    assert_eq!(overview_state(&state).selected(), ITEMS - 1);
    let state = press(state, &[Message::First]);
    assert_eq!(overview_state(&state).selected(), 0);
}

#[test]
fn a_page_is_as_many_items_as_fit() {
    let state = press(state(), &[Message::Viewport(5)]);
    let state = press(state, &[Message::PageDown]);
    assert_eq!(overview_state(&state).selected(), 5);
    let state = press(state, &[Message::PageDown]);
    assert_eq!(overview_state(&state).selected(), 10);
    let state = press(state, &[Message::PageUp]);
    assert_eq!(overview_state(&state).selected(), 5);
}

#[test]
fn a_page_stops_at_the_ends_too() {
    let state = press(state(), &[Message::Viewport(5)]);
    let state = press(state, &[Message::PageUp]);
    assert_eq!(overview_state(&state).selected(), 0);

    let pages = vec![Message::PageDown; 10];
    let state = press(state, &pages);
    assert_eq!(overview_state(&state).selected(), ITEMS - 1);
}

#[test]
fn without_a_known_height_a_page_is_one_item() {
    let state = press(state(), &[Message::PageDown]);
    assert_eq!(overview_state(&state).selected(), 1);
}

#[test]
fn the_scroll_position_only_moves_when_the_selection_leaves_the_visible_part() {
    let mut state = press(state(), &[Message::Viewport(5)]);
    // selection 0..=4 are visible without scrolling
    for expected in 1..=4 {
        state = press(state, &[Message::Down]);
        let overview = overview_state(&state);
        assert_eq!((overview.selected(), overview.scroll()), (expected, 0));
    }
    // beyond the last visible item the list scrolls by one
    state = press(state, &[Message::Down]);
    let overview = overview_state(&state);
    assert_eq!((overview.selected(), overview.scroll()), (5, 1));
}

#[test]
fn moving_up_scrolls_back_only_above_the_visible_part() {
    let mut state = press(state(), &[Message::Viewport(5), Message::Last]);
    assert_eq!(overview_state(&state).scroll(), ITEMS - 5);

    // moving up inside the visible part does not scroll
    for _ in 0..4 {
        state = press(state, &[Message::Up]);
    }
    assert_eq!(overview_state(&state).scroll(), ITEMS - 5);

    // above the visible part it scrolls by one
    state = press(state, &[Message::Up]);
    let overview = overview_state(&state);
    assert_eq!(
        (overview.selected(), overview.scroll()),
        (ITEMS - 6, ITEMS - 6)
    );
}

#[test]
fn first_and_last_scroll_to_the_ends() {
    let state = press(state(), &[Message::Viewport(5), Message::Last]);
    assert_eq!(overview_state(&state).scroll(), ITEMS - 5);
    let state = press(state, &[Message::First]);
    assert_eq!(overview_state(&state).scroll(), 0);
}

#[test]
fn a_smaller_height_keeps_the_selection_visible() {
    let state = press(state(), &[Message::Viewport(10), Message::Last]);
    assert_eq!(overview_state(&state).scroll(), ITEMS - 10);

    let state = press(state, &[Message::Viewport(3)]);
    let overview = overview_state(&state);
    assert_eq!(overview.height(), 3);
    assert_eq!(overview.scroll(), ITEMS - 3);
    assert_eq!(overview.selected(), ITEMS - 1);
}

#[test]
fn a_height_larger_than_the_list_scrolls_back_to_the_top() {
    let state = press(state(), &[Message::Viewport(5), Message::Last]);
    let state = press(state, &[Message::Viewport(ITEMS + 10)]);
    assert_eq!(overview_state(&state).scroll(), 0);
}

#[test]
fn an_empty_overview_ignores_every_movement() {
    let (state, _dir) = empty_state();
    let state = press(
        state,
        &[
            Message::Viewport(5),
            Message::Down,
            Message::Up,
            Message::PageDown,
            Message::PageUp,
            Message::Last,
            Message::First,
        ],
    );
    let overview = overview_state(&state);
    assert_eq!((overview.selected(), overview.scroll()), (0, 0));
    assert!(!state.should_quit());
}

#[test]
fn moving_does_not_quit_and_does_not_touch_the_session() {
    let state = press(
        state(),
        &[Message::Viewport(5), Message::Down, Message::Last],
    );
    assert!(!state.should_quit());
    assert_eq!(state.session().store().files().len(), 12);
}
