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
    /// Added to the selected item.
    pub selected: Style,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            folder: Style::new().add_modifier(Modifier::BOLD),
            bundle: Style::new(),
            file: Style::new(),
            uncertain: Style::new().fg(Color::Yellow),
            excluded: Style::new().fg(Color::DarkGray),
            selected: Style::new().add_modifier(Modifier::REVERSED),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ui {
    pub keymap: Keymap,
    pub theme: Theme,
}
