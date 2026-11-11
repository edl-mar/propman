//! Specification of the commands: the table, and the candidates for what is typed (see
//! `app::commands`): which fit, in which order, and which letters were hit.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashSet;

use app::{AppState, Message, Session, TextEdit, candidates, commands, update};
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

/// The file is marked and under the cursor, so that every command can be done.
fn everything_possible() -> (AppState, TempDir) {
    let (state, dir) = one_file();
    let state = send(
        state,
        &[
            Message::Viewport(10),
            Message::Down,
            Message::Down,
            Message::ToggleMark,
        ],
    );
    (state, dir)
}

/// The cursor is on the folder: it cannot be edited, and nothing is marked.
fn on_the_folder() -> (AppState, TempDir) {
    let (state, dir) = one_file();
    (send(state, &[Message::Viewport(10)]), dir)
}

/// The command line is opened and this is typed.
fn typed(state: AppState, input: &str) -> AppState {
    let mut state = send(state, &[Message::OpenCommandLine]);
    for c in input.chars() {
        state = send(state, &[Message::Text(TextEdit::Insert(c))]);
    }
    state
}

fn names(state: &AppState) -> Vec<&'static str> {
    candidates(state)
        .iter()
        .map(|candidate| candidate.command.name)
        .collect()
}

fn matched(state: &AppState) -> Vec<(&'static str, Vec<usize>)> {
    candidates(state)
        .into_iter()
        .map(|candidate| (candidate.command.name, candidate.matched))
        .collect()
}

// ── The table ────────────────────────────────────────────────────────────────

#[test]
fn the_commands_are_these_in_this_order() {
    let names: Vec<&str> = commands().iter().map(|command| command.name).collect();
    assert_eq!(
        names,
        [
            "quit",
            "edit",
            "mark",
            "clear-marks",
            "preview",
            "preview-up",
            "preview-down",
            "preview-half-up",
            "preview-half-down",
            "up",
            "down",
            "page-up",
            "page-down",
            "first",
            "last",
        ]
    );
}

#[test]
fn every_command_has_its_own_name_label_and_message() {
    let commands = commands();
    let names: HashSet<&str> = commands.iter().map(|command| command.name).collect();
    assert_eq!(names.len(), commands.len(), "the names differ");
    for command in commands {
        assert!(!command.label.is_empty(), "{}", command.name);
        assert!(
            command
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-'),
            "{}: lower case letters and hyphens",
            command.name
        );
    }
    for (index, command) in commands.iter().enumerate() {
        for other in &commands[index + 1..] {
            assert_ne!(command.message, other.message, "{}", command.name);
        }
    }
}

#[test]
fn the_commands_say_what_the_message_does() {
    let by_name = |name: &str| {
        commands()
            .iter()
            .find(|command| command.name == name)
            .unwrap()
            .message
    };
    assert_eq!(by_name("quit"), Message::Quit);
    assert_eq!(by_name("edit"), Message::Edit);
    assert_eq!(by_name("mark"), Message::ToggleMark);
    assert_eq!(by_name("clear-marks"), Message::ClearMarks);
    assert_eq!(by_name("preview"), Message::TogglePreview);
    assert_eq!(by_name("preview-half-down"), Message::PreviewHalfPageDown);
    assert_eq!(by_name("page-up"), Message::PageUp);
    assert_eq!(by_name("last"), Message::Last);
}

// ── The candidates ───────────────────────────────────────────────────────────

#[test]
fn there_are_no_candidates_while_the_command_line_is_closed() {
    let (state, _dir) = everything_possible();
    assert_eq!(names(&state), Vec::<&str>::new());
}

#[test]
fn without_input_every_command_that_can_be_done_is_a_candidate_in_the_order_of_the_table() {
    let (state, _dir) = everything_possible();
    let state = typed(state, "");
    let all: Vec<&str> = commands().iter().map(|command| command.name).collect();
    assert_eq!(names(&state), all);
    assert!(
        candidates(&state)
            .iter()
            .all(|candidate| candidate.matched.is_empty())
    );
}

#[test]
fn only_commands_that_can_be_done_are_candidates() {
    let (state, _dir) = on_the_folder();
    let state = typed(state, "");
    let listed = names(&state);
    assert!(!listed.contains(&"edit"), "a folder cannot be edited");
    assert!(!listed.contains(&"clear-marks"), "nothing is marked");
    assert!(listed.contains(&"mark"));
    assert!(listed.contains(&"quit"));

    let (state, _dir) = on_the_folder();
    let state = typed(state, "ed");
    assert_eq!(
        names(&state),
        ["preview-down", "preview-half-down", "page-down"]
    );
}

#[test]
fn a_name_that_begins_with_the_input_comes_first() {
    let (state, _dir) = everything_possible();
    assert_eq!(names(&typed(state, "q")), ["quit"]);

    let (state, _dir) = everything_possible();
    assert_eq!(
        names(&typed(state, "pre")),
        [
            "preview",
            "preview-up",
            "preview-down",
            "preview-half-up",
            "preview-half-down",
        ]
    );
}

#[test]
fn then_come_the_names_that_have_the_input_as_a_subsequence() {
    let (state, _dir) = everything_possible();
    assert_eq!(
        names(&typed(state, "ed")),
        ["edit", "preview-down", "preview-half-down", "page-down"]
    );

    let (state, _dir) = everything_possible();
    assert_eq!(
        names(&typed(state, "up")),
        ["up", "preview-up", "preview-half-up", "page-up"]
    );
}

#[test]
fn the_beginnings_of_the_words_come_before_other_subsequences() {
    // `pd` is the beginnings of `preview-down` and `page-down`; words can be skipped
    let (state, _dir) = everything_possible();
    assert_eq!(
        names(&typed(state, "pd")),
        ["preview-down", "preview-half-down", "page-down"]
    );

    let (state, _dir) = everything_possible();
    assert_eq!(
        names(&typed(state, "pu")),
        ["preview-up", "preview-half-up", "page-up"]
    );

    let (state, _dir) = everything_possible();
    assert_eq!(names(&typed(state, "cm")), ["clear-marks"]);
}

#[test]
fn nothing_fits_what_is_not_a_subsequence() {
    let (state, _dir) = everything_possible();
    assert_eq!(names(&typed(state, "xyz")), Vec::<&str>::new());
}

#[test]
fn the_case_of_the_input_does_not_matter() {
    let (state, _dir) = everything_possible();
    let lower = names(&typed(state, "ed"));
    let (state, _dir) = everything_possible();
    assert_eq!(names(&typed(state, "ED")), lower);
    let (state, _dir) = everything_possible();
    assert_eq!(names(&typed(state, "Quit")), ["quit"]);
}

#[test]
fn the_places_of_the_letters_that_were_hit_are_given() {
    let (state, _dir) = everything_possible();
    assert_eq!(
        matched(&typed(state, "ed")),
        [
            ("edit", vec![0, 1]),
            ("preview-down", vec![2, 8]),
            ("preview-half-down", vec![2, 13]),
            ("page-down", vec![3, 5]),
        ]
    );

    let (state, _dir) = everything_possible();
    assert_eq!(
        matched(&typed(state, "pd")),
        [
            ("preview-down", vec![0, 8]),
            ("preview-half-down", vec![0, 13]),
            ("page-down", vec![0, 5]),
        ]
    );

    let (state, _dir) = everything_possible();
    assert_eq!(matched(&typed(state, "pre"))[0], ("preview", vec![0, 1, 2]));
}
