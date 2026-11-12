//! Specification of a condition as the selection: `sel` narrows the files that are selected to
//! those that have something (a locale, a bundle, an encoding), see `app::Predicate`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{
    AppState, Encoding, ItemId, Message, OverviewItem, OverviewState, Predicate, Session, View,
    applicable, overview, selected_items, selection, selection_text, target, update,
};
use common::TempDir;
use store::FileId;

/// A folder with two bundles. The list is: the folder (0), the bundle `m` (1) with the files `m`
/// (2), `m_de` (3) and `m_fr` (4), the bundle `n` (5) with the files `n` (6) and `n_de` (7).
/// `m_fr` has a letter that makes it not UTF-8; the others are UTF-8.
fn project() -> (AppState, TempDir) {
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

fn send(state: AppState, messages: &[Message]) -> AppState {
    messages
        .iter()
        .fold(state, |state, &message| update(state, message).0)
}

fn view(state: &AppState) -> OverviewState {
    match state.view() {
        View::Overview(overview) => overview.clone(),
    }
}

fn file(state: &AppState, name: &str) -> FileId {
    overview(state.session())
        .iter()
        .find_map(|item| match item {
            OverviewItem::File { id, name: n, .. } if n == name => Some(*id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file {name}"))
}

/// The ids of the files with these names, in this order.
fn files(state: &AppState, names: &[&str]) -> Vec<ItemId> {
    names
        .iter()
        .map(|name| ItemId::File(file(state, &format!("{name}.properties"))))
        .collect()
}

fn locale(state: &AppState, name: &str) -> Predicate {
    let store = state.session().store();
    let assignment = store.file_assignment(file(state, name)).unwrap();
    Predicate::Locale(assignment.locale)
}

fn bundle(state: &AppState, name: &str) -> Predicate {
    let store = state.session().store();
    let found = store
        .bundles()
        .into_iter()
        .find(|&bundle| store.bundle_name(bundle) == Some(name))
        .unwrap();
    Predicate::Bundle(found)
}

fn select(state: AppState, predicate: Predicate) -> AppState {
    send(state, &[Message::Select(predicate)])
}

fn down(state: AppState, times: usize) -> AppState {
    (0..times).fold(state, |state, _| send(state, &[Message::Down]))
}

fn encoding_of(state: &AppState, name: &str) -> Encoding {
    let store = state.session().store();
    store.file_encoding(file(state, name)).unwrap()
}

// ── Selecting by what the files have ─────────────────────────────────────────

#[test]
fn there_is_no_condition_at_the_start() {
    let (state, _dir) = project();
    assert!(view(&state).condition().is_empty());
    assert_eq!(selected_items(&state), []);
}

#[test]
fn a_condition_selects_the_files_that_have_it_in_the_order_of_the_list() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    let state = select(state, de);
    assert_eq!(view(&state).condition(), [de]);
    let wanted = files(&state, &["m_de", "n_de"]);
    assert_eq!(selected_items(&state), wanted);
    assert_eq!(selection(&state), wanted);
}

#[test]
fn the_default_file_is_a_locale_of_its_own() {
    let (state, _dir) = project();
    let state = select(state, Predicate::Locale(None));
    assert_eq!(selection(&state), files(&state, &["m", "n"]));
}

#[test]
fn a_bundle_selects_its_files() {
    let (state, _dir) = project();
    let m = bundle(&state, "m");
    let state = select(state, m);
    assert_eq!(selection(&state), files(&state, &["m", "m_de", "m_fr"]));
}

#[test]
fn an_encoding_selects_the_files_that_have_it() {
    let (state, _dir) = project();
    let state = select(state, Predicate::Encoding(Encoding::Utf8));
    assert_eq!(
        selection(&state),
        files(&state, &["m", "m_de", "n", "n_de"])
    );

    let (state, _dir) = project();
    let other = encoding_of(&state, "m_fr.properties");
    let state = select(state, Predicate::Encoding(other));
    assert_eq!(selection(&state), files(&state, &["m_fr"]));
}

#[test]
fn every_new_condition_narrows_what_there_is() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    let m = bundle(&state, "m");
    let state = select(select(state, de), m);
    assert_eq!(view(&state).condition(), [de, m]);
    assert_eq!(selection(&state), files(&state, &["m_de"]));
}

#[test]
fn a_condition_that_nothing_meets_selects_nothing_and_does_not_fall_back_to_the_cursor() {
    let (state, _dir) = project();
    let fr = locale(&state, "m_fr.properties");
    let n = bundle(&state, "n");
    // the cursor is on a file; there is no file of the bundle `n` in `fr`
    let state = select(select(down(state, 2), fr), n);
    assert_eq!(selection(&state), []);
    assert_eq!(selected_items(&state), []);
    assert!(!applicable(&state, Message::SetEncoding(Encoding::Latin1)));
    assert!(!applicable(&state, Message::Edit));
    assert_eq!(target(&state).unwrap(), "0 files (locale fr, bundle n)");
}

#[test]
fn the_condition_does_not_depend_on_where_the_cursor_is() {
    for message in [
        Message::Down,
        Message::Last,
        Message::First,
        Message::TogglePreview,
        Message::PreviewDown,
    ] {
        let (state, _dir) = project();
        let de = locale(&state, "m_de.properties");
        let state = send(select(state, de), &[message]);
        assert_eq!(selected_items(&state).len(), 2, "{message:?}");
        assert_eq!(view(&state).condition(), [de], "{message:?}");
    }
}

// ── Clearing, and marks ──────────────────────────────────────────────────────

#[test]
fn clearing_the_selection_takes_the_condition_and_the_marks_away() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    let state = send(select(state, de), &[Message::ClearSelection]);
    assert!(view(&state).condition().is_empty());
    assert_eq!(selected_items(&state), []);

    let state = send(
        down(state, 2),
        &[Message::ToggleMark, Message::ClearSelection],
    );
    assert!(view(&state).marked().is_empty());
}

#[test]
fn clearing_can_be_done_only_if_something_is_selected() {
    let (state, _dir) = project();
    assert!(!applicable(&state, Message::ClearSelection));
    let de = locale(&state, "m_de.properties");
    let state = select(state, de);
    assert!(applicable(&state, Message::ClearSelection));
    let state = send(state, &[Message::ClearSelection]);
    assert!(!applicable(&state, Message::ClearSelection));
}

#[test]
fn a_condition_narrows_the_files_that_are_marked_and_they_stay_marked() {
    let (state, _dir) = project();
    // mark m, m_de and m_fr
    let state = send(
        down(state, 2),
        &[
            Message::ToggleMark,
            Message::Down,
            Message::ToggleMark,
            Message::Down,
            Message::ToggleMark,
        ],
    );
    assert_eq!(view(&state).marked().len(), 3);
    let de = locale(&state, "m_de.properties");
    let state = select(state, de);
    assert_eq!(view(&state).marked(), files(&state, &["m_de"]));
    assert!(
        view(&state).condition().is_empty(),
        "the marks stay a fixed set"
    );
    assert_eq!(selection(&state), files(&state, &["m_de"]));
}

#[test]
fn a_condition_cannot_narrow_marks_that_are_not_files() {
    let (state, _dir) = project();
    let state = send(down(state, 1), &[Message::ToggleMark]);
    let de = locale(&state, "m_de.properties");
    let state = select(state, de);
    assert!(state.alert().unwrap().contains("sel works on files"));
    assert_eq!(view(&state).marked().len(), 1);
    assert!(view(&state).condition().is_empty());
    assert!(!applicable(&state, Message::Select(de)));
}

#[test]
fn marking_while_a_condition_holds_makes_it_a_fixed_set_and_toggles_the_item() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    // the cursor is on m_fr, which is not selected: it is added to the marked ones
    let state = send(down(select(state, de), 4), &[Message::ToggleMark]);
    assert!(view(&state).condition().is_empty());
    assert_eq!(
        selected_items(&state),
        files(&state, &["m_de", "m_fr", "n_de"]),
        "in the order of the list"
    );
    assert_eq!(view(&state).marked().len(), 3);

    // the cursor is on m_de, which is selected: it is taken out
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    let state = send(down(select(state, de), 3), &[Message::ToggleMark]);
    assert!(view(&state).condition().is_empty());
    assert_eq!(selected_items(&state), files(&state, &["n_de"]));
}

