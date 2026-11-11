//! Specification of the command line as an overlay: opening and closing it, editing its text,
//! choosing a candidate and running it, and that it takes the keys of the view while it is open.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{
    AppState, EditorOutcome, Effect, Message, OverviewState, Session, TextEdit, View, candidates,
    update,
};
use common::TempDir;

/// A folder with `a.properties`; the list is: folder, bundle `a`, the file.
fn one_file() -> (AppState, TempDir) {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a.properties"), b"a=1\n").unwrap();
    let (session, problems) = Session::open(dir.path());
    (AppState::new(session, problems), dir)
}

fn send(state: AppState, messages: &[Message]) -> AppState {
    messages
        .iter()
        .fold(state, |state, &message| update(state, message).0)
}

/// The cursor is on the file.
fn on_the_file() -> (AppState, TempDir) {
    let (state, dir) = one_file();
    let state = send(
        state,
        &[Message::Viewport(10), Message::Down, Message::Down],
    );
    (state, dir)
}

fn type_text(state: AppState, input: &str) -> AppState {
    input.chars().fold(state, |state, c| {
        send(state, &[Message::Text(TextEdit::Insert(c))])
    })
}

fn open_with(state: AppState, input: &str) -> AppState {
    type_text(send(state, &[Message::OpenCommandLine]), input)
}

fn text(state: &AppState) -> String {
    state.command_line().unwrap().field().text().to_owned()
}

fn highlighted(state: &AppState) -> usize {
    state.command_line().unwrap().highlighted()
}

fn overview_state(state: &AppState) -> OverviewState {
    match state.view() {
        View::Overview(overview) => overview.clone(),
    }
}

// ── Opening and closing ──────────────────────────────────────────────────────

#[test]
fn the_command_line_is_closed_at_the_start() {
    let (state, _dir) = one_file();
    assert!(state.command_line().is_none());
}

#[test]
fn opening_gives_an_empty_line_with_the_first_candidate_highlighted() {
    let (state, _dir) = on_the_file();
    let state = send(state, &[Message::OpenCommandLine]);
    assert_eq!(text(&state), "");
    assert_eq!(state.command_line().unwrap().field().cursor(), 0);
    assert_eq!(highlighted(&state), 0);
    assert!(!candidates(&state).is_empty());
}

#[test]
fn closing_closes_it_and_changes_nothing_else() {
    let (state, _dir) = on_the_file();
    let before = overview_state(&state);
    let state = send(open_with(state, "ed"), &[Message::CloseCommandLine]);
    assert!(state.command_line().is_none());
    assert_eq!(overview_state(&state), before);
    assert_eq!(state.alert(), None);
}

#[test]
fn opening_again_starts_with_an_empty_line() {
    let (state, _dir) = on_the_file();
    let state = send(open_with(state, "ed"), &[Message::CloseCommandLine]);
    let state = send(state, &[Message::OpenCommandLine]);
    assert_eq!(text(&state), "");
}

#[test]
fn closing_a_closed_line_and_editing_without_a_line_do_nothing() {
    let (state, _dir) = on_the_file();
    let before = overview_state(&state);
    let state = send(
        state,
        &[
            Message::CloseCommandLine,
            Message::Text(TextEdit::Insert('x')),
            Message::NextCandidate,
            Message::PreviousCandidate,
            Message::CompleteCommand,
            Message::RunCommand,
        ],
    );
    assert!(state.command_line().is_none());
    assert_eq!(overview_state(&state), before);
    assert!(!state.should_quit());
}

// ── Typing ───────────────────────────────────────────────────────────────────

#[test]
fn typing_edits_the_text_and_the_first_candidate_is_highlighted_again() {
    let (state, _dir) = on_the_file();
    let state = open_with(state, "e");
    let state = send(state, &[Message::NextCandidate, Message::NextCandidate]);
    assert_eq!(highlighted(&state), 2);

    let state = type_text(state, "d");
    assert_eq!(text(&state), "ed");
    assert_eq!(highlighted(&state), 0);
}

#[test]
fn the_keys_of_a_text_field_work_in_the_command_line() {
    let (state, _dir) = on_the_file();
    let state = open_with(state, "edit");
    let state = send(state, &[Message::Text(TextEdit::Backspace)]);
    assert_eq!(text(&state), "edi");
    let state = send(
        state,
        &[
            Message::Text(TextEdit::Home),
            Message::Text(TextEdit::Delete),
        ],
    );
    assert_eq!(text(&state), "di");
    let state = send(state, &[Message::Text(TextEdit::Clear)]);
    assert_eq!(text(&state), "");
}

// ── Choosing ─────────────────────────────────────────────────────────────────

#[test]
fn the_highlight_moves_through_the_candidates_and_stops_at_the_ends() {
    let (state, _dir) = on_the_file();
    let state = send(state, &[Message::OpenCommandLine]);
    let last = candidates(&state).len() - 1;

    let state = send(state, &[Message::PreviousCandidate]);
    assert_eq!(highlighted(&state), 0, "not before the first");

    let state = send(state, &[Message::NextCandidate]);
    assert_eq!(highlighted(&state), 1);

    let mut state = state;
    for _ in 0..100 {
        state = send(state, &[Message::NextCandidate]);
    }
    assert_eq!(highlighted(&state), last, "not after the last");

    let state = send(state, &[Message::PreviousCandidate]);
    assert_eq!(highlighted(&state), last - 1);
}

