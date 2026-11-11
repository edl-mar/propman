//! Keys and the keymap: the one place that knows which key gives which message.
//!
//! `Key` is the program's own description of a key, independent of the terminal library, with a
//! text form (`ctrl+s`, `pgdn`) that a settings file can use later. The keymap has a global layer
//! and one layer per context (the current view); a key of the context wins over the same key of
//! the global layer. Nothing else in the program names a key.

use app::{AppState, Message, TextEdit, View};
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Esc,
    Tab,
    Backspace,
    Delete,
    /// A function key: `F(12)` is F12.
    F(u8),
}

/// A key with its modifiers. For a character the case is in the character: `N` is not
/// `shift+n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: Code,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Key {
    pub fn new(code: Code) -> Key {
        Key {
            code,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    pub fn char(c: char) -> Key {
        Key::new(Code::Char(c))
    }

    pub fn ctrl(self) -> Key {
        Key { ctrl: true, ..self }
    }

    pub fn alt(self) -> Key {
        Key { alt: true, ..self }
    }

    pub fn shift(self) -> Key {
        Key {
            shift: true,
            ..self
        }
    }

    /// The key of a terminal event: only the press of a key counts, and a character with Ctrl
    /// and Alt (what AltGr sends) is a plain character. Keys that `Code` cannot name give
    /// nothing.
    pub fn from_event(event: &Event) -> Option<Key> {
        let Event::Key(key) = event else {
            return None;
        };
        if key.kind != KeyEventKind::Press {
            return None;
        }
        let code = match key.code {
            KeyCode::Char(c) => Code::Char(c),
            KeyCode::Up => Code::Up,
            KeyCode::Down => Code::Down,
            KeyCode::Left => Code::Left,
            KeyCode::Right => Code::Right,
            KeyCode::PageUp => Code::PageUp,
            KeyCode::PageDown => Code::PageDown,
            KeyCode::Home => Code::Home,
            KeyCode::End => Code::End,
            KeyCode::Enter => Code::Enter,
            KeyCode::Esc => Code::Esc,
            KeyCode::Tab | KeyCode::BackTab => Code::Tab,
            KeyCode::Backspace => Code::Backspace,
            KeyCode::Delete => Code::Delete,
            KeyCode::F(number) => Code::F(number),
            _ => return None,
        };
        let modifiers = key.modifiers;
        if matches!(code, Code::Char(_)) {
            // AltGr arrives as Ctrl and Alt; the case of a character is in the character
            if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::ALT) {
                return Some(Key::new(code));
            }
            return Some(Key {
                shift: false,
                ctrl: modifiers.contains(KeyModifiers::CONTROL),
                alt: modifiers.contains(KeyModifiers::ALT),
                code,
            });
        }
        Some(Key {
            code,
            ctrl: modifiers.contains(KeyModifiers::CONTROL),
            alt: modifiers.contains(KeyModifiers::ALT),
            // a terminal reports Shift+Tab as a key of its own
            shift: modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::BackTab,
        })
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (set, name) in [
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
        ] {
            if set {
                write!(f, "{name}+")?;
            }
        }
        match self.code {
            Code::Char(' ') => f.write_str("space"),
            Code::Char(c) => write!(f, "{c}"),
            Code::Up => f.write_str("up"),
            Code::Down => f.write_str("down"),
            Code::Left => f.write_str("left"),
            Code::Right => f.write_str("right"),
            Code::PageUp => f.write_str("pgup"),
            Code::PageDown => f.write_str("pgdn"),
            Code::Home => f.write_str("home"),
            Code::End => f.write_str("end"),
            Code::Enter => f.write_str("enter"),
            Code::Esc => f.write_str("esc"),
            Code::Tab => f.write_str("tab"),
            Code::Backspace => f.write_str("backspace"),
            Code::Delete => f.write_str("delete"),
            Code::F(number) => write!(f, "f{number}"),
        }
    }
}

/// Where a key is looked up besides the global layer: what the user is looking at. An alert is
/// in front of the view and takes the keys, so the keys of the view do not work meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    Overview,
    Alert,
    CommandLine,
}

impl Context {
    /// Whether plain characters typed in this context are text for a field.
    pub fn takes_text(self) -> bool {
        matches!(self, Context::CommandLine)
    }

