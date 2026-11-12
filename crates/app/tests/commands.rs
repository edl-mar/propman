//! Specification of the commands: the table, and the candidates for what is typed (see
//! `app::commands`): which fit, in which order, and which letters were hit.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashSet;

use app::{
    Action, AppState, Choice, Message, Session, TextEdit, Then, candidates, commands, summary,
    target, update,
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

/// The names live as long as the test run: it is shorter than that.
fn leaked(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

fn names(state: &AppState) -> Vec<&'static str> {
    candidates(state)
        .into_iter()
        .map(|candidate| leaked(candidate.name))
        .collect()
}

fn matched(state: &AppState) -> Vec<(&'static str, Vec<usize>)> {
    candidates(state)
        .into_iter()
        .map(|candidate| (leaked(candidate.name), candidate.matched))
        .collect()
}

/// The messages that the choices lead to, depth first.
fn leaf_messages(choices: &[Choice]) -> Vec<Message> {
    choices
        .iter()
        .flat_map(|choice| match &choice.then {
            Then::Run(message) => vec![*message],
            Then::Choose(more) => leaf_messages(more),
        })
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
            "sel",
            "clear-selection",
            "encoding",
            "excluded",
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
    // every message is the message of one command or one value
    let (state, _dir) = two_files();
    let mut messages: Vec<Message> = commands
        .iter()
        .flat_map(|command| match &command.action {
            Action::Message(message) => vec![*message],
            Action::Choose(make) => leaf_messages(&make(&state)),
        })
        .collect();
    let all = messages.len();
    messages.dedup_by(|a, b| a == b);
    assert_eq!(messages.len(), all, "no message twice (next to each other)");
    for (index, message) in messages.iter().enumerate() {
        assert!(!messages[index + 1..].contains(message), "{message:?}");
    }
}

#[test]
fn the_commands_say_what_the_message_does() {
    let by_name = |name: &str| match &commands()
        .iter()
        .find(|command| command.name == name)
        .unwrap()
        .action
    {
        Action::Message(message) => *message,
        Action::Choose(_) => panic!("{name} needs a value"),
    };
    assert_eq!(by_name("quit"), Message::Quit);
    assert_eq!(by_name("edit"), Message::Edit);
    assert_eq!(by_name("mark"), Message::ToggleMark);
    assert_eq!(by_name("clear-selection"), Message::ClearSelection);
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
    // everything but `excluded`: that is for bundles, and a file is under the cursor
    let all: Vec<&str> = commands()
        .iter()
        .map(|command| command.name)
        .filter(|&name| name != "excluded")
        .collect();
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
    assert!(!listed.contains(&"clear-selection"), "nothing is marked");
    assert!(!listed.contains(&"encoding"), "that is for files");
    assert!(!listed.contains(&"excluded"), "that is for bundles");
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
        [
            "edit",
            "encoding",
            "preview-down",
            "preview-half-down",
            "page-down"
        ]
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
    assert_eq!(names(&typed(state, "cs")), ["clear-selection"]);
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
            ("encoding", vec![0, 4]),
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

// ── Commands with values ─────────────────────────────────────────────────────

/// A folder with two bundles of one file each; the list is: folder, bundle `a`, file `a`,
/// bundle `b`, file `b`. Both files are UTF-8.
fn two_files() -> (AppState, TempDir) {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a.properties"), b"a=1\n").unwrap();
    std::fs::write(dir.path().join("b.properties"), b"b=1\n").unwrap();
    let (session, problems) = Session::open(dir.path());
    let state = send(AppState::new(session, problems), &[Message::Viewport(10)]);
    (state, dir)
}

fn down(state: AppState, times: usize) -> AppState {
    (0..times).fold(state, |state, _| send(state, &[Message::Down]))
}

/// The command with this name is chosen in the command line and this is typed for its value.
fn value_typed(state: AppState, command: &str, value: &str) -> AppState {
    let state = typed(state, command);
    let state = send(state, &[Message::CompleteCommand]);
    value.chars().fold(state, |state, c| {
        send(state, &[Message::Text(TextEdit::Insert(c))])
    })
}

#[test]
fn encoding_and_excluded_are_commands_that_need_a_value() {
    let (state, _dir) = two_files();
    let values_of = |name: &str| -> Vec<&'static str> {
        match &commands().iter().find(|c| c.name == name).unwrap().action {
            Action::Choose(make) => make(&state)
                .into_iter()
                .map(|choice| leaked(choice.name))
                .collect(),
            Action::Message(_) => panic!("{name} needs no value"),
        }
    };
    assert_eq!(
        values_of("encoding"),
        ["UTF-8", "UTF-8+BOM", "Windows-1252", "Latin-1"]
    );
    assert_eq!(values_of("excluded"), ["yes", "no"]);
}

#[test]
fn the_names_of_the_encodings_are_the_ones_that_are_shown() {
    use app::{Encoding, encoding_name};
    assert_eq!(encoding_name(Encoding::Utf8), "UTF-8");
    assert_eq!(encoding_name(Encoding::Utf8Bom), "UTF-8+BOM");
    assert_eq!(encoding_name(Encoding::Windows1252), "Windows-1252");
    assert_eq!(encoding_name(Encoding::Latin1), "Latin-1");
    let (state, _dir) = two_files();
    for command in commands() {
        if let Action::Choose(make) = &command.action {
            for choice in make(&state) {
                if let Then::Run(Message::SetEncoding(encoding)) = choice.then {
                    assert_eq!(choice.name, encoding_name(encoding));
                }
            }
        }
    }
}

#[test]
fn a_command_with_values_is_offered_for_the_kind_of_item_it_works_on() {
    let (state, _dir) = two_files();
    let on_a_folder = names(&typed(state, "enc"));
    assert_eq!(on_a_folder, Vec::<&str>::new(), "a folder has no encoding");

    let (state, _dir) = two_files();
    let on_a_bundle = typed(down(state, 1), "e");
    assert!(names(&on_a_bundle).contains(&"excluded"));
    assert!(!names(&on_a_bundle).contains(&"encoding"));

    let (state, _dir) = two_files();
    let on_a_file = typed(down(state, 2), "e");
    assert!(names(&on_a_file).contains(&"encoding"));
    assert!(!names(&on_a_file).contains(&"excluded"));
}

#[test]
fn after_a_command_with_values_the_values_are_the_candidates() {
    let (state, _dir) = two_files();
    let state = value_typed(down(state, 2), "enc", "");
    assert_eq!(
        names(&state),
        ["UTF-8", "UTF-8+BOM", "Windows-1252", "Latin-1"]
    );
}

#[test]
fn values_are_narrowed_like_commands() {
    let narrowed = |input: &str| {
        let (state, _dir) = two_files();
        names(&value_typed(down(state, 2), "enc", input))
    };
    assert_eq!(narrowed("w"), ["Windows-1252"]);
    assert_eq!(narrowed("u"), ["UTF-8", "UTF-8+BOM"]);
    assert_eq!(narrowed("1252"), ["Windows-1252"]);
    assert_eq!(narrowed("l"), ["Latin-1"]);
    assert_eq!(narrowed("8"), ["UTF-8", "UTF-8+BOM"]);
    assert_eq!(narrowed("BOM"), ["UTF-8+BOM"]);
    assert_eq!(narrowed("xyz"), Vec::<&str>::new());

    let (state, _dir) = two_files();
    let state = value_typed(down(state, 2), "enc", "w");
    assert_eq!(matched(&state), [("Windows-1252", vec![0])]);

    let (state, _dir) = two_files();
    let on_a_bundle = value_typed(down(state, 1), "excl", "y");
    assert_eq!(names(&on_a_bundle), ["yes"]);
}

// ── What the line says it works on and what it does ──────────────────────────

#[test]
fn the_target_is_the_item_under_the_cursor_or_the_marked_ones() {
    let (state, _dir) = two_files();
    assert_eq!(target(&state).unwrap(), "folder .");
    assert_eq!(target(&down(state, 1)).unwrap(), "bundle a");

    let (state, _dir) = two_files();
    assert_eq!(target(&down(state, 2)).unwrap(), "file a.properties");

    let (state, _dir) = two_files();
    let state = send(down(state, 2), &[Message::ToggleMark]);
    assert_eq!(target(&state).unwrap(), "1 file (marked)");
    let state = send(down(state, 2), &[Message::ToggleMark]);
    assert_eq!(target(&state).unwrap(), "2 files (marked)");
    // the marks win over the cursor
    let state = send(state, &[Message::First]);
    assert_eq!(target(&state).unwrap(), "2 files (marked)");

    let (state, _dir) = two_files();
    let state = send(down(state, 1), &[Message::ToggleMark]);
    assert_eq!(target(&state).unwrap(), "1 bundle (marked)");
}

#[test]
fn there_is_no_target_in_an_empty_list() {
    let dir = TempDir::new();
    let (session, problems) = Session::open(dir.path());
    let state = AppState::new(session, problems);
    assert_eq!(target(&state), None);
}

#[test]
fn the_summary_says_how_many_of_the_selection_would_change() {
    // both files are UTF-8
    let (state, _dir) = two_files();
    let state = value_typed(down(state, 2), "enc", "w");
    assert_eq!(
        summary(&state).unwrap(),
        "encoding Windows-1252 changes 1 of 1 file"
    );

    let (state, _dir) = two_files();
    let state = value_typed(down(state, 2), "enc", "u");
    assert_eq!(
        summary(&state).unwrap(),
        "encoding UTF-8 changes 0 of 1 file",
        "the highlighted value is the first, UTF-8"
    );

    // two marked files, both UTF-8
    let (state, _dir) = two_files();
    let state = send(
        down(state, 2),
        &[Message::ToggleMark, Message::Down, Message::Down],
    );
    let state = send(state, &[Message::ToggleMark]);
    let state = send(state, &[Message::OpenCommandLine]);
    let state = send(state, &[Message::Text(TextEdit::Insert('e'))]);
    let state = send(state, &[Message::CompleteCommand]);
    assert_eq!(
        summary(&state).unwrap(),
        "encoding UTF-8 changes 0 of 2 files"
    );

    let (state, _dir) = two_files();
    let state = value_typed(down(state, 1), "excl", "y");
    assert_eq!(
        summary(&state).unwrap(),
        "excluded yes changes 1 of 1 bundle"
    );
}

#[test]
fn there_is_no_summary_before_a_value_is_chosen_or_while_the_line_is_closed() {
    let (state, _dir) = two_files();
    assert_eq!(summary(&state), None, "closed");
    let state = typed(down(state, 2), "enc");
    assert_eq!(summary(&state), None, "the command is not chosen yet");
    let state = send(
        state,
        &[
            Message::CompleteCommand,
            Message::Text(TextEdit::Insert('x')),
        ],
    );
    assert_eq!(summary(&state), None, "no value fits");
}

// ── `sel`: the fields and what the files have ────────────────────────────────

/// The folder of the tests of the condition: the bundle `m` with `m`, `m_de` and `m_fr` (not
/// UTF-8) and the bundle `n` with `n` and `n_de`.
fn locale_project() -> (AppState, TempDir) {
    let dir = TempDir::new();
    for (name, bytes) in [
        ("m.properties", &b"a=1\n"[..]),
        ("m_de.properties", b"a=1\n"),
        ("m_fr.properties", b"a=\xe9\n"),
        ("n.properties", b"a=1\n"),
        ("n_de.properties", b"a=1\n"),
    ] {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let (session, problems) = Session::open(dir.path());
    let state = send(AppState::new(session, problems), &[Message::Viewport(20)]);
    (state, dir)
}

/// The line is open and these were chosen, each typed in full and taken with tab.
fn chosen(state: AppState, segments: &[&str]) -> AppState {
    let mut state = send(state, &[Message::OpenCommandLine]);
    for segment in segments {
        for c in segment.chars() {
            state = send(state, &[Message::Text(TextEdit::Insert(c))]);
        }
        state = send(state, &[Message::CompleteCommand]);
    }
    state
}

fn labels(state: &AppState) -> Vec<String> {
    candidates(state)
        .into_iter()
        .map(|candidate| candidate.label)
        .collect()
}

#[test]
fn sel_is_a_command_that_leads_to_the_fields_of_a_file() {
    let (state, _dir) = locale_project();
    assert!(names(&typed(state, "se")).contains(&"sel"));

    let (state, _dir) = locale_project();
    assert_eq!(
        names(&chosen(state, &["sel"])),
        ["locale", "bundle", "encoding"]
    );
}

#[test]
fn the_values_of_a_field_are_what_the_files_have() {
    let (state, _dir) = locale_project();
    let state = chosen(state, &["sel", "locale"]);
    assert_eq!(names(&state), ["(default)", "de", "fr"]);
    assert_eq!(labels(&state), ["2 files", "2 files", "1 file"]);

    let (state, _dir) = locale_project();
    let state = chosen(state, &["sel", "bundle"]);
    assert_eq!(names(&state), ["m", "n"]);
    assert_eq!(labels(&state), ["3 files", "2 files"]);

    // the encodings that there are: UTF-8 and the one of `m_fr`
    let (state, _dir) = locale_project();
    let other = {
        let store = state.session().store();
        let fr = store
            .files()
            .into_iter()
            .find(|&file| {
                store
                    .file_path(file)
                    .is_some_and(|path| path.ends_with("m_fr.properties"))
            })
            .unwrap();
        app::encoding_name(store.file_encoding(fr).unwrap())
    };
    let state = chosen(state, &["sel", "encoding"]);
    assert_eq!(names(&state), ["UTF-8", other]);
    assert_eq!(labels(&state), ["4 files", "1 file"]);
}

#[test]
fn the_values_of_a_field_are_narrowed_like_the_commands() {
    let narrowed = |input: &str| {
        let (state, _dir) = locale_project();
        let state = chosen(state, &["sel", "locale"]);
        let state = input.chars().fold(state, |state, c| {
            send(state, &[Message::Text(TextEdit::Insert(c))])
        });
        names(&state)
    };
    assert_eq!(narrowed("d"), ["de", "(default)"]);
    assert_eq!(narrowed("fr"), ["fr"]);
    assert_eq!(narrowed("def"), ["(default)"]);
    assert_eq!(narrowed("xyz"), Vec::<&str>::new());
}

#[test]
fn two_bundles_with_one_name_are_told_apart_by_their_folders() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a.properties"), b"a=1\n").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub").join("a.properties"), b"a=1\n").unwrap();
    let (session, problems) = Session::open(dir.path());
    let state = send(AppState::new(session, problems), &[Message::Viewport(20)]);
    let state = chosen(state, &["sel", "bundle"]);
    assert_eq!(names(&state), ["a (.)", "a (sub)"]);
}

#[test]
fn sel_is_not_offered_when_the_marks_are_not_files() {
    let (state, _dir) = locale_project();
    // mark the bundle `m`
    let state = send(state, &[Message::Down, Message::ToggleMark]);
    assert!(!names(&typed(state, "se")).contains(&"sel"));
}
