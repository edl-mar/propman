//! Specification of editing a file in the editor of the user: what `update` asks for and tells
//! the user, what `perform` does, and the whole chain from `Edit` to the new lines.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use app::{
    AppState, EditorOutcome, Effect, Message, OverviewState, ReloadError, Session, View, perform,
    preview, update,
};
use common::{TempDir, fixtures};
use store::FileId;

/// A folder with `a.properties`; the list is: folder, bundle `a`, the file.
fn one_file(bytes: &[u8]) -> (AppState, TempDir) {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a.properties"), bytes).unwrap();
    let (session, problems) = Session::open(dir.path());
    assert!(problems.is_empty(), "{problems:?}");
    (AppState::new(session, problems), dir)
}

fn send(state: AppState, messages: &[Message]) -> AppState {
    messages
        .iter()
        .fold(state, |state, &message| update(state, message).0)
}

fn on_the_file(state: AppState) -> AppState {
    send(
        state,
        &[Message::Viewport(10), Message::Down, Message::Down],
    )
}

fn the_file(state: &AppState) -> FileId {
    state.session().store().files()[0]
}

fn overview_state(state: &AppState) -> OverviewState {
    match state.view() {
        View::Overview(overview) => overview.clone(),
    }
}

fn path_of(dir: &TempDir) -> PathBuf {
    dir.path().join("a.properties")
}

// ── Edit ─────────────────────────────────────────────────────────────────────

#[test]
fn edit_on_a_file_asks_to_edit_that_file() {
    let (state, dir) = one_file(b"a=1\n");
    let state = on_the_file(state);
    let file = the_file(&state);
    let (_, effect) = update(state, Message::Edit);
    assert_eq!(
        effect,
        Some(Effect::Edit {
            file,
            path: path_of(&dir)
        })
    );
}

#[test]
fn edit_on_a_folder_or_a_bundle_asks_for_nothing() {
    let (state, _dir) = one_file(b"a=1\n");
    let state = send(state, &[Message::Viewport(10)]);
    let (state, effect) = update(state, Message::Edit);
    assert_eq!(effect, None, "the folder");
    let state = send(state, &[Message::Down]);
    assert_eq!(update(state, Message::Edit).1, None, "the bundle");
}

// ── What comes back from the editor ──────────────────────────────────────────

#[test]
fn a_closed_editor_asks_to_read_the_file_again() {
    let (state, _dir) = one_file(b"a=1\n");
    let file = the_file(&state);
    let (state, effect) = update(
        state,
        Message::EditorClosed {
            file,
            outcome: EditorOutcome::Done,
        },
    );
    assert_eq!(effect, Some(Effect::Reload(file)));
    assert_eq!(state.alert(), None);
}

#[test]
fn an_editor_that_did_not_work_is_told_to_the_user_and_the_file_is_not_read_again() {
    let cases = [
        (EditorOutcome::NoEditor, "no editor: set VISUAL or EDITOR"),
        (EditorOutcome::CouldNotStart, "could not start the editor"),
        (EditorOutcome::Failed(Some(2)), "exit code 2"),
        (
            EditorOutcome::Failed(None),
            "the editor ended with an error",
        ),
    ];
    for (outcome, text) in cases {
        let (state, _dir) = one_file(b"a=1\n");
        let file = the_file(&state);
        let (state, effect) = update(state, Message::EditorClosed { file, outcome });
        assert_eq!(effect, None, "{outcome:?}");
        assert!(
            state.alert().is_some_and(|alert| alert.contains(text)),
            "{outcome:?}: {:?}",
            state.alert()
        );
    }
}

#[test]
fn a_file_that_was_read_again_says_nothing() {
    let (state, _dir) = one_file(b"a=1\n");
    let file = the_file(&state);
    let (state, effect) = update(
        state,
        Message::Reloaded {
            file,
            result: Ok(()),
        },
    );
    assert_eq!(effect, None);
    assert_eq!(state.alert(), None);
}

