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
    pub filter:        HashMap<KeyEvent, Message>,
    pub bundle_naming: HashMap<KeyEvent, Message>,
    pub locale_naming: HashMap<KeyEvent, Message>,
    pub pasting:       HashMap<KeyEvent, Message>,
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
    let shift = KeyModifiers::SHIFT;

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
        (KeyCode::Up,            shift) => Message::JumpToPrevBundle,
        (KeyCode::Down,          shift) => Message::JumpToNextBundle,
        (KeyCode::Enter,          none) => Message::StartEdit,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('/'),      none) => Message::FocusFilter,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('q'),      none) => Message::Quit,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
        (KeyCode::Char('n'),      none) => Message::NewKey,
        (KeyCode::Char('N'),     shift) => Message::NewBundle,
        (KeyCode::Char('d'),      none) => Message::DeleteKey,
        (KeyCode::Char(' '),      none) => Message::TogglePreview,
        (KeyCode::Char('m'),      none) => Message::TogglePin,
        (KeyCode::Tab,            none) => Message::CycleScope,
        (KeyCode::Char('y'),      none) => Message::YankCell,
        (KeyCode::Char('y'),      ctrl) => Message::YankAndOpenPaste,
        (KeyCode::Char('p'),      none) => Message::OpenPaste,
        (KeyCode::Char('p'),      ctrl) => Message::QuickPaste,
        (KeyCode::Up,             ctrl) => Message::SiblingUp,
        (KeyCode::Down,           ctrl) => Message::SiblingDown,
        (KeyCode::Right,          ctrl) => Message::GoToFirstChild,
    ];

    let editing = map![
        (KeyCode::Enter,          none) => Message::CommitEdit,
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
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
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let key_renaming = map![
        (KeyCode::Enter,          none) => Message::CommitKeyRename,
        (KeyCode::Enter,          ctrl) => Message::CommitKeyCopy,
        (KeyCode::Tab,            none) => Message::CycleScope,
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let deleting = map![
        (KeyCode::Enter,          none) => Message::CommitDelete,
        (KeyCode::Tab,            none) => Message::CycleScope,
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let filter = map![
        (KeyCode::Enter,          none) => Message::CommitEdit,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let pasting = map![
        // Table navigation (same as Normal).
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
        (KeyCode::Up,            shift) => Message::JumpToPrevBundle,
        (KeyCode::Down,          shift) => Message::JumpToNextBundle,
        // Ctrl+arrows navigate the clipboard panel.
        (KeyCode::Left,           ctrl) => Message::PasteNavLeft,
        (KeyCode::Right,          ctrl) => Message::PasteNavRight,
        (KeyCode::Up,             ctrl) => Message::PasteNavUp,
        (KeyCode::Down,           ctrl) => Message::PasteNavDown,
        // Yank while in paste mode.
        (KeyCode::Char('y'),      none) => Message::YankCell,
        (KeyCode::Char('y'),      ctrl) => Message::YankToFocusedLocale,
        // Paste operations.
        (KeyCode::Char('p'),      ctrl) => Message::PasteHere,
        (KeyCode::Char('d'),      none) => Message::RemovePasteEntry,
        (KeyCode::Char('p'),      none) => Message::QuickPaste,
        (KeyCode::Enter,          none) => Message::CommitPaste,
        (KeyCode::Enter,          ctrl) => Message::CommitPasteStay,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let bundle_naming = map![
        (KeyCode::Enter,          none) => Message::CommitBundleName,
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    let locale_naming = map![
        (KeyCode::Enter,          none) => Message::CommitLocaleName,
        (KeyCode::Up,             none) => Message::MoveCursorUp,
        (KeyCode::Down,           none) => Message::MoveCursorDown,
        (KeyCode::Esc,            none) => Message::CancelEdit,
        (KeyCode::Char('s'),      ctrl) => Message::SaveFile,
        (KeyCode::Char('c'),      ctrl) => Message::Quit,
    ];

    Keybindings { normal, editing, continuation, key_naming, key_renaming, deleting, filter, bundle_naming, locale_naming, pasting }
}
