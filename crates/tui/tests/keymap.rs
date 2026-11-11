//! Specification of keys and the keymap: the text form of a key, the key of a terminal event, the
//! two layers (global and context) and the standard bindings.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::Message;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tui::{Code, Context, Key, Keymap};

fn event(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

const OVERVIEW: Context = Context::Overview;

// ── The text form of a key ───────────────────────────────────────────────────

#[test]
fn a_key_is_written_as_text_with_its_modifiers_in_front() {
    let cases = [
        (Key::char('q'), "q"),
        (Key::char('N'), "N"),
        (Key::char(' '), "space"),
        (Key::char('s').ctrl(), "ctrl+s"),
        (Key::char('x').alt(), "alt+x"),
        (Key::new(Code::Up), "up"),
        (Key::new(Code::Down).shift(), "shift+down"),
        (Key::new(Code::Left), "left"),
        (Key::new(Code::Right).ctrl(), "ctrl+right"),
        (Key::new(Code::PageUp), "pgup"),
        (Key::new(Code::PageDown), "pgdn"),
        (Key::new(Code::Home), "home"),
        (Key::new(Code::End), "end"),
        (Key::new(Code::Enter).ctrl(), "ctrl+enter"),
        (Key::new(Code::Esc), "esc"),
        (Key::new(Code::Tab), "tab"),
        (Key::char('y').ctrl().alt().shift(), "ctrl+alt+shift+y"),
    ];
    for (key, text) in cases {
        assert_eq!(key.to_string(), text, "{key:?}");
    }
}

// ── The key of an event ──────────────────────────────────────────────────────

#[test]
fn an_event_gives_the_key_that_was_pressed() {
    let none = KeyModifiers::NONE;
    let cases = [
        (event(KeyCode::Char('q'), none), Key::char('q')),
        (
            event(KeyCode::Char('s'), KeyModifiers::CONTROL),
            Key::char('s').ctrl(),
        ),
        (
            event(KeyCode::Char('x'), KeyModifiers::ALT),
            Key::char('x').alt(),
        ),
        (event(KeyCode::Up, none), Key::new(Code::Up)),
        (event(KeyCode::Down, none), Key::new(Code::Down)),
        (event(KeyCode::Left, none), Key::new(Code::Left)),
        (event(KeyCode::Right, none), Key::new(Code::Right)),
        (event(KeyCode::PageUp, none), Key::new(Code::PageUp)),
        (event(KeyCode::PageDown, none), Key::new(Code::PageDown)),
        (event(KeyCode::Home, none), Key::new(Code::Home)),
        (event(KeyCode::End, none), Key::new(Code::End)),
        (event(KeyCode::Enter, none), Key::new(Code::Enter)),
        (event(KeyCode::Esc, none), Key::new(Code::Esc)),
        (event(KeyCode::Tab, none), Key::new(Code::Tab)),
        (
            event(KeyCode::Down, KeyModifiers::SHIFT),
            Key::new(Code::Down).shift(),
        ),
    ];
    for (event, key) in cases {
        assert_eq!(Key::from_event(&event), Some(key), "{event:?}");
    }
}

#[test]
fn the_case_of_a_character_is_in_the_character_and_not_a_shift() {
    // a terminal reports Shift together with the capital letter
    let event = event(KeyCode::Char('N'), KeyModifiers::SHIFT);
    assert_eq!(Key::from_event(&event), Some(Key::char('N')));
}

#[test]
fn only_the_press_of_a_key_counts() {
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        let event = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            kind,
        ));
        assert_eq!(Key::from_event(&event), None, "{kind:?}");
    }
}

#[test]
fn a_character_with_ctrl_and_alt_is_a_plain_character() {
    // AltGr sends Ctrl and Alt together with the character
    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    assert_eq!(
        Key::from_event(&event(KeyCode::Char('@'), altgr)),
        Some(Key::char('@'))
    );
    assert_eq!(
        Key::from_event(&event(KeyCode::Char('c'), altgr)),
        Some(Key::char('c')),
        "it is not ctrl+c"
    );
    // ... but not for keys that are not characters
    assert_eq!(
        Key::from_event(&event(KeyCode::Up, altgr)),
        Some(Key::new(Code::Up).ctrl().alt())
    );
}

