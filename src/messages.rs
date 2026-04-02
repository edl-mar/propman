#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    // Navigation
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorLeft,
    MoveCursorRight,
    PageUp,
    PageDown,
    // Editing
    StartEdit,
    CommitEdit,
    CancelEdit,
    /// `\` pressed in Editing mode — inserts `\` into the TextArea and enters
    /// Continuation sub-mode, where Enter inserts a newline instead of committing.
    EnterContinuation,
    /// Enter pressed in Continuation sub-mode — strips trailing `\`, inserts newline.
    InsertNewline,
    /// Esc pressed in Continuation sub-mode — returns to Editing, `\` stays literal.
    CancelContinuation,
    /// A raw key event forwarded to the active TextArea (Editing/Continuation mode).
    TextInput(crossterm::event::KeyEvent),
    // Filter
    FocusFilter,
    /// A raw key event forwarded to the filter TextArea (Filter mode).
    FilterInput(crossterm::event::KeyEvent),
    ClearFilter,
    // File ops
    SaveFile,
    // App
    Quit,
}
