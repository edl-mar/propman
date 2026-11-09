//! Specification of the first screen, the keys and the event loop (see `tui`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use app::{AppState, EditorOutcome, Message, Session, View, preview, update};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use tui::{
    Code, Context, Key, Keymap, LoopError, Theme, Ui, list_height, message_for as message_with,
    render as render_with, run_loop as run_loop_with,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn fixtures(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn state_with_preview(folder: &Path) -> AppState {
    let (session, problems) = Session::open(folder);
    AppState::new(session, problems)
}

/// The state with the preview switched off, so that the tests of the list see the whole width.
fn state_for(folder: &Path) -> AppState {
    update(state_with_preview(folder), Message::TogglePreview).0
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

fn drawn_with(ui: &Ui, state: &AppState, width: u16, height: u16) -> String {
    let mut terminal = terminal(width, height);
    terminal
        .draw(|frame| render_with(ui, state, frame))
        .unwrap();
    screen(&terminal)
}

/// Drawn with the standard keymap and theme.
fn drawn(state: &AppState, width: u16, height: u16) -> String {
    drawn_with(&Ui::default(), state, width, height)
}

fn render(state: &AppState, frame: &mut ratatui::Frame) {
    render_with(&Ui::default(), state, frame);
}

/// The message of an event with the standard keymap, in the overview.
fn message_for(event: &Event) -> Option<Message> {
    message_with(&Ui::default(), Context::Overview, event)
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
    assert!(screen.contains("q/ctrl+c: quit"), "{screen}");
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
        key(KeyCode::Enter, KeyModifiers::CONTROL),
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
    run_with_ui(&Ui::default(), terminal, state, events)
}

fn run_with_ui(
    ui: &Ui,
    terminal: &mut Terminal<TestBackend>,
    state: AppState,
    events: Vec<Event>,
) -> (Result<AppState, LoopError<Infallible>>, usize) {
    let mut events = events.into_iter();
    let mut asked = 0;
    let result = run_loop_with(
        terminal,
        ui,
        state,
        || {
            asked += 1;
            events
                .next()
                .ok_or_else(|| io::Error::other("no more events"))
        },
        |_, _| EditorOutcome::NoEditor,
    );
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
        .fold(state, |state, &message| update(state, message).0)
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

// ── The keymap decides ───────────────────────────────────────────────────────

const OVERVIEW: Context = Context::Overview;

fn ui_with(keymap: Keymap) -> Ui {
    Ui {
        keymap,
        theme: Theme::default(),
    }
}

#[test]
fn the_messages_come_from_the_keymap_of_the_ui() {
    let ui = ui_with(
        Keymap::empty()
            .bind(OVERVIEW, Key::char('x'), Message::Down)
            .bind(OVERVIEW, Key::new(Code::Esc), Message::Quit),
    );
    let view = Context::Overview;
    assert_eq!(message_with(&ui, view, &press('x')), Some(Message::Down));
    assert_eq!(
        message_with(&ui, view, &key(KeyCode::Esc, KeyModifiers::NONE)),
        Some(Message::Quit)
    );
    // the standard keys are gone, the keymap is the only place that knows them
    assert_eq!(message_with(&ui, view, &press('q')), None);
    assert_eq!(message_with(&ui, view, &press('j')), None);
}

#[test]
fn only_the_press_of_a_bound_key_gives_a_message() {
    let ui = ui_with(Keymap::empty().bind_global(Key::char('x'), Message::Quit));
    let release = Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('x'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert_eq!(message_with(&ui, Context::Overview, &release), None);
    assert_eq!(
        message_with(&ui, Context::Overview, &Event::FocusLost),
        None
    );
}

#[test]
fn altgr_still_gives_the_plain_character_with_a_custom_keymap() {
    let ui = ui_with(Keymap::empty().bind_global(Key::char('@'), Message::Down));
    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    assert_eq!(
        message_with(&ui, Context::Overview, &key(KeyCode::Char('@'), altgr)),
        Some(Message::Down)
    );
}

#[test]
fn the_loop_follows_the_keymap_of_the_ui() {
    let ui = ui_with(Keymap::empty().bind_global(Key::char('x'), Message::Quit));
    let mut terminal = terminal(120, 10);
    let (result, asked) = run_with_ui(
        &ui,
        &mut terminal,
        state_for(&fixtures("project")),
        vec![
            press('q'),
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            press('x'),
        ],
    );
    assert!(result.unwrap().should_quit());
    assert_eq!(asked, 3, "q and ctrl+c are not bound in this keymap");
}

// ── The line with the keys ───────────────────────────────────────────────────

fn last_line(screen: &str) -> String {
    screen
        .lines()
        .last()
        .unwrap_or_default()
        .trim_end()
        .to_owned()
}

#[test]
fn the_line_with_the_keys_comes_from_the_keymap() {
    let footer = last_line(&drawn(&state_for(&fixtures("project")), 140, 10));
    assert!(footer.contains("q/ctrl+c: quit"), "{footer}");
    assert!(footer.contains("up/k/down/j: move"), "{footer}");
    assert!(footer.contains("pgup/pgdn: page"), "{footer}");
    assert!(footer.contains("home/end: ends"), "{footer}");
}

#[test]
fn a_changed_binding_changes_the_line_with_the_keys() {
    let ui = ui_with(Keymap::default().bind_global(Key::char('x'), Message::Quit));
    let footer = last_line(&drawn_with(&ui, &state_for(&fixtures("project")), 120, 10));
    assert!(footer.contains("q/ctrl+c/x: quit"), "{footer}");
}

#[test]
fn what_is_not_bound_is_not_named() {
    let ui = ui_with(Keymap::empty().bind_global(Key::char('x'), Message::Quit));
    let footer = last_line(&drawn_with(&ui, &state_for(&fixtures("project")), 120, 10));
    assert_eq!(footer, "x: quit");
}

#[test]
fn only_the_keys_of_a_movement_that_are_bound_are_named() {
    let ui = ui_with(
        Keymap::empty()
            .bind(OVERVIEW, Key::char('s'), Message::Down)
            .bind_global(Key::char('q'), Message::Quit),
    );
    let footer = last_line(&drawn_with(&ui, &state_for(&fixtures("project")), 120, 10));
    assert_eq!(footer, "q: quit   s: move");
}

// ── The theme decides ────────────────────────────────────────────────────────

fn loud_theme() -> Theme {
    Theme {
        folder: Style::new().fg(Color::Red),
        bundle: Style::new().fg(Color::Green),
        file: Style::new().fg(Color::Blue),
        uncertain: Style::new().fg(Color::Magenta),
        excluded: Style::new().fg(Color::Cyan),
        selected: Style::new().add_modifier(Modifier::UNDERLINED),
        alert: Style::new().fg(Color::LightRed),
        line_number: Style::new().fg(Color::Yellow),
        ending: Style::new().fg(Color::LightBlue),
    }
}

fn themed(theme: Theme) -> Ui {
    Ui {
        keymap: Keymap::default(),
        theme,
    }
}

/// The style of the first character of `needle` on the first line that contains it.
fn style_at(terminal: &Terminal<TestBackend>, needle: &str) -> Style {
    let screen = screen(terminal);
    let (y, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} is not on the screen:\n{screen}"));
    let x = line[..line.find(needle).unwrap()].chars().count();
    terminal.backend().buffer()[(x as u16, y as u16)].style()
}

fn drawn_terminal(ui: &Ui, state: &AppState) -> Terminal<TestBackend> {
    let mut terminal = terminal(110, 30);
    terminal
        .draw(|frame| render_with(ui, state, frame))
        .unwrap();
    terminal
}

#[test]
fn a_theme_changes_the_style_and_never_the_text() {
    let state = state_for(&fixtures("project"));
    assert_eq!(
        drawn_with(&themed(loud_theme()), &state, 110, 30),
        drawn(&state, 110, 30)
    );
}

#[test]
fn every_kind_of_item_is_drawn_in_the_style_of_its_role() {
    let state = press_all(state_for(&fixtures("project")), &[Message::Down]);
    let terminal = drawn_terminal(&themed(loud_theme()), &state);
    // the folder is not selected any more, the bundle is
    assert_eq!(style_at(&terminal, "3 bundles").fg, Some(Color::Red));
    assert_eq!(
        style_at(&terminal, "stripping  ").fg,
        Some(Color::Green),
        "a bundle"
    );
    assert_eq!(
        style_at(&terminal, "messages.properties").fg,
        Some(Color::Blue),
        "a file"
    );
}

#[test]
fn the_selected_item_gets_the_style_for_the_selection_on_top_of_its_role() {
    let state = state_for(&fixtures("project"));
    let terminal = drawn_terminal(&themed(loud_theme()), &state);
    let folder = style_at(&terminal, "3 bundles");
    assert_eq!(folder.fg, Some(Color::Red), "the role stays");
    assert!(folder.add_modifier.contains(Modifier::UNDERLINED));
    assert!(
        !folder.add_modifier.contains(Modifier::REVERSED),
        "the selection style is the one of the theme"
    );
    let other = style_at(&terminal, "messages.properties");
    assert!(!other.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn an_uncertain_file_and_an_excluded_bundle_get_their_styles_on_top_of_their_roles() {
    let dir = TempDir::new();
    write(&dir, "single_file_de.properties", b"a=1\n");
    let terminal = drawn_terminal(&themed(loud_theme()), &state_for(&dir.0));
    assert_eq!(
        style_at(&terminal, "single_file_de.properties").fg,
        Some(Color::Magenta)
    );

    let (mut session, problems) = Session::open(&fixtures("project"));
    let bundle = session.store().bundles()[0];
    session.exclude_bundle(bundle, true);
    let state = press_all(AppState::new(session, problems), &[Message::Last]);
    let terminal = drawn_terminal(&themed(loud_theme()), &state);
    assert_eq!(style_at(&terminal, "messages  ").fg, Some(Color::Cyan));
    assert_eq!(style_at(&terminal, "stripping  ").fg, Some(Color::Green));
}

#[test]
fn the_standard_theme_marks_the_selection_without_colour() {
    let state = state_for(&fixtures("project"));
    let terminal = drawn_terminal(&Ui::default(), &state);
    assert!(
        style_at(&terminal, "3 bundles")
            .add_modifier
            .contains(Modifier::REVERSED)
    );
    assert!(
        !style_at(&terminal, "messages.properties")
            .add_modifier
            .contains(Modifier::REVERSED)
    );
}

// ── The preview ──────────────────────────────────────────────────────────────

/// `a.properties` as the only file, selected, with the height of a terminal of `height` rows
/// told, as the loop does. The preview is on.
fn on_a_file(bytes: &[u8], height: u16) -> (AppState, TempDir) {
    let dir = TempDir::new();
    write(&dir, "a.properties", bytes);
    let state = press_all(
        state_with_preview(&dir.0),
        &[
            Message::Viewport(list_height(height, false)),
            Message::Down,
            Message::Down,
        ],
    );
    (state, dir)
}

/// The first line of the screen that contains `needle`.
fn row_with(screen: &str, needle: &str) -> String {
    screen
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} is not on the screen:\n{screen}"))
        .to_owned()
}

fn has_separator(screen: &str) -> bool {
    screen.contains('|')
}

#[test]
fn the_preview_shows_the_header_and_the_lines_of_the_selected_file() {
    let (state, _dir) = on_a_file(b"x=1\r\ny=2\nz=3", 10);
    let screen = drawn(&state, 110, 10);
    assert!(
        screen.contains("| a.properties  UTF-8  3 lines"),
        "{screen}"
    );
    let first = row_with(&screen, "| 1 x=1");
    assert!(first.contains("CRLF"), "{screen}");
    let second = row_with(&screen, "| 2 y=2");
    assert!(second.trim_end().ends_with("LF"), "{screen}");
    assert!(!second.contains("CRLF"), "{screen}");
    assert!(row_with(&screen, "| 3 z=3").contains("(none)"), "{screen}");
}

#[test]
fn the_list_gets_the_left_half_and_the_separator_runs_down_the_pane() {
    let (state, _dir) = on_a_file(b"x=1\n", 10);
    let screen = drawn(&state, 110, 10);
    let rows: Vec<&str> = screen.lines().collect();
    // below the title and the count, above the line with the keys
    for (y, row) in rows.iter().enumerate().take(rows.len() - 1).skip(2) {
        assert_eq!(row.chars().nth(55), Some('|'), "row {y}:\n{screen}");
    }
    // the list is cut at the separator: nothing of it is written behind
    let file = row_with(&screen, "(default)");
    assert!(file.find("(default)").unwrap() < 55, "{screen}");
    assert!(!file[..55].contains('|'), "{screen}");
}

#[test]
fn the_numbers_are_as_wide_as_the_biggest_one() {
    let lines: Vec<u8> = (1..=30)
        .flat_map(|n| format!("l{n:02}=1\n").into_bytes())
        .collect();
    let (state, _dir) = on_a_file(&lines, 12);
    let state = press_all(
        state,
        &[
            Message::PreviewDown,
            Message::PreviewDown,
            Message::PreviewDown,
        ],
    );
    let screen = drawn(&state, 110, 12);
    // 12 rows: 9 for the list, 1 for the header, 8 lines from the fourth on
    assert!(screen.contains("|  4 l04=1"), "{screen}");
    assert!(screen.contains("| 11 l11=1"), "{screen}");
    assert!(!screen.contains("l03=1"), "{screen}");
    assert!(!screen.contains("l12=1"), "{screen}");
}

#[test]
fn the_preview_is_drawn_from_100_columns_on_and_not_below() {
    let (state, _dir) = on_a_file(b"x=1\n", 10);
    assert!(!has_separator(&drawn(&state, 99, 10)));
    assert!(has_separator(&drawn(&state, 100, 10)));
}

#[test]
fn a_switched_off_preview_is_not_drawn_and_a_narrow_screen_keeps_the_setting() {
    let (state, _dir) = on_a_file(b"x=1\n", 10);
    let state = press_all(state, &[Message::TogglePreview]);
    assert!(!has_separator(&drawn(&state, 110, 10)));
    let state = press_all(state, &[Message::TogglePreview]);
    assert!(!has_separator(&drawn(&state, 80, 10)));
    assert!(has_separator(&drawn(&state, 110, 10)));
}

#[test]
fn for_a_folder_and_a_bundle_the_pane_asks_for_a_file() {
    let state = state_with_preview(&fixtures("project"));
    let screen = drawn(&state, 110, 10);
    assert!(has_separator(&screen), "{screen}");
    assert!(screen.contains("select a file"), "{screen}");
    let state = press_all(state, &[Message::Down]);
    assert!(drawn(&state, 110, 10).contains("select a file"));
}

#[test]
fn a_line_that_is_too_long_is_cut_and_keeps_its_ending() {
    let long = format!("k={}\n", "x".repeat(200));
    let (state, _dir) = on_a_file(long.as_bytes(), 10);
    let screen = drawn(&state, 100, 10);
    let row = row_with(&screen, "| 1 k=xxx");
    assert!(row.contains("..."), "{screen}");
    assert!(row.trim_end().ends_with("LF"), "{screen}");
    assert!(row.chars().count() <= 100, "{screen}");
}

#[test]
fn control_characters_are_drawn_as_a_dot() {
    let (state, _dir) = on_a_file(b"a\tb=1\n", 10);
    let screen = drawn(&state, 110, 10);
    assert!(screen.contains("| 1 a\u{b7}b=1"), "{screen}");
    assert!(!screen.contains('\t'), "{screen}");
}

#[test]
fn an_empty_file_shows_only_the_header() {
    let (state, _dir) = on_a_file(b"", 10);
    let screen = drawn(&state, 110, 10);
    assert!(
        screen.contains("| a.properties  UTF-8  0 lines"),
        "{screen}"
    );
}

#[test]
fn the_line_with_the_keys_names_the_keys_of_the_preview() {
    let footer = last_line(&drawn(&state_with_preview(&fixtures("project")), 140, 10));
    assert!(footer.contains("space: preview"), "{footer}");
    assert!(footer.contains("ctrl+u/ctrl+d: scroll preview"), "{footer}");
}

#[test]
fn drawing_the_preview_survives_a_tiny_area() {
    let (state, _dir) = on_a_file(b"x=1\ny=2\n", 10);
    for (width, height) in [(100, 0), (100, 1), (100, 2), (101, 3), (1000, 2), (100, 40)] {
        let mut terminal = terminal(width, height);
        terminal
            .draw(|frame| render_with(&Ui::default(), &state, frame))
            .unwrap();
    }
}

#[test]
fn the_theme_styles_the_numbers_and_the_endings_of_the_preview() {
    let (state, _dir) = on_a_file(b"x=1\r\ny=2\n", 10);
    let ui = themed(loud_theme());
    let terminal = drawn_terminal(&ui, &state);
    assert_eq!(style_at(&terminal, "1 x=1").fg, Some(Color::Yellow));
    assert_eq!(style_at(&terminal, "CRLF").fg, Some(Color::LightBlue));
    assert_eq!(
        drawn_with(&ui, &state, 110, 30),
        drawn(&state, 110, 30),
        "a theme never changes the text"
    );
}

#[test]
fn the_keys_of_the_preview_work_in_the_loop() {
    let lines: Vec<u8> = (1..=30)
        .flat_map(|n| format!("l{n:02}=1\n").into_bytes())
        .collect();
    let dir = TempDir::new();
    write(&dir, "a.properties", &lines);
    let mut terminal = terminal(100, 20);
    let (result, _) = run_with(
        &mut terminal,
        state_with_preview(&dir.0),
        vec![
            press('j'),
            press('j'),
            key(KeyCode::Down, KeyModifiers::CONTROL),
            key(KeyCode::Down, KeyModifiers::CONTROL),
            press('q'),
        ],
    );
    match result.unwrap().view() {
        View::Overview(overview) => assert_eq!(overview.preview_scroll(), 2),
    }

    let (result, _) = run_with(
        &mut terminal,
        state_with_preview(&dir.0),
        vec![press(' '), press('q')],
    );
    match result.unwrap().view() {
        View::Overview(overview) => assert!(!overview.preview_on()),
    }
}

// ── Editing a file in the editor ─────────────────────────────────────────────

/// The loop with a fake editor, which is told the path and answers with what `edit` returns.
fn run_with_editor(
    terminal: &mut Terminal<TestBackend>,
    state: AppState,
    events: Vec<Event>,
    mut edit: impl FnMut(&Path) -> EditorOutcome,
) -> Result<AppState, LoopError<Infallible>> {
    let mut events = events.into_iter();
    run_loop_with(
        terminal,
        &Ui::default(),
        state,
        || {
            events
                .next()
                .ok_or_else(|| io::Error::other("no more events"))
        },
        |_, path| edit(path),
    )
}

fn enter() -> Event {
    key(KeyCode::Enter, KeyModifiers::NONE)
}

#[test]
fn enter_on_a_file_runs_the_editor_on_it_and_reads_the_file_again() {
    let dir = TempDir::new();
    write(&dir, "a.properties", b"a=1\n");
    let mut terminal = terminal(100, 20);
    let mut calls = Vec::new();

    let state = run_with_editor(
        &mut terminal,
        state_with_preview(&dir.0),
        vec![press('j'), press('j'), enter(), press('q')],
        |path| {
            calls.push(path.to_path_buf());
            std::fs::write(path, b"a=1\nb=2\n").unwrap();
            EditorOutcome::Done
        },
    )
    .unwrap();

    assert_eq!(calls, [dir.0.join("a.properties")]);
    assert_eq!(preview(&state).unwrap().total, 2);
}

#[test]
fn enter_on_a_folder_does_not_run_the_editor() {
    let mut terminal = terminal(100, 20);
    let mut calls = 0;
    run_with_editor(
        &mut terminal,
        state_for(&fixtures("project")),
        vec![enter(), press('j'), enter(), press('q')],
        |_| {
            calls += 1;
            EditorOutcome::Done
        },
    )
    .unwrap();
    assert_eq!(calls, 0, "the folder and then the bundle");
}

// ── The alert ────────────────────────────────────────────────────────────────

/// The state, with an alert about the outcome of an editor. Any file id will do for an outcome
/// that only brings an alert: the id is not looked at.
fn with_alert(state: AppState, outcome: EditorOutcome) -> AppState {
    let file = state.session().store().files()[0];
    update(state, Message::EditorClosed { file, outcome }).0
}

fn rows(screen: &str) -> Vec<&str> {
    screen.lines().collect()
}

#[test]
fn the_context_is_the_alert_as_long_as_there_is_one() {
    let state = state_for(&fixtures("project"));
    assert_eq!(Context::of(&state), Context::Overview);
    let state = with_alert(state, EditorOutcome::NoEditor);
    assert_eq!(Context::of(&state), Context::Alert);
    let state = update(state, Message::DismissAlert).0;
    assert_eq!(Context::of(&state), Context::Overview);
}

#[test]
fn an_alert_is_a_box_at_the_top_with_the_title_error_and_the_text() {
    let state = with_alert(state_for(&fixtures("project")), EditorOutcome::NoEditor);
    let screen = drawn(&state, 110, 12);
    let rows = rows(&screen);
    assert!(rows[0].contains("error"), "{screen}");
    assert!(
        rows[1].contains("no editor: set VISUAL or EDITOR"),
        "{screen}"
    );
    assert!(
        rows[0].starts_with(|c: char| !c.is_whitespace()),
        "the box starts at the left edge: {screen}"
    );
    // the box is as high as its text and its border: the third row is the bottom border
    assert!(!rows[2].contains("bundle"), "{screen}");
    assert!(
        rows[3].contains("files"),
        "the list goes on below the box: {screen}"
    );
}

#[test]
fn a_long_alert_is_wrapped_inside_the_box() {
    let state = with_alert(
        state_for(&fixtures("project")),
        EditorOutcome::Failed(Some(1)),
    );
    let screen = drawn(&state, 24, 12);
    let rows = rows(&screen);
    let first = rows
        .iter()
        .position(|row| row.contains("the editor"))
        .unwrap();
    let last = rows.iter().position(|row| row.contains("code 1")).unwrap();
    assert!(first < last, "{screen}");
    assert!(
        last <= 4,
        "the box is not higher than its text needs: {screen}"
    );
}

#[test]
fn an_alert_does_not_move_the_list() {
    let plain = state_for(&fixtures("project"));
    let plain_screen = drawn(&plain, 110, 14);
    let alert_screen = drawn(&with_alert(plain, EditorOutcome::NoEditor), 110, 14);
    let plain_rows = rows(&plain_screen);
    let alert_rows = rows(&alert_screen);
    let last = plain_rows.len() - 1;
    // below the box and above the line with the keys, the screen is the same
    assert_eq!(plain_rows[3..last], alert_rows[3..last]);
}

#[test]
fn the_line_with_the_keys_names_only_what_works_while_there_is_an_alert() {
    let state = with_alert(
        state_with_preview(&fixtures("project")),
        EditorOutcome::NoEditor,
    );
    let footer = last_line(&drawn(&state, 140, 10));
    assert!(footer.contains("esc/enter/space: dismiss"), "{footer}");
    assert!(footer.contains("q/ctrl+c: quit"), "{footer}");
    for not_there in ["move", "edit", "page", "preview"] {
        assert!(!footer.contains(not_there), "{not_there}: {footer}");
    }
}

#[test]
fn the_theme_styles_the_alert_and_never_changes_its_text() {
    let state = with_alert(state_for(&fixtures("project")), EditorOutcome::NoEditor);
    let ui = themed(loud_theme());
    let terminal = drawn_terminal(&ui, &state);
    assert_eq!(
        style_at(&terminal, "no editor: set VISUAL").fg,
        Some(Color::LightRed)
    );
    assert_eq!(style_at(&terminal, "error").fg, Some(Color::LightRed));
    assert_eq!(
        drawn_with(&ui, &state, 110, 30),
        drawn(&state, 110, 30),
        "a theme never changes the text"
    );
}

#[test]
fn drawing_an_alert_survives_a_tiny_area() {
    let state = with_alert(
        state_with_preview(&fixtures("project")),
        EditorOutcome::Failed(Some(1)),
    );
    for (width, height) in [
        (0, 0),
        (1, 1),
        (5, 1),
        (20, 3),
        (100, 2),
        (110, 40),
        (3, 30),
    ] {
        let mut terminal = terminal(width, height);
        terminal
            .draw(|frame| render_with(&Ui::default(), &state, frame))
            .unwrap();
    }
}

#[test]
fn while_an_alert_is_shown_the_loop_leaves_the_selection_alone_until_it_is_dismissed() {
    for dismiss in [
        key(KeyCode::Esc, KeyModifiers::NONE),
        key(KeyCode::Enter, KeyModifiers::NONE),
        press(' '),
    ] {
        let dir = TempDir::new();
        write(&dir, "a.properties", b"a=1\n");
        write(&dir, "b.properties", b"b=1\n");
        let mut terminal = terminal(100, 20);
        let state = run_with_editor(
            &mut terminal,
            state_with_preview(&dir.0),
            // the list: folder, bundle a, a, bundle b, b; the editor fails on a, `j` is blocked,
            // and only after the dismissal it moves on
            vec![
                press('j'),
                press('j'),
                enter(),
                press('j'),
                dismiss.clone(),
                press('j'),
                press('q'),
            ],
            |_| EditorOutcome::NoEditor,
        )
        .unwrap();
        match state.view() {
            View::Overview(overview) => assert_eq!(overview.selected(), 3, "{dismiss:?}"),
        }
        assert_eq!(state.alert(), None, "{dismiss:?}");
    }
}

#[test]
fn an_editor_that_could_not_run_is_shown_in_the_box_and_the_list_keeps_its_height() {
    let lines: Vec<u8> = (1..=30)
        .flat_map(|n| format!("l{n:02}=1\n").into_bytes())
        .collect();
    let dir = TempDir::new();
    write(&dir, "a.properties", &lines);
    let mut terminal = terminal(100, 10);

    // the loop stops with an error when the events are used up; the screen is the last one drawn
    let result = run_with_editor(
        &mut terminal,
        state_with_preview(&dir.0),
        vec![press('j'), press('j'), enter(), Event::Resize(100, 10)],
        |_| EditorOutcome::NoEditor,
    );
    assert!(matches!(result, Err(LoopError::Events(_))));

    let screen = screen(&terminal);
    assert!(
        row_with(&screen, "no editor: set VISUAL or EDITOR").contains("no editor"),
        "{screen}"
    );
    // 10 rows: the title, the count, the keys and 7 for the list: 6 lines of the file, as always
    assert!(screen.contains("l06=1"), "{screen}");
    assert!(!screen.contains("l07=1"), "{screen}");
}

#[test]
fn the_line_with_the_keys_names_the_key_to_edit() {
    let footer = last_line(&drawn(&state_with_preview(&fixtures("project")), 140, 10));
    assert!(footer.contains("enter: edit"), "{footer}");
}