#[test]
fn events_without_a_nameable_key_give_none() {
    for event in [
        Event::Resize(80, 24),
        Event::FocusGained,
        Event::FocusLost,
        event(KeyCode::Insert, KeyModifiers::NONE),
        event(KeyCode::Null, KeyModifiers::NONE),
    ] {
        assert_eq!(Key::from_event(&event), None, "{event:?}");
    }
}

// ── The two layers ───────────────────────────────────────────────────────────

#[test]
fn an_empty_keymap_binds_nothing() {
    let keymap = Keymap::empty();
    assert_eq!(keymap.message(OVERVIEW, Key::char('q')), None);
    assert_eq!(keymap.keys_for(OVERVIEW, Message::Quit), []);
}

#[test]
fn a_global_binding_is_found_in_every_context() {
    let keymap = Keymap::empty().bind_global(Key::char('x'), Message::Quit);
    assert_eq!(
        keymap.message(OVERVIEW, Key::char('x')),
        Some(Message::Quit)
    );
}

#[test]
fn a_binding_of_a_context_is_found_in_that_context() {
    let keymap = Keymap::empty().bind(OVERVIEW, Key::char('x'), Message::Down);
    assert_eq!(
        keymap.message(OVERVIEW, Key::char('x')),
        Some(Message::Down)
    );
}

#[test]
fn the_key_must_match_in_its_modifiers() {
    let keymap = Keymap::empty().bind_global(Key::char('c').ctrl(), Message::Quit);
    assert_eq!(keymap.message(OVERVIEW, Key::char('c')), None);
    assert_eq!(keymap.message(OVERVIEW, Key::char('c').alt()), None);
    assert_eq!(
        keymap.message(OVERVIEW, Key::char('c').ctrl()),
        Some(Message::Quit)
    );
}

#[test]
fn the_context_wins_over_the_global_layer() {
    let keymap = Keymap::empty()
        .bind_global(Key::char('x'), Message::Quit)
        .bind(OVERVIEW, Key::char('x'), Message::Down);
    assert_eq!(
        keymap.message(OVERVIEW, Key::char('x')),
        Some(Message::Down)
    );
}

#[test]
fn binding_a_key_again_replaces_the_earlier_binding() {
    let keymap = Keymap::empty()
        .bind_global(Key::char('x'), Message::Quit)
        .bind_global(Key::char('x'), Message::Up)
        .bind(OVERVIEW, Key::char('y'), Message::Down)
        .bind(OVERVIEW, Key::char('y'), Message::Last);
    assert_eq!(keymap.message(OVERVIEW, Key::char('x')), Some(Message::Up));
    assert_eq!(
        keymap.message(OVERVIEW, Key::char('y')),
        Some(Message::Last)
    );
    assert_eq!(keymap.keys_for(OVERVIEW, Message::Quit), []);
    assert_eq!(keymap.keys_for(OVERVIEW, Message::Down), []);
}

#[test]
fn several_keys_can_give_the_same_message() {
    let keymap = Keymap::empty()
        .bind(OVERVIEW, Key::char('j'), Message::Down)
        .bind(OVERVIEW, Key::new(Code::Down), Message::Down);
    assert_eq!(
        keymap.message(OVERVIEW, Key::char('j')),
        Some(Message::Down)
    );
    assert_eq!(
        keymap.message(OVERVIEW, Key::new(Code::Down)),
        Some(Message::Down)
    );
}

#[test]
fn the_keys_of_a_message_come_in_binding_order_context_before_global() {
    let keymap = Keymap::empty()
        .bind_global(Key::char('q'), Message::Quit)
        .bind(OVERVIEW, Key::char('z'), Message::Quit)
        .bind_global(Key::char('c').ctrl(), Message::Quit)
        .bind(OVERVIEW, Key::char('y'), Message::Quit);
    assert_eq!(
        keymap.keys_for(OVERVIEW, Message::Quit),
        [
            Key::char('z'),
            Key::char('y'),
            Key::char('q'),
            Key::char('c').ctrl()
        ]
    );
}

#[test]
fn a_key_that_the_context_binds_elsewhere_is_not_a_key_of_the_global_message() {
    let keymap = Keymap::empty()
        .bind_global(Key::char('x'), Message::Quit)
        .bind_global(Key::char('q'), Message::Quit)
        .bind(OVERVIEW, Key::char('x'), Message::Down);
    assert_eq!(keymap.keys_for(OVERVIEW, Message::Quit), [Key::char('q')]);
    assert_eq!(keymap.keys_for(OVERVIEW, Message::Down), [Key::char('x')]);
}

