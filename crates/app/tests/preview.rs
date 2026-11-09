//! Specification of the preview of the selected file in the overview: what it shows (see
//! `app::preview`) and how the messages for it change the state.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{AppState, Encoding, Message, OverviewState, Preview, Session, View, preview, update};
use common::{TempDir, fixtures};

fn state_for(folder: &std::path::Path) -> AppState {
    let (session, problems) = Session::open(folder);
    assert!(problems.is_empty(), "{problems:?}");
    AppState::new(session, problems)
}

/// A folder with one file, `a.properties`, whose list is: folder, bundle `a`, the file.
fn one_file(bytes: &[u8]) -> (AppState, TempDir) {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a.properties"), bytes).unwrap();
    (state_for(dir.path()), dir)
}

fn send(state: AppState, messages: &[Message]) -> AppState {
    messages
        .iter()
        .fold(state, |state, &message| update(state, message))
}

/// The file is selected and the list is `height` rows high.
fn on_the_file(state: AppState, height: usize) -> AppState {
    send(
        state,
        &[Message::Viewport(height), Message::Down, Message::Down],
    )
}

/// 30 lines, `l01=1` to `l30=1`.
fn thirty_lines() -> Vec<u8> {
    (1..=30)
        .flat_map(|n| format!("l{n:02}=1\n").into_bytes())
        .collect()
}

fn overview_state(state: &AppState) -> OverviewState {
    match state.view() {
        View::Overview(overview) => *overview,
    }
}

fn shown(state: &AppState) -> Preview {
    preview(state).expect("a preview")
}

fn texts(preview: &Preview) -> Vec<&str> {
    preview.lines.iter().map(|l| l.text.as_str()).collect()
}

fn endings(preview: &Preview) -> Vec<&str> {
    preview.lines.iter().map(|l| l.ending).collect()
}

// ── What the preview is made of ──────────────────────────────────────────────

#[test]
fn a_folder_and_a_bundle_have_no_preview() {
    let (state, _dir) = one_file(b"x=1\n");
    let state = send(state, &[Message::Viewport(10)]);
    assert_eq!(preview(&state), None, "the folder");
    let state = send(state, &[Message::Down]);
    assert_eq!(preview(&state), None, "the bundle");
}

#[test]
fn a_file_has_a_preview_that_names_it_and_counts_its_lines() {
    let (state, _dir) = one_file(b"x=1\r\ny=2\nz=3");
    let preview = shown(&on_the_file(state, 10));
    assert_eq!(preview.name, "a.properties");
    assert_eq!(preview.encoding, Encoding::Utf8);
    assert_eq!(preview.total, 3);
    assert_eq!(preview.first, 0);
}

#[test]
fn the_lines_have_numbers_texts_and_endings() {
    let (state, _dir) = one_file(b"x=1\r\ny=2\nz=3");
    let preview = shown(&on_the_file(state, 10));
    let numbers: Vec<usize> = preview.lines.iter().map(|l| l.number).collect();
    assert_eq!(numbers, [1, 2, 3]);
    assert_eq!(texts(&preview), ["x=1", "y=2", "z=3"]);
    assert_eq!(endings(&preview), ["CRLF", "LF", "(none)"]);
}

#[test]
fn a_lone_carriage_return_is_a_line_ending() {
    let (state, _dir) = one_file(b"a=1\rb=2\n");
    let preview = shown(&on_the_file(state, 10));
    assert_eq!(texts(&preview), ["a=1", "b=2"]);
    assert_eq!(endings(&preview), ["CR", "LF"]);
}

#[test]
fn control_characters_are_shown_as_a_dot_and_other_characters_as_they_are() {
    let (state, _dir) = one_file("a\tb=\u{1}\u{7f}\n\u{e9}=\u{fc}\u{20ac}\n".as_bytes());
    let preview = shown(&on_the_file(state, 10));
    assert_eq!(
        texts(&preview),
        ["a\u{b7}b=\u{b7}\u{b7}", "\u{e9}=\u{fc}\u{20ac}"]
    );
}