// ── What the selection is called, and what acts on it ────────────────────────

#[test]
fn the_selection_says_what_it_is() {
    let (state, _dir) = project();
    assert_eq!(selection_text(&state), None);

    let de = locale(&state, "m_de.properties");
    let m = bundle(&state, "m");
    let state = select(state, de);
    assert_eq!(
        selection_text(&state).unwrap(),
        "2 files selected: locale de"
    );
    let state = select(state, m);
    assert_eq!(
        selection_text(&state).unwrap(),
        "1 file selected: locale de, bundle m"
    );

    let (state, _dir) = project();
    let state = select(state, Predicate::Locale(None));
    assert_eq!(
        selection_text(&state).unwrap(),
        "2 files selected: locale (default)"
    );

    let (state, _dir) = project();
    let state = select(state, Predicate::Encoding(Encoding::Utf8));
    assert_eq!(
        selection_text(&state).unwrap(),
        "4 files selected: encoding UTF-8"
    );

    let (state, _dir) = project();
    let state = send(down(state, 2), &[Message::ToggleMark]);
    assert_eq!(selection_text(&state).unwrap(), "1 file marked");
}

#[test]
fn the_target_of_a_condition_is_its_files_and_the_condition() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    let state = select(state, de);
    assert_eq!(target(&state).unwrap(), "2 files (locale de)");
}