    pub fn of(state: &AppState) -> Context {
        if state.alert().is_some() {
            return Context::Alert;
        }
        if state.command_line().is_some() {
            return Context::CommandLine;
        }
        match state.view() {
            View::Overview(_) => Context::Overview,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    global: Vec<(Key, Message)>,
    contexts: Vec<(Context, Key, Message)>,
}

impl Keymap {
    /// A keymap without any binding.
    pub fn empty() -> Keymap {
        Keymap {
            global: Vec::new(),
            contexts: Vec::new(),
        }
    }

    /// Binds a key in every context. Binding a key again replaces the earlier binding.
    pub fn bind_global(mut self, key: Key, message: Message) -> Keymap {
        self.global.retain(|(bound, _)| *bound != key);
        self.global.push((key, message));
        self
    }

    /// Binds a key in one context. Binding a key again replaces the earlier binding.
    pub fn bind(mut self, context: Context, key: Key, message: Message) -> Keymap {
        self.contexts
            .retain(|(c, bound, _)| !(*c == context && *bound == key));
        self.contexts.push((context, key, message));
        self
    }

    /// The message of a key: the binding of the context, else the global one.
    pub fn message(&self, context: Context, key: Key) -> Option<Message> {
        let in_context = self
            .contexts
            .iter()
            .find(|(c, bound, _)| *c == context && *bound == key)
            .map(|(_, _, message)| *message);
        in_context.or_else(|| {
            self.global
                .iter()
                .find(|(bound, _)| *bound == key)
                .map(|(_, message)| *message)
        })
    }

    /// The message of a key in the layer of the context alone, without the global layer.
    pub fn context_message(&self, context: Context, key: Key) -> Option<Message> {
        self.contexts
            .iter()
            .find(|(c, bound, _)| *c == context && *bound == key)
            .map(|(_, _, message)| *message)
    }

    /// The keys of the layer of the context alone that give this message, in the order they were
    /// bound.
    pub fn context_keys_for(&self, context: Context, message: Message) -> Vec<Key> {
        self.contexts
            .iter()
            .filter(|(c, _, bound)| *c == context && *bound == message)
            .map(|(_, key, _)| *key)
            .collect()
    }

    /// The keys that give this message in this context, in the order they were bound, those of
    /// the context before the global ones. A key that the context binds to another message is
    /// not among them.
    pub fn keys_for(&self, context: Context, message: Message) -> Vec<Key> {
        let of_context = self
            .contexts
            .iter()
            .filter(|(c, _, bound)| *c == context && *bound == message)
            .map(|(_, key, _)| *key);
        let global = self
            .global
            .iter()
            .filter(|(key, bound)| {
                *bound == message && self.message(context, *key) == Some(message)
            })
            .map(|(key, _)| *key);
        of_context.chain(global).collect()
    }
}

impl Default for Keymap {
    /// `q` and `ctrl+c` quit; an alert is dismissed by `esc`, `enter` and `space`; in the overview `up`/`k` and `down`/`j` move, `pgup` and `pgdn`
    /// page, `home` and `end` go to the ends, `enter` edits the file, `space` marks, `esc`
    /// clears the marks, `v` switches the preview and `ctrl+up`, `ctrl+down`, `ctrl+u` and `ctrl+d` scroll it.
    fn default() -> Keymap {
        let overview = Context::Overview;
        let line = Context::CommandLine;
        Keymap::empty()
            .bind_global(Key::char('q'), Message::Quit)
            .bind_global(Key::char('c').ctrl(), Message::Quit)
            .bind(overview, Key::new(Code::Up), Message::Up)
            .bind(overview, Key::char('k'), Message::Up)
            .bind(overview, Key::new(Code::Down), Message::Down)
            .bind(overview, Key::char('j'), Message::Down)
            .bind(overview, Key::new(Code::PageUp), Message::PageUp)
            .bind(overview, Key::new(Code::PageDown), Message::PageDown)
            .bind(overview, Key::new(Code::Home), Message::First)
            .bind(overview, Key::new(Code::End), Message::Last)
            .bind(overview, Key::new(Code::Enter), Message::Edit)
            .bind(Context::Alert, Key::new(Code::Esc), Message::DismissAlert)
            .bind(Context::Alert, Key::new(Code::Enter), Message::DismissAlert)
            .bind(Context::Alert, Key::char(' '), Message::DismissAlert)
            .bind(line, Key::new(Code::Esc), Message::CloseCommandLine)
            .bind(line, Key::new(Code::Enter), Message::RunCommand)
            // the best candidate is the row above the input, so up goes to the next one
            .bind(line, Key::new(Code::Up), Message::NextCandidate)
            .bind(line, Key::new(Code::Down), Message::PreviousCandidate)
            .bind(line, Key::new(Code::Tab), Message::CompleteCommand)
            .bind(
                line,
                Key::new(Code::Backspace),
                Message::Text(TextEdit::Backspace),
            )
            .bind(
                line,
                Key::new(Code::Delete),
                Message::Text(TextEdit::Delete),
            )
            .bind(line, Key::new(Code::Left), Message::Text(TextEdit::Left))
            .bind(line, Key::new(Code::Right), Message::Text(TextEdit::Right))
            .bind(line, Key::new(Code::Home), Message::Text(TextEdit::Home))
            .bind(line, Key::new(Code::End), Message::Text(TextEdit::End))
            .bind(line, Key::char('u').ctrl(), Message::Text(TextEdit::Clear))
            .bind(overview, Key::char(':'), Message::OpenCommandLine)
            .bind(overview, Key::char(' '), Message::ToggleMark)
            .bind(overview, Key::new(Code::Esc), Message::ClearMarks)
            .bind(overview, Key::char('v'), Message::TogglePreview)
            .bind(overview, Key::new(Code::Up).ctrl(), Message::PreviewUp)
            .bind(overview, Key::new(Code::Down).ctrl(), Message::PreviewDown)
            .bind(overview, Key::char('u').ctrl(), Message::PreviewHalfPageUp)
            .bind(
                overview,
                Key::char('d').ctrl(),
                Message::PreviewHalfPageDown,
            )
    }
}
