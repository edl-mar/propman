//! Specification of the first screen, the keys and the event loop (see `tui`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use app::{AppState, Message, Session, View, update};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tui::{LoopError, list_height, message_for, render, run_loop};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn fixtures(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn state_for(folder: &Path) -> AppState {
    let (session, problems) = Session::open(folder);
    AppState::new(session, problems)
}

/// A fresh folder below the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "propman-tui-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(width, height)).unwrap()
}

/// What the test buffer shows, one line per row.
fn screen(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (area.top()..area.bottom())
        .map(|y| {
            (area.left()..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn drawn(state: &AppState, width: u16, height: u16) -> String {
    let mut terminal = terminal(width, height);
    terminal.draw(|frame| render(state, frame)).unwrap();
    screen(&terminal)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn press(c: char) -> Event {
    key(KeyCode::Char(c), KeyModifiers::NONE)
}

// ── The screen ───────────────────────────────────────────────────────────────

#[test]
fn the_screen_shows_the_name_of_the_program_and_the_folder() {
    let folder = fixtures("project");
    let screen = drawn(&state_for(&folder), 200, 10);
    assert!(screen.contains("propman"), "{screen}");
    assert!(screen.contains(&folder.display().to_string()), "{screen}");
}

#[test]
fn the_screen_shows_how_many_files_and_bundles_were_loaded() {
    let screen = drawn(&state_for(&fixtures("project")), 120, 10);
    // 12 files in the bundles messages, stripping, testbundle and translations
    assert!(screen.contains("12 files in 4 bundles"), "{screen}");
}

#[test]
fn one_is_singular() {
    let dir = TempDir::new();
    std::fs::write(dir.0.join("only.properties"), b"a=1\n").unwrap();
    let screen = drawn(&state_for(&dir.0), 120, 10);
    assert!(screen.contains("1 file in 1 bundle"), "{screen}");
}

#[test]
fn an_empty_folder_shows_zero() {
    let dir = TempDir::new();
    let screen = drawn(&state_for(&dir.0), 120, 10);
    assert!(screen.contains("0 files in 0 bundles"), "{screen}");
}

#[test]
fn the_screen_names_the_key_to_quit() {
    let screen = drawn(&state_for(&fixtures("project")), 120, 10);
    assert!(screen.contains("q: quit"), "{screen}");
}

#[test]
fn problems_are_shown_only_when_there_are_some() {
    let without = drawn(&state_for(&fixtures("project")), 120, 10);
    assert!(!without.contains("problem"), "{without}");

    let dir = TempDir::new();
    let with = drawn(&state_for(&dir.0.join("does_not_exist")), 120, 10);
    assert!(with.contains("1 problem"), "{with}");
    assert!(!with.contains("1 problems"), "{with}");
}

#[test]
fn drawing_survives_a_tiny_area() {
    let state = state_for(&fixtures("project"));
    for (width, height) in [(0, 0), (1, 1), (5, 1), (20, 2), (3, 30)] {
        let mut terminal = terminal(width, height);
        terminal.draw(|frame| render(&state, frame)).unwrap();
    }
}

#[test]
fn drawing_does_not_change_the_state() {
    let state = state_for(&fixtures("project"));
    let files = state.session().store().files().len();
    drawn(&state, 80, 10);
    assert_eq!(state.session().store().files().len(), files);
    assert!(!state.should_quit());
}

// ── Keys ─────────────────────────────────────────────────────────────────────

#[test]
fn q_quits() {
    assert_eq!(message_for(&press('q')), Some(Message::Quit));
}

#[test]
fn ctrl_c_quits() {
    let event = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(message_for(&event), Some(Message::Quit));
}

#[test]
fn other_keys_give_no_message() {
    for event in [
        press('x'),
        key(KeyCode::Enter, KeyModifiers::NONE),
        key(KeyCode::Esc, KeyModifiers::NONE),
        key(KeyCode::Tab, KeyModifiers::NONE),
    ] {
        assert_eq!(message_for(&event), None, "{event:?}");
    }
}

#[test]
fn the_release_of_a_key_does_not_count() {
    // on Windows a key arrives twice, as a press and as a release
    let release = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert_eq!(message_for(&release), None);
}

#[test]
fn a_character_with_ctrl_and_alt_is_a_plain_character() {
    // AltGr sends Ctrl and Alt together with the character
    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    assert_eq!(message_for(&key(KeyCode::Char('@'), altgr)), None);
    assert_eq!(
        message_for(&key(KeyCode::Char('q'), altgr)),
        Some(Message::Quit)
    );
    // and it is not Ctrl+C
    assert_eq!(message_for(&key(KeyCode::Char('c'), altgr)), None);
}

#[test]
fn events_that_are_not_keys_give_no_message() {
    for event in [Event::Resize(80, 24), Event::FocusGained, Event::FocusLost] {
        assert_eq!(message_for(&event), None, "{event:?}");
    }
}

// ── The event loop ───────────────────────────────────────────────────────────

/// Runs the loop with a fixed list of events. Asking for an event after the list is used up is
/// an error, so a loop that does not stop when it should shows up as one.
fn run_with(
    terminal: &mut Terminal<TestBackend>,
    state: AppState,
    events: Vec<Event>,
) -> (Result<AppState, LoopError<Infallible>>, usize) {
    let mut events = events.into_iter();
    let mut asked = 0;
    let result = run_loop(terminal, state, || {
        asked += 1;
        events
            .next()
            .ok_or_else(|| io::Error::other("no more events"))
    });
    (result, asked)
}

#[test]
fn the_loop_ends_when_quit_is_pressed_and_returns_the_updated_state() {
    let mut terminal = terminal(120, 10);
    let (result, asked) = run_with(
        &mut terminal,
        state_for(&fixtures("project")),
        vec![press('x'), press('q')],
    );

    let state = result.unwrap();
    assert!(state.should_quit());
    assert_eq!(asked, 2, "it must not wait for another event after quit");
}

#[test]
fn the_loop_has_drawn_the_screen_before_it_returns() {
    let mut terminal = terminal(120, 10);
    run_with(
        &mut terminal,
        state_for(&fixtures("project")),
        vec![press('q')],
    )
    .0
    .unwrap();
    assert!(screen(&terminal).contains("12 files in 4 bundles"));
}

#[test]
fn events_without_a_message_do_not_stop_the_loop() {
    let mut terminal = terminal(120, 10);
    let events = vec![
        Event::Resize(100, 20),
        press('x'),
        key(KeyCode::Enter, KeyModifiers::NONE),
        Event::FocusGained,
        press('q'),
    ];
    let (result, asked) = run_with(&mut terminal, state_for(&fixtures("project")), events);
    assert!(result.unwrap().should_quit());
    assert_eq!(asked, 5);
}

#[test]
fn a_failing_source_of_events_ends_the_loop_with_an_error() {
    let mut terminal = terminal(120, 10);
    let (result, asked) = run_with(
        &mut terminal,
        state_for(&fixtures("project")),
        vec![press('x'), press('y')],
    );
    assert!(matches!(result, Err(LoopError::Events(_))));
    assert_eq!(asked, 3, "it asked until the events were used up");
}

#[test]
fn the_loop_keeps_the_session_it_was_given() {
    let mut terminal = terminal(120, 10);
    let state = run_with(
        &mut terminal,
        state_for(&fixtures("project")),
        vec![press('q')],
    )
    .0
    .unwrap();
    assert_eq!(state.session().store().files().len(), 12);
}

// ── The bundle overview ──────────────────────────────────────────────────────

fn write(dir: &TempDir, relative: &str, bytes: &[u8]) {
    let path = dir.0.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn press_all(state: AppState, messages: &[Message]) -> AppState {
    messages
        .iter()
        .fold(state, |state, &message| update(state, message))
}

/// The lines of the list on the screen: after the title, the count and (if there are any) the
/// line for the problems, and before the line with the keys.
fn list_lines(screen: &str, problems: bool) -> Vec<String> {
    let lines: Vec<&str> = screen.lines().collect();
    let start = if problems { 3 } else { 2 };
    lines[start..lines.len().saturating_sub(1)]
        .iter()
        .map(|line| line.trim_end().to_owned())
        .collect()
}

/// The indentation of an item: the spaces after the two characters of the selection mark.
fn indent(line: &str) -> usize {
    line.chars().skip(2).take_while(|&c| c == ' ').count()
}

#[test]
fn the_list_height_is_what_is_left_after_the_header_and_the_keys() {
    assert_eq!(list_height(24, false), 21);
    assert_eq!(list_height(24, true), 20);
    assert_eq!(list_height(4, false), 1);
    assert_eq!(list_height(3, false), 0);
    assert_eq!(list_height(2, true), 0);
    assert_eq!(list_height(0, false), 0);
}

#[test]
fn the_overview_lists_folders_bundles_and_files_in_order() {
    let screen = drawn(&state_for(&fixtures("project")), 110, 30);
    let list = list_lines(&screen, false);

    let expected: [&[&str]; 18] = [
        &["3 bundles"],
        &["messages", "4 files"],
        &["(default)", "messages.properties", "UTF-8"],
        &["de", "messages_de.properties", "UTF-8"],
        &["fr", "messages_fr.properties", "0 lines"],
        &["it", "messages_it.properties", "0 lines"],
        &["stripping", "1 file"],
        &["(default)", "stripping.properties"],
        &["testbundle", "4 files"],
        &["(default)", "testbundle.properties", "0 lines"],
        &["de", "testbundle_de.properties", "0 lines"],
        &["fr", "testbundle_fr.properties"],
        &["it", "testbundle_it.properties"],
        &["sub_folder", "1 bundle"],
        &["translations", "3 files"],
        &["(default)", "translations.properties"],
        &["de", "translations_de.properties"],
        &["si", "translations_si.properties"],
    ];
    for (index, parts) in expected.iter().enumerate() {
        for part in *parts {
            assert!(
                list[index].contains(part),
                "line {index} {:?} lacks {part:?}\n{screen}",
                list[index]
            );
        }
    }
}

#[test]
fn folders_bundles_and_files_are_indented_by_kind() {
    let screen = drawn(&state_for(&fixtures("project")), 110, 30);
    let list = list_lines(&screen, false);

    // F = folder, B = bundle, f = file
    let kinds = "FBffffBfBffffFBfff";
    for (index, kind) in kinds.chars().enumerate() {
        let expected = match kind {
            'F' => 0,
            'B' => 2,
            _ => 4,
        };
        assert_eq!(
            indent(&list[index]),
            expected,
            "line {index}: {:?}",
            list[index]
        );
    }
}

#[test]
fn exactly_one_line_carries_the_selection_mark_and_it_follows_the_selection() {
    let state = state_for(&fixtures("project"));
    let marked = |state: &AppState| -> Vec<usize> {
        let screen = drawn(state, 110, 30);
        list_lines(&screen, false)
            .iter()
            .enumerate()
            .filter(|(_, line)| line.starts_with('>'))
            .map(|(index, _)| index)
            .collect()
    };

    assert_eq!(marked(&state), [0]);
    let state = press_all(state, &[Message::Down, Message::Down]);
    assert_eq!(marked(&state), [2]);
    let state = press_all(state, &[Message::Last]);
    assert_eq!(marked(&state), [17]);
}

#[test]
fn an_uncertain_file_is_marked_with_a_question_mark_and_a_certain_one_is_not() {
    let dir = TempDir::new();
    write(&dir, "single_file_de.properties", b"a=1\n");
    let screen = drawn(&state_for(&dir.0), 110, 30);
    let list = list_lines(&screen, false);
    assert!(list[2].contains("single_file_de.properties"), "{screen}");
    assert!(list[2].contains('?'), "{screen}");

    let certain = drawn(&state_for(&fixtures("project")), 110, 30);
    assert!(!certain.contains('?'), "{certain}");
}

#[test]
fn an_excluded_bundle_says_so() {
    let (mut session, problems) = Session::open(&fixtures("project"));
    let bundle = session.store().bundles()[0];
    session.exclude_bundle(bundle, true);
    let state = AppState::new(session, problems);

    let screen = drawn(&state, 110, 30);
    let marked: Vec<String> = list_lines(&screen, false)
        .into_iter()
        .filter(|line| line.contains("[excluded]"))
        .collect();
    assert_eq!(marked.len(), 1, "{screen}");
    assert!(marked[0].contains("messages"), "{screen}");
}

#[test]
fn encodings_have_readable_names() {
    let dir = TempDir::new();
    write(&dir, "u8.properties", b"a=1\n");
    write(&dir, "bom.properties", b"\xef\xbb\xbfa=1\n");
    write(&dir, "lat.properties", b"a=caf\xe9\n");
    write(&dir, "win.properties", b"a=\x80\n");
    let screen = drawn(&state_for(&dir.0), 110, 30);

    let line_of = |name: &str| {
        list_lines(&screen, false)
            .into_iter()
            .find(|line| line.contains(name))
            .unwrap_or_else(|| panic!("{name} not shown\n{screen}"))
    };
    assert!(line_of("u8.properties").contains("UTF-8"));
    assert!(!line_of("u8.properties").contains("BOM"));
    assert!(line_of("bom.properties").contains("UTF-8+BOM"));
    assert!(line_of("lat.properties").contains("Latin-1"));
    assert!(line_of("win.properties").contains("Windows-1252"));
}

#[test]
fn the_number_of_lines_is_shown_with_the_right_word() {
    let dir = TempDir::new();
    write(&dir, "empty.properties", b"");
    write(&dir, "one.properties", b"a=1\n");
    write(&dir, "two.properties", b"a=1\nb=2\n");
    let screen = drawn(&state_for(&dir.0), 110, 30);

    let line_of = |name: &str| {
        list_lines(&screen, false)
            .into_iter()
            .find(|line| line.contains(name))
            .unwrap()
    };
    assert!(line_of("empty.properties").contains("0 lines"));
    assert!(
        line_of("one.properties").contains("1 line")
            && !line_of("one.properties").contains("1 lines")
    );
    assert!(line_of("two.properties").contains("2 lines"));
}

#[test]
fn a_long_folder_path_is_cut_from_the_left_and_keeps_its_end_and_its_count() {
    let dir = TempDir::new();
    write(
        &dir,
        "a_long_folder_name/another_long_folder/properties_here/x.properties",
        b"a=1\n",
    );
    let screen = drawn(&state_for(&dir.0), 40, 10);
    let folder = list_lines(&screen, false)[0].clone();

    assert!(folder.contains("..."), "{folder:?}");
    assert!(folder.contains("properties_here"), "{folder:?}");
    assert!(!folder.contains("a_long_folder_name"), "{folder:?}");
    assert!(folder.contains("1 bundle"), "{folder:?}");
}

#[test]
fn an_empty_folder_says_that_nothing_was_found() {
    let dir = TempDir::new();
    let screen = drawn(&state_for(&dir.0), 80, 10);
    assert!(screen.contains("no .properties files"), "{screen}");
}

#[test]
fn problems_push_the_list_down_by_one_line() {
    let dir = TempDir::new();
    let screen = drawn(&state_for(&dir.0.join("does_not_exist")), 80, 10);
    assert!(
        screen.lines().nth(2).unwrap().contains("1 problem"),
        "{screen}"
    );
}

#[test]
fn the_list_shows_the_part_that_the_state_scrolled_to() {
    // 8 rows leave 5 for the list
    let state = state_for(&fixtures("project"));
    let state = press_all(state, &[Message::Viewport(5)]);
    let state = press_all(state, &[Message::Down; 7]);
    let screen = drawn(&state, 100, 8);
    let list = list_lines(&screen, false);

    assert_eq!(list.len(), 5, "{screen}");
    // item 3 is the first one that is shown, item 7 is selected and the last one shown
    assert!(list[0].contains("messages_de.properties"), "{screen}");
    assert!(list[4].starts_with('>'), "{screen}");
    assert!(list[4].contains("stripping.properties"), "{screen}");
}

// ── Keys for the overview ────────────────────────────────────────────────────

#[test]
fn the_keys_to_move_give_messages() {
    let cases = [
        (KeyCode::Up, Message::Up),
        (KeyCode::Down, Message::Down),
        (KeyCode::Char('k'), Message::Up),
        (KeyCode::Char('j'), Message::Down),
        (KeyCode::PageUp, Message::PageUp),
        (KeyCode::PageDown, Message::PageDown),
        (KeyCode::Home, Message::First),
        (KeyCode::End, Message::Last),
    ];
    for (code, message) in cases {
        let event = key(code, KeyModifiers::NONE);
        assert_eq!(message_for(&event), Some(message), "{code:?}");
    }
}

#[test]
fn the_release_of_a_movement_key_does_not_count() {
    let release = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Down,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert_eq!(message_for(&release), None);
}

#[test]
fn a_j_with_ctrl_and_alt_is_a_plain_j() {
    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    assert_eq!(
        message_for(&key(KeyCode::Char('j'), altgr)),
        Some(Message::Down)
    );
}

// ── The loop tells the state how many items fit ──────────────────────────────

fn height_after_running(width: u16, height: u16, state: AppState) -> usize {
    let mut terminal = terminal(width, height);
    let (result, _) = run_with(&mut terminal, state, vec![press('q')]);
    match result.unwrap().view() {
        View::Overview(overview) => overview.height(),
    }
}

#[test]
fn the_loop_tells_the_state_the_height_of_the_list() {
    let project = fixtures("project");
    assert_eq!(
        height_after_running(40, 10, state_for(&project)),
        list_height(10, false)
    );
    assert_eq!(
        height_after_running(40, 20, state_for(&project)),
        list_height(20, false)
    );
}

#[test]
fn the_height_leaves_room_for_the_line_with_problems() {
    let dir = TempDir::new();
    let state = state_for(&dir.0.join("does_not_exist"));
    assert_eq!(height_after_running(40, 10, state), list_height(10, true));
}

#[test]
fn keys_in_the_loop_move_the_selection() {
    let mut terminal = terminal(100, 20);
    let (result, _) = run_with(
        &mut terminal,
        state_for(&fixtures("project")),
        vec![
            press('j'),
            key(KeyCode::Down, KeyModifiers::NONE),
            press('j'),
            press('k'),
            press('q'),
        ],
    );
    match result.unwrap().view() {
        View::Overview(overview) => assert_eq!(overview.selected(), 2),
    }
}
