//! What the user can change about the interface: the keymap and the theme. `Ui` is handed to the
//! loop and to the drawing; nothing else in the program knows a key or a colour.

use ratatui::style::{Color, Modifier, Style};

use crate::keys::Keymap;

/// How the roles of the screen are drawn. The meaning never rests on a style alone (the marks
/// `>`, `?` and `[excluded]` are text), so a theme is decoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub folder: Style,
    pub bundle: Style,
    pub file: Style,
    /// Added to a file that is not certain.
    pub uncertain: Style,
    /// Added to a bundle that is excluded.
    pub excluded: Style,
    /// Added to the row under the cursor.
    pub cursor: Style,
    /// The letters of a candidate that were hit by what is typed in the command line.
    pub matched: Style,
    /// Added to an item that is marked.
    pub marked: Style,
    /// The box of an alert: its border, its title and its text.
    pub alert: Style,
    /// The number of a line in the preview.
    pub line_number: Style,
    /// The line ending at the right edge of a line in the preview.
    pub ending: Style,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            folder: Style::new().add_modifier(Modifier::BOLD),
            bundle: Style::new(),
            file: Style::new(),
            uncertain: Style::new().fg(Color::Yellow),
            excluded: Style::new().fg(Color::DarkGray),
            cursor: Style::new().add_modifier(Modifier::REVERSED),
            matched: Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            marked: Style::new().fg(Color::Cyan),
            alert: Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            line_number: Style::new().fg(Color::DarkGray),
            ending: Style::new().fg(Color::DarkGray),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ui {
    pub keymap: Keymap,
    pub theme: Theme,
}