// ── The standard keymap ──────────────────────────────────────────────────────

#[test]
fn the_standard_keymap_has_the_keys_of_the_overview() {
    let keymap = Keymap::default();
    let cases = [
        (Key::char('q'), Message::Quit),
        (Key::char('c').ctrl(), Message::Quit),
        (Key::new(Code::Up), Message::Up),
        (Key::char('k'), Message::Up),
        (Key::new(Code::Down), Message::Down),
        (Key::char('j'), Message::Down),
        (Key::new(Code::PageUp), Message::PageUp),
        (Key::new(Code::PageDown), Message::PageDown),
        (Key::new(Code::Home), Message::First),
        (Key::new(Code::End), Message::Last),
    ];
    for (key, message) in cases {
        assert_eq!(keymap.message(OVERVIEW, key), Some(message), "{key}");
    }
}

#[test]
fn the_standard_keymap_leaves_other_keys_unbound() {
    let keymap = Keymap::default();
    for key in [
        Key::char('x'),
        Key::char('c'),
        Key::char('q').ctrl(),
        Key::new(Code::Enter).ctrl(),
        Key::new(Code::Tab),
        Key::new(Code::Up).shift(),
    ] {
        assert_eq!(keymap.message(OVERVIEW, key), None, "{key}");
    }
}

#[test]
fn the_standard_keys_to_quit_are_q_and_ctrl_c_in_that_order() {
    assert_eq!(
        Keymap::default().keys_for(OVERVIEW, Message::Quit),
        [Key::char('q'), Key::char('c').ctrl()]
    );
}

#[test]
fn the_standard_keymap_has_the_keys_of_the_preview() {
    let keymap = Keymap::default();
    let cases = [
        (Key::char('v'), Message::TogglePreview),
        (Key::new(Code::Up).ctrl(), Message::PreviewUp),
        (Key::new(Code::Down).ctrl(), Message::PreviewDown),
        (Key::char('u').ctrl(), Message::PreviewHalfPageUp),
        (Key::char('d').ctrl(), Message::PreviewHalfPageDown),
    ];
    for (key, message) in cases {
        assert_eq!(keymap.message(OVERVIEW, key), Some(message), "{key}");
    }
    // the plain keys still move the cursor
    assert_eq!(
        keymap.message(OVERVIEW, Key::new(Code::Up)),
        Some(Message::Up)
    );
    assert_eq!(keymap.message(OVERVIEW, Key::char('d')), None);
}