#[test]
fn an_action_works_on_the_files_of_the_condition() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    let state = select(state, de);
    let state = send(state, &[Message::SetEncoding(Encoding::Latin1)]);
    assert_eq!(encoding_of(&state, "m_de.properties"), Encoding::Latin1);
    assert_eq!(encoding_of(&state, "n_de.properties"), Encoding::Latin1);
    assert_eq!(encoding_of(&state, "m.properties"), Encoding::Utf8);
    assert_eq!(state.alert(), None);
    assert!(
        !view(&state).condition().is_empty(),
        "the condition is still there"
    );
}

#[test]
fn a_condition_that_picks_one_file_lets_it_be_edited() {
    let (state, dir) = project();
    let de = locale(&state, "m_de.properties");
    let m = bundle(&state, "m");
    let wanted = file(&state, "m_de.properties");
    let (_, effect) = update(select(select(state, de), m), Message::Edit);
    assert_eq!(
        effect,
        Some(app::Effect::Edit {
            file: wanted,
            path: dir.path().join("m_de.properties")
        })
    );
}

#[test]
fn what_selects_can_be_done_from_any_item() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    assert!(applicable(&state, Message::Select(de)), "on the folder");
    assert!(
        applicable(&down(state, 3), Message::Select(de)),
        "on a file"
    );
}

#[test]
fn an_alert_blocks_a_new_condition() {
    let (state, _dir) = project();
    let de = locale(&state, "m_de.properties");
    let file = file(&state, "m.properties");
    let state = send(
        state,
        &[Message::EditorClosed {
            file,
            outcome: app::EditorOutcome::NoEditor,
        }],
    );
    let state = select(state, de);
    assert!(view(&state).condition().is_empty());
}

#[test]
fn messages_stay_small_enough_to_be_copied() {
    fn is_copy<T: Copy>() {}
    is_copy::<Message>();
    is_copy::<Predicate>();
}
