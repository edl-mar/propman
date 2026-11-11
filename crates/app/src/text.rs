//! A line of text that is edited: the text and the place of the cursor in it.
//!
//! The cursor is counted in characters, not in bytes, so that a letter such as an umlaut is one
//! step. Characters that cannot be part of one line (a line break, a tab or any other control
//! character) are not inserted.

/// What can be done to a `TextField`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEdit {
    /// Puts a character in front of the cursor.
    Insert(char),
    /// Removes the character in front of the cursor.
    Backspace,
    /// Removes the character at the cursor.
    Delete,
    Left,
    Right,
    Home,
    End,
    /// Removes everything.
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextField {
    text: String,
    cursor: usize,
}

impl TextField {
    pub fn new() -> Self {
        Self::default()
    }

    /// A field with this text and the cursor behind it.
    pub fn with_text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            cursor: text.chars().count(),
        }
    }

    pub fn apply(&mut self, edit: TextEdit) {
        let length = self.text.chars().count();
        match edit {
            TextEdit::Insert(c) => {
                if !c.is_control() {
                    self.text.insert(self.byte_index(self.cursor), c);
                    self.cursor += 1;
                }
            }
            TextEdit::Backspace => {
                if self.cursor > 0 {
                    self.text.remove(self.byte_index(self.cursor - 1));
                    self.cursor -= 1;
                }
            }
            TextEdit::Delete => {
                if self.cursor < length {
                    self.text.remove(self.byte_index(self.cursor));
                }
            }
            TextEdit::Left => self.cursor = self.cursor.saturating_sub(1),
            TextEdit::Right => self.cursor = (self.cursor + 1).min(length),
            TextEdit::Home => self.cursor = 0,
            TextEdit::End => self.cursor = length,
            TextEdit::Clear => {
                self.text.clear();
                self.cursor = 0;
            }
        }
    }

    /// The place in the string of the character with this index; the end if there is none.
    fn byte_index(&self, characters: usize) -> usize {
        self.text
            .char_indices()
            .nth(characters)
            .map_or(self.text.len(), |(index, _)| index)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The place of the cursor, in characters from the start: 0 is in front of the first one.
    pub fn cursor(&self) -> usize {
        self.cursor
    }
}