#[test]
fn enter_edits_in_the_overview() {
    let keymap = Keymap::default();
    assert_eq!(
        keymap.message(OVERVIEW, Key::new(Code::Enter)),
        Some(Message::Edit)
    );
    assert_eq!(
        Key::from_event(&event(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Key::new(Code::Enter))
    );
}

// ── In front of the view: the alert ──────────────────────────────────────────

const ALERT: Context = Context::Alert;

#[test]
fn escape_enter_and_space_dismiss_an_alert() {
    let keymap = Keymap::default();
    for key in [Key::new(Code::Esc), Key::new(Code::Enter), Key::char(' ')] {
        assert_eq!(
            keymap.message(ALERT, key),
            Some(Message::DismissAlert),
            "{key}"
        );
    }
    assert_eq!(
        keymap.keys_for(ALERT, Message::DismissAlert),
        [Key::new(Code::Esc), Key::new(Code::Enter), Key::char(' ')]
    );
}

#[test]
fn the_keys_of_the_view_do_not_work_through_an_alert() {
    let keymap = Keymap::default();
    for key in [
        Key::char('j'),
        Key::char('k'),
        Key::new(Code::Up),
        Key::new(Code::Down),
        Key::new(Code::PageDown),
        Key::new(Code::Home),
        Key::new(Code::End),
        Key::char('u').ctrl(),
        Key::char('d').ctrl(),
    ] {
        assert_eq!(keymap.message(ALERT, key), None, "{key}");
    }
    for message in [Message::Down, Message::Edit, Message::TogglePreview] {
        assert_eq!(keymap.keys_for(ALERT, message), [], "{message:?}");
    }
}

#[test]
fn the_global_keys_still_work_through_an_alert() {
    let keymap = Keymap::default();
    assert_eq!(keymap.message(ALERT, Key::char('q')), Some(Message::Quit));
    assert_eq!(
        keymap.message(ALERT, Key::char('c').ctrl()),
        Some(Message::Quit)
    );
}

#[test]
fn the_keys_of_the_overview_keep_their_meaning() {
    let keymap = Keymap::default();
    assert_eq!(
        keymap.message(OVERVIEW, Key::new(Code::Enter)),
        Some(Message::Edit)
    );
    assert_eq!(
        keymap.message(OVERVIEW, Key::char(' ')),
        Some(Message::ToggleMark)
    );
    assert_eq!(
        keymap.message(OVERVIEW, Key::new(Code::Esc)),
        Some(Message::ClearMarks)
    );
}

// ── Function keys ────────────────────────────────────────────────────────────

#[test]
fn function_keys_are_keys() {
    let none = KeyModifiers::NONE;
    assert_eq!(
        Key::from_event(&event(KeyCode::F(1), none)),
        Some(Key::new(Code::F(1)))
    );
    assert_eq!(
        Key::from_event(&event(KeyCode::F(12), KeyModifiers::CONTROL)),
        Some(Key::new(Code::F(12)).ctrl())
    );
    assert_eq!(Key::new(Code::F(1)).to_string(), "f1");
    assert_eq!(Key::new(Code::F(12)).ctrl().to_string(), "ctrl+f12");
}

// ── Keys for text ────────────────────────────────────────────────────────────

#[test]
fn backspace_delete_and_shift_tab_are_keys() {
    let none = KeyModifiers::NONE;
    assert_eq!(
        Key::from_event(&event(KeyCode::Backspace, none)),
        Some(Key::new(Code::Backspace))
    );
    assert_eq!(
        Key::from_event(&event(KeyCode::Delete, none)),
        Some(Key::new(Code::Delete))
    );
    // a terminal reports Shift+Tab as its own key
    assert_eq!(
        Key::from_event(&event(KeyCode::BackTab, KeyModifiers::SHIFT)),
        Some(Key::new(Code::Tab).shift())
    );
    assert_eq!(Key::new(Code::Backspace).to_string(), "backspace");
    assert_eq!(Key::new(Code::Delete).to_string(), "delete");
    assert_eq!(Key::new(Code::Tab).shift().to_string(), "shift+tab");
}

#[test]
fn only_the_command_line_takes_text() {
    assert!(Context::CommandLine.takes_text());
    assert!(!Context::Overview.takes_text());
    assert!(!Context::Alert.takes_text());
}

#[test]
fn the_standard_keymap_has_the_keys_of_the_command_line() {
    use app::TextEdit;
    let keymap = Keymap::default();
    let line = Context::CommandLine;
    assert_eq!(
        keymap.message(OVERVIEW, Key::char(':')),
        Some(Message::OpenCommandLine)
    );
    let cases = [
        (Key::new(Code::Esc), Message::CloseCommandLine),
        (Key::new(Code::Enter), Message::RunCommand),
        (Key::new(Code::Up), Message::NextCandidate),
        (Key::new(Code::Down), Message::PreviousCandidate),
        (Key::new(Code::Tab), Message::CompleteCommand),
        (
            Key::new(Code::Backspace),
            Message::Text(TextEdit::Backspace),
        ),
        (Key::new(Code::Delete), Message::Text(TextEdit::Delete)),
        (Key::new(Code::Left), Message::Text(TextEdit::Left)),
        (Key::new(Code::Right), Message::Text(TextEdit::Right)),
        (Key::new(Code::Home), Message::Text(TextEdit::Home)),
        (Key::new(Code::End), Message::Text(TextEdit::End)),
        (Key::char('u').ctrl(), Message::Text(TextEdit::Clear)),
    ];
    for (key, message) in cases {
        assert_eq!(keymap.message(line, key), Some(message), "{key}");
    }
    // the keys of the overview do not reach through
    for key in [Key::char('j'), Key::new(Code::PageDown), Key::char('v')] {
        assert_eq!(keymap.message(line, key), None, "{key}");
    }
    assert_eq!(
        keymap.keys_for(line, Message::NextCandidate),
        [Key::new(Code::Up)]
    );
}
