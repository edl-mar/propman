use std::collections::HashMap;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::messages::Message;

/// Mode-specific keybinding maps. Keys not present in the active map fall
/// through to `TextInput` / `FilterInput` (in text modes) or are ignored
/// (in Normal mode).
pub struct Keybindings {
    pub normal:       HashMap<KeyEvent, Message>,
    pub editing:      HashMap<KeyEvent, Message>,
    pub continuation: HashMap<KeyEvent, Message>,
    pub key_naming:   HashMap<KeyEvent, Message>,
    pub key_renaming: HashMap<KeyEvent, Message>,
    pub deleting:     HashMap<KeyEvent, Message>,
    pub filter:       HashMap<KeyEvent, Message>,
}

pub fn default_keybindings() -> Keybindings {
    macro_rules! map {
        ( $( ($code:expr, $mods:expr) => $msg:expr ),* $(,)? ) => {{
            let mut m = HashMap::new();
            $( m.insert(KeyEvent::new($code, $mods), $msg); )*
            m
        }};
    }

    let none  = KeyModifiers::NONE;
    let ctrl  = KeyModifiers::CONTROL;

    let normal = map![
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
        (KeyCode::Left,           none) => Message::MoveCursorLeft,
        (KeyCode::Right,          none) => Message::MoveCursorRight,
        (KeyCode::Char('k'),      none) => Message::MoveCursorUp,
        (KeyCode::Char('j'),      none) => Message::MoveCursorDown,
        (KeyCode::Char('h'),      none) => Message::MoveCursorLeft,
        (KeyCode::Char('l'),      none) => Message::MoveCursorRight,
        (KeyCode::PageUp,         none) => Message::PageUp,
        (KeyCode::PageDown,       none) => Message::PageDown,
        (KeyCode::Enter,          none) => Message::StartEdit,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('/'),      none) => Message::FocusFilter,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('q'),      none) => Message::Quit,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
        (KeyCode::Char('n'),      none) => Message::NewKey,
        (KeyCode::Char('d'),      none) => Message::DeleteKey,
    ];

    let editing = map![
        (KeyCode::Enter,          none) => Message::CommitEdit,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
        // `\` enters Continuation sub-mode instead of typing a literal backslash.
        // A literal `\` can still be entered via any other means (e.g. paste).
        (KeyCode::Char('\\'),     none) => Message::EnterContinuation,
    ];

    let continuation = map![
        (KeyCode::Enter,          none) => Message::InsertNewline,
        (KeyCode::Esc,            none) => Message::CancelContinuation,
    ];

    let key_naming = map![
        (KeyCode::Enter,          none) => Message::CommitKeyName,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let key_renaming = map![
        (KeyCode::Enter,          none) => Message::CommitKeyRename,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Tab,            none) => Message::ToggleRenameScope,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let deleting = map![
        (KeyCode::Enter,          none) => Message::CommitDelete,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Tab,            none) => Message::ToggleDeleteScope,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let filter = map![
        (KeyCode::Enter,          none) => Message::CommitEdit,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    Keybindings { normal, editing, continuation, key_naming, key_renaming, deleting, filter }
}
