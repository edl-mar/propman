//! Specification of the first screen, the keys and the event loop (see `tui`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use app::{AppState, Message, Session};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tui::{LoopError, message_for, render, run_loop};

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
        key(KeyCode::Up, KeyModifiers::NONE),
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