#[test]
fn a_file_that_could_not_be_read_again_is_told_to_the_user() {
    let cases = [
        (ReloadError::Deleted, "deleted"),
        (ReloadError::Unreadable, "could not be read"),
        (ReloadError::Undecodable, "could not be decoded"),
        (ReloadError::UnknownFile, "unknown file"),
    ];
    for (error, text) in cases {
        let (state, _dir) = one_file(b"a=1\n");
        let file = the_file(&state);
        let (state, effect) = update(
            state,
            Message::Reloaded {
                file,
                result: Err(error),
            },
        );
        assert_eq!(effect, None, "{error:?}");
        assert!(
            state.alert().is_some_and(|alert| alert.contains(text)),
            "{error:?}: {:?}",
            state.alert()
        );
    }
}

// ── The alert ───────────────────────────────────────────────────────────────

fn with_an_alert() -> (AppState, TempDir) {
    let (state, dir) = one_file(b"a=1\n");
    let file = the_file(&state);
    let state = send(
        state,
        &[Message::EditorClosed {
            file,
            outcome: EditorOutcome::NoEditor,
        }],
    );
    assert!(state.alert().is_some());
    (state, dir)
}

/// What the cursor and the preview are, to see that nothing moved.
fn looks_like(state: &AppState) -> (usize, usize, bool, usize) {
    let overview = overview_state(state);
    (
        overview.index(),
        overview.scroll(),
        overview.preview_on(),
        overview.preview_scroll(),
    )
}

#[test]
fn an_alert_blocks_what_the_keys_of_the_view_do() {
    let blocked = [
        Message::Down,
        Message::Up,
        Message::PageDown,
        Message::First,
        Message::Last,
        Message::TogglePreview,
        Message::PreviewDown,
        Message::PreviewHalfPageDown,
        Message::Edit,
    ];
    for message in blocked {
        let (state, _dir) = with_an_alert();
        let before = looks_like(&state);
        let text = state.alert().map(str::to_owned);
        let (state, effect) = update(state, message);
        assert_eq!(effect, None, "{message:?}");
        assert_eq!(looks_like(&state), before, "{message:?}");
        assert_eq!(state.alert().map(str::to_owned), text, "{message:?}");
    }
}

#[test]
fn edit_on_a_file_is_blocked_by_an_alert_and_gives_no_effect() {
    let (state, _dir) = one_file(
        b"a=1
",
    );
    let state = on_the_file(state);
    let file = the_file(&state);
    let state = send(
        state,
        &[Message::EditorClosed {
            file,
            outcome: EditorOutcome::NoEditor,
        }],
    );
    assert_eq!(update(state, Message::Edit).1, None);
}

#[test]
fn the_size_of_the_screen_still_reaches_the_state_while_an_alert_is_shown() {
    let (state, _dir) = with_an_alert();
    let state = send(state, &[Message::Viewport(20)]);
    assert_eq!(overview_state(&state).height(), 20);
    assert!(state.alert().is_some());
}

#[test]
fn dismissing_closes_the_alert_and_the_keys_of_the_view_work_again() {
    let (state, _dir) = with_an_alert();
    let state = send(state, &[Message::DismissAlert]);
    assert_eq!(state.alert(), None);
    let state = send(state, &[Message::Down]);
    assert_eq!(overview_state(&state).index(), 1);
}

#[test]
fn dismissing_without_an_alert_does_nothing() {
    let (state, _dir) = one_file(
        b"a=1
",
    );
    let state = send(state, &[Message::Viewport(10), Message::Down]);
    let before = looks_like(&state);
    let (state, effect) = update(state, Message::DismissAlert);
    assert_eq!(effect, None);
    assert_eq!(looks_like(&state), before);
    assert_eq!(state.alert(), None);
}

#[test]
fn an_alert_does_not_keep_the_program_from_quitting() {
    let (state, _dir) = with_an_alert();
    let state = send(state, &[Message::Quit]);
    assert!(state.should_quit());
}

#[test]
fn a_new_alert_replaces_the_old_one() {
    let (state, _dir) = with_an_alert();
    let file = the_file(&state);
    let state = send(
        state,
        &[Message::EditorClosed {
            file,
            outcome: EditorOutcome::CouldNotStart,
        }],
    );
    assert!(state.alert().unwrap().contains("could not start"));
}

// ── Performing ───────────────────────────────────────────────────────────────