#[test]
fn completing_writes_the_name_of_the_highlighted_command_into_the_line() {
    let (state, _dir) = on_the_file();
    let state = send(open_with(state, "ed"), &[Message::CompleteCommand]);
    assert_eq!(text(&state), "edit");
    assert_eq!(state.command_line().unwrap().field().cursor(), 4);
    assert_eq!(highlighted(&state), 0);

    let (state, _dir) = on_the_file();
    let state = send(
        open_with(state, "ed"),
        &[Message::NextCandidate, Message::CompleteCommand],
    );
    assert_eq!(text(&state), "preview-down");
}

#[test]
fn completing_without_a_candidate_changes_nothing() {
    let (state, _dir) = on_the_file();
    let state = send(open_with(state, "xyz"), &[Message::CompleteCommand]);
    assert_eq!(text(&state), "xyz");
}

// ── Running ──────────────────────────────────────────────────────────────────

#[test]
fn running_closes_the_line_and_does_the_highlighted_command() {
    let (state, _dir) = on_the_file();
    let state = send(open_with(state, "mark"), &[Message::RunCommand]);
    assert!(state.command_line().is_none());
    assert_eq!(overview_state(&state).marked().len(), 1);
}

#[test]
fn a_command_that_asks_for_an_effect_does_so() {
    let (state, dir) = on_the_file();
    let file = state.session().store().files()[0];
    let state = open_with(state, "ed");
    let (state, effect) = update(state, Message::RunCommand);
    assert!(state.command_line().is_none());
    assert_eq!(
        effect,
        Some(Effect::Edit {
            file,
            path: dir.path().join("a.properties")
        })
    );
}

#[test]
fn quit_can_be_typed() {
    let (state, _dir) = on_the_file();
    let state = send(open_with(state, "quit"), &[Message::RunCommand]);
    assert!(state.should_quit());
}

#[test]
fn the_highlighted_command_is_run_not_the_first() {
    let (state, _dir) = on_the_file();
    // `ed` lists `edit` first and `preview-down` second; the second is chosen
    let state = send(
        open_with(state, "ed"),
        &[Message::NextCandidate, Message::RunCommand],
    );
    assert!(state.command_line().is_none());
    // `edit` was not run: it would have asked for an effect, and nothing is marked either
    assert!(overview_state(&state).marked().is_empty());
}

#[test]
fn running_without_a_candidate_leaves_the_line_open() {
    let (state, _dir) = on_the_file();
    let (state, effect) = update(open_with(state, "xyz"), Message::RunCommand);
    assert_eq!(effect, None);
    assert_eq!(text(&state), "xyz");
}

// ── The line takes the keys of the view ──────────────────────────────────────

fn looks_like(state: &AppState) -> (usize, usize, bool, usize, Option<String>) {
    let overview = overview_state(state);
    (
        overview.index(),
        overview.marked().len(),
        overview.preview_on(),
        overview.preview_scroll(),
        state
            .command_line()
            .map(|line| line.field().text().to_owned()),
    )
}

#[test]
fn while_the_line_is_open_the_keys_of_the_view_do_nothing() {
    for message in [
        Message::Up,
        Message::Down,
        Message::PageDown,
        Message::First,
        Message::Last,
        Message::TogglePreview,
        Message::PreviewDown,
        Message::PreviewHalfPageDown,
        Message::ToggleMark,
        Message::ClearMarks,
        Message::Edit,
        Message::OpenCommandLine,
    ] {
        let (state, _dir) = on_the_file();
        let state = open_with(state, "ed");
        let before = looks_like(&state);
        let (state, effect) = update(state, message);
        assert_eq!(effect, None, "{message:?}");
        assert_eq!(looks_like(&state), before, "{message:?}");
    }
}

#[test]
fn quitting_and_the_size_of_the_screen_still_get_through() {
    let (state, _dir) = on_the_file();
    let state = send(open_with(state, "ed"), &[Message::Viewport(20)]);
    assert_eq!(overview_state(&state).height(), 20);
    assert!(state.command_line().is_some());

    let state = send(state, &[Message::Quit]);
    assert!(state.should_quit());
}

#[test]
fn the_results_of_an_editor_get_through_and_an_alert_is_in_front_of_the_line() {
    let (state, _dir) = on_the_file();
    let file = state.session().store().files()[0];
    let state = open_with(state, "ed");
    let state = send(
        state,
        &[Message::EditorClosed {
            file,
            outcome: EditorOutcome::NoEditor,
        }],
    );
    assert!(state.alert().is_some());

    // the alert is dismissed first; the line is still there
    let state = send(state, &[Message::DismissAlert]);
    assert_eq!(state.alert(), None);
    assert_eq!(text(&state), "ed");
}

#[test]
fn an_alert_does_not_let_the_line_open() {
    let (state, _dir) = on_the_file();
    let file = state.session().store().files()[0];
    let state = send(
        state,
        &[Message::EditorClosed {
            file,
            outcome: EditorOutcome::NoEditor,
        }],
    );
    let state = send(state, &[Message::OpenCommandLine]);
    assert!(state.command_line().is_none());
}
