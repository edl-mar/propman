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
    /// Enter pressed in KeyNaming mode — validates and confirms the typed key name.
    CommitKeyName,
    /// `n` pressed in Normal mode — open the key-naming editor to create a new key.
    NewKey,
    /// Enter pressed in KeyRenaming mode — validate and apply the rename (move).
    CommitKeyRename,
    /// `p` pressed in KeyRenaming mode — validate and apply as a copy (source kept).
    CommitKeyCopy,
    /// Tab pressed in Normal mode — cycle selection scope (exact / +children).
    CycleScope,
    /// A raw key event forwarded to the active TextArea (Editing/Continuation mode).
    TextInput(crossterm::event::KeyEvent),
    // Filter
    FocusFilter,
    /// A raw key event forwarded to the filter TextArea (Filter mode).
    FilterInput(crossterm::event::KeyEvent),
    ClearFilter,
    // File ops
    SaveFile,
    /// `d` in Normal mode — on col 0: enter Deleting mode; on locale col: delete
    /// that one locale's entry immediately.
    DeleteKey,
    /// Enter in Deleting mode — confirms key/prefix deletion.
    CommitDelete,
    // App
    Quit,
    /// `Space` in Normal mode — toggle the read-only preview pane.
    TogglePreview,
}