#[test]
fn performing_edit_runs_the_editor_on_the_path_and_tells_how_it_ended() {
    for outcome in [
        EditorOutcome::Done,
        EditorOutcome::NoEditor,
        EditorOutcome::Failed(Some(1)),
    ] {
        let (state, dir) = one_file(b"a=1\n");
        let file = the_file(&state);
        let mut called_with = None;
        let (state, message) = perform(
            state,
            Effect::Edit {
                file,
                path: path_of(&dir),
            },
            |path| {
                called_with = Some(path.to_path_buf());
                outcome
            },
        );
        assert_eq!(called_with, Some(path_of(&dir)));
        assert_eq!(message, Message::EditorClosed { file, outcome });
        assert_eq!(state.alert(), None, "update tells the user, not perform");
    }
}

#[test]
fn performing_reload_reads_the_file_without_running_the_editor() {
    let (state, dir) = one_file(b"a=1\n");
    let state = on_the_file(state);
    let file = the_file(&state);
    std::fs::write(path_of(&dir), b"a=1\nb=2\n").unwrap();

    let (state, message) = perform(state, Effect::Reload(file), |_| {
        panic!("the editor must not run")
    });

    assert_eq!(
        message,
        Message::Reloaded {
            file,
            result: Ok(())
        }
    );
    assert_eq!(preview(&state).unwrap().total, 2);
}

#[test]
fn performing_reload_of_a_deleted_file_tells_the_error() {
    let (state, dir) = one_file(b"a=1\n");
    let file = the_file(&state);
    std::fs::remove_file(path_of(&dir)).unwrap();
    let (_, message) = perform(state, Effect::Reload(file), |_| EditorOutcome::Done);
    assert_eq!(
        message,
        Message::Reloaded {
            file,
            result: Err(ReloadError::Deleted)
        }
    );
}

// ── The whole chain ──────────────────────────────────────────────────────────

/// What the loop does: update, and while there is an effect, perform it and update with the
/// message that comes back.
fn settle(
    mut state: AppState,
    message: Message,
    mut edit: impl FnMut(&std::path::Path) -> EditorOutcome,
) -> AppState {
    let (next, mut effect) = update(state, message);
    state = next;
    while let Some(current) = effect {
        let (next, message) = perform(state, current, &mut edit);
        let (next, following) = update(next, message);
        state = next;
        effect = following;
    }
    state
}

#[test]
fn a_file_that_the_editor_changed_shows_its_new_lines() {
    let (state, _dir) = one_file(b"a=1\n");
    let state = on_the_file(state);
    assert_eq!(preview(&state).unwrap().total, 1);

    let state = settle(state, Message::Edit, |path| {
        std::fs::write(path, b"a=1\nb=2\nc=3\n").unwrap();
        EditorOutcome::Done
    });

    let shown = preview(&state).unwrap();
    assert_eq!(shown.total, 3);
    assert_eq!(shown.lines[2].text, "c=3");
    assert_eq!(state.alert(), None);
}

#[test]
fn an_editor_that_failed_leaves_the_lines_and_says_so() {
    let (state, _dir) = one_file(b"a=1\n");
    let state = on_the_file(state);
    let state = settle(state, Message::Edit, |path| {
        std::fs::write(path, b"changed\n").unwrap();
        EditorOutcome::Failed(Some(1))
    });
    assert_eq!(preview(&state).unwrap().lines[0].text, "a=1");
    assert!(state.alert().unwrap().contains("exit code 1"));
}

#[test]
fn an_editor_that_deleted_the_file_says_so() {
    let (state, _dir) = one_file(b"a=1\n");
    let state = on_the_file(state);
    let state = settle(state, Message::Edit, |path| {
        std::fs::remove_file(path).unwrap();
        EditorOutcome::Done
    });
    assert!(state.alert().unwrap().contains("deleted"));
}

#[test]
fn a_real_file_that_the_editor_left_alone_shows_the_same_lines() {
    // the fixtures are never changed: the test works on a copy of one file
    let dir = TempDir::new();
    let source = fixtures().join("edge/encodings/messages_fr.properties");
    std::fs::copy(&source, dir.path().join("messages_fr.properties")).unwrap();
    let (session, problems) = Session::open(dir.path());
    let state = on_the_file(AppState::new(session, problems));
    let before = preview(&state).unwrap();

    let state = settle(state, Message::Edit, |_| EditorOutcome::Done);

    assert_eq!(
        preview(&state).unwrap(),
        before,
        "nothing changed on the disk"
    );
}