#[test]
fn an_empty_file_has_a_preview_without_lines() {
    let (state, _dir) = one_file(b"");
    let preview = shown(&on_the_file(state, 10));
    assert_eq!(preview.total, 0);
    assert!(preview.lines.is_empty());
}

#[test]
fn the_lines_of_a_real_file_with_another_encoding_and_mixed_endings() {
    let folder = fixtures().join("edge/encodings");
    // the list: folder, bundle, then the default file and the locales de, fr, it, pl
    let at = |downs: usize| {
        let mut state = send(state_for(&folder), &[Message::Viewport(50)]);
        for _ in 0..downs {
            state = send(state, &[Message::Down]);
        }
        shown(&state)
    };

    let italian = at(5);
    assert_eq!(italian.name, "messages_it.properties");
    assert_eq!(italian.encoding, Encoding::Windows1252);
    assert_eq!(italian.lines.last().unwrap().ending, "(none)");

    let polish = at(6);
    assert_eq!(polish.name, "messages_pl.properties");
    assert!(texts(&polish).contains(&"greeting=Cze\u{15b}\u{107}"));
    for ending in ["LF", "CRLF", "CR"] {
        assert!(endings(&polish).contains(&ending), "{ending}");
    }
}

// ── The window ───────────────────────────────────────────────────────────────

#[test]
fn the_window_is_as_high_as_the_list_without_the_row_of_the_header() {
    let (state, _dir) = one_file(&thirty_lines());
    let preview = shown(&on_the_file(state, 10));
    assert_eq!(preview.total, 30);
    assert_eq!(preview.lines.len(), 9);
    assert_eq!(preview.lines[0].text, "l01=1");
    assert_eq!(preview.lines[8].text, "l09=1");
}

#[test]
fn the_window_is_empty_as_long_as_the_height_is_not_known() {
    let (state, _dir) = one_file(&thirty_lines());
    let state = send(state, &[Message::Down, Message::Down]);
    let preview = shown(&state);
    assert_eq!(preview.total, 30);
    assert!(preview.lines.is_empty());
}

#[test]
fn the_window_follows_the_scroll_position() {
    let (state, _dir) = one_file(&thirty_lines());
    let state = send(
        on_the_file(state, 10),
        &[
            Message::PreviewDown,
            Message::PreviewDown,
            Message::PreviewDown,
        ],
    );
    let preview = shown(&state);
    assert_eq!(overview_state(&state).preview_scroll(), 3);
    assert_eq!(preview.first, 3);
    assert_eq!(preview.lines[0].number, 4);
    assert_eq!(preview.lines[0].text, "l04=1");
    assert_eq!(preview.lines.len(), 9);
}

#[test]
fn a_short_file_is_not_filled_up() {
    let (state, _dir) = one_file(b"x=1\ny=2\n");
    let preview = shown(&on_the_file(state, 10));
    assert_eq!(preview.lines.len(), 2);
}

// ── The messages ─────────────────────────────────────────────────────────────

#[test]
fn the_preview_is_on_at_the_start_and_the_message_switches_it() {
    let (state, _dir) = one_file(b"x=1\n");
    assert!(overview_state(&state).preview_on());
    let state = send(state, &[Message::TogglePreview]);
    assert!(!overview_state(&state).preview_on());
    let state = send(state, &[Message::TogglePreview]);
    assert!(overview_state(&state).preview_on());
}

#[test]
fn switching_the_preview_does_not_move_the_selection_or_the_scroll_position() {
    let (state, _dir) = one_file(&thirty_lines());
    let state = send(
        on_the_file(state, 10),
        &[Message::PreviewDown, Message::PreviewDown],
    );
    let before = overview_state(&state);
    let state = send(state, &[Message::TogglePreview, Message::TogglePreview]);
    assert_eq!(overview_state(&state), before);
    assert_eq!(overview_state(&state).selected(), 2);
    assert_eq!(overview_state(&state).preview_scroll(), 2);
}

