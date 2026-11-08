//! Specification of the state of the user interface and its `update` (see `app::AppState`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{AppState, Message, Session, update};
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
fn quit_asks_to_quit() {
    let state = update(state(), Message::Quit);
    assert!(state.should_quit());
}

#[test]
fn update_keeps_the_session_and_the_problems() {
    let dir = TempDir::new();
    let (session, problems) = Session::open(&dir.path().join("does_not_exist"));
    let state = AppState::new(session, problems);

    let state = update(state, Message::Quit);

    assert_eq!(state.problems().len(), 1);
    assert!(state.session().store().files().is_empty());
}

#[test]
fn quitting_twice_is_the_same_as_quitting_once() {
    let state = update(update(state(), Message::Quit), Message::Quit);
    assert!(state.should_quit());
    assert_eq!(state.session().store().files().len(), 12);
}