#[test]
fn scrolling_by_a_line_stops_at_the_top_and_at_the_end() {
    let (state, _dir) = one_file(&thirty_lines());
    let state = send(on_the_file(state, 10), &[Message::PreviewUp]);
    assert_eq!(overview_state(&state).preview_scroll(), 0);

    // 30 lines, 9 of them in the window: the last window starts at index 21
    let mut state = state;
    for _ in 0..100 {
        state = send(state, &[Message::PreviewDown]);
    }
    assert_eq!(overview_state(&state).preview_scroll(), 21);
    assert_eq!(shown(&state).lines.last().unwrap().text, "l30=1");
    assert_eq!(shown(&state).lines.len(), 9);
}

#[test]
fn scrolling_by_half_a_page_is_half_of_the_height_of_the_preview() {
    let (state, _dir) = one_file(&thirty_lines());
    let scroll = |state: &AppState| overview_state(state).preview_scroll();
    // 9 rows for lines, half is 4
    let state = send(on_the_file(state, 10), &[Message::PreviewHalfPageDown]);
    assert_eq!(scroll(&state), 4);
    let state = send(state, &[Message::PreviewHalfPageDown]);
    assert_eq!(scroll(&state), 8);
    let state = send(state, &[Message::PreviewHalfPageUp]);
    assert_eq!(scroll(&state), 4);
    let state = send(
        state,
        &[Message::PreviewHalfPageUp, Message::PreviewHalfPageUp],
    );
    assert_eq!(scroll(&state), 0);
    let mut state = state;
    for _ in 0..20 {
        state = send(state, &[Message::PreviewHalfPageDown]);
    }
    assert_eq!(scroll(&state), 21);
}

#[test]
fn half_a_page_is_at_least_one_line() {
    let (state, _dir) = one_file(&thirty_lines());
    // 1 row for lines
    let state = send(on_the_file(state, 2), &[Message::PreviewHalfPageDown]);
    assert_eq!(overview_state(&state).preview_scroll(), 1);
}

#[test]
fn the_scroll_position_starts_again_when_the_selection_changes() {
    let dir = TempDir::new();
    std::fs::write(dir.path().join("a.properties"), thirty_lines()).unwrap();
    std::fs::write(dir.path().join("b.properties"), thirty_lines()).unwrap();
    // the list: folder, bundle a, a, bundle b, b
    let scrolled = || {
        send(
            on_the_file(state_for(dir.path()), 10),
            &[Message::PreviewDown, Message::PreviewDown],
        )
    };
    assert_eq!(overview_state(&scrolled()).preview_scroll(), 2);

    for message in [Message::Down, Message::Up, Message::First, Message::Last] {
        let moved = send(scrolled(), &[message]);
        assert_eq!(overview_state(&moved).preview_scroll(), 0, "{message:?}");
    }
}

#[test]
fn scrolling_without_a_file_does_nothing() {
    let (state, _dir) = one_file(&thirty_lines());
    let state = send(
        state,
        &[
            Message::Viewport(10),
            Message::PreviewDown,
            Message::PreviewHalfPageDown,
        ],
    );
    assert_eq!(overview_state(&state).preview_scroll(), 0);
}

#[test]
fn a_bigger_height_keeps_the_scroll_position_inside_the_file() {
    let (state, _dir) = one_file(&thirty_lines());
    let mut state = on_the_file(state, 10);
    for _ in 0..100 {
        state = send(state, &[Message::PreviewDown]);
    }
    assert_eq!(overview_state(&state).preview_scroll(), 21);
    // 29 rows for lines: the last window starts at index 1
    let state = send(state, &[Message::Viewport(30)]);
    assert_eq!(overview_state(&state).preview_scroll(), 1);
    assert_eq!(shown(&state).lines.last().unwrap().text, "l30=1");
}
