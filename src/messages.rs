#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    // Navigation
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorLeft,
    MoveCursorRight,
    PageUp,
    PageDown,
    /// `Shift+↑` — jump to the start of the previous bundle.
    JumpToPrevBundle,
    /// `Shift+↓` — jump to the start of the next bundle.
    JumpToNextBundle,
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
    /// `n` pressed in Normal mode — open the key-naming editor to create a new key,
    /// or the locale-naming editor when on a bundle-level header.
    NewKey,
    /// `N` (Shift+n) in Normal mode — open the bundle-naming editor.
    NewBundle,
    /// Enter pressed in LocaleNaming mode — validate and create the locale file.
    CommitLocaleName,
    /// Enter pressed in BundleNaming mode — validate and create the bundle.
    CommitBundleName,
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
    /// `m` in Normal mode — toggle permanent pin on the current key.
    /// Pinned keys bypass the filter and show an `@` indicator.
    TogglePin,
    /// `y` in Normal mode on a locale cell — yank value into per-locale clipboard history.
    YankCell,
    /// `Ctrl+Y` in Normal mode — yank the current cell and immediately open paste mode.
    YankAndOpenPaste,
    /// `p` in Normal mode — open paste mode to review and apply clipboard contents.
    OpenPaste,
    /// Ctrl+P in Normal mode — quick-paste the last yanked value into the current cell.
    QuickPaste,
    // ── Paste mode ──────────────────────────────────────────────────────────────
    /// Ctrl+Left in paste mode — move focus to the previous locale column.
    PasteNavLeft,
    /// Ctrl+Right in paste mode — move focus to the next locale column.
    PasteNavRight,
    /// Ctrl+Up in paste mode — move `>` selection up in the focused locale's history.
    PasteNavUp,
    /// Ctrl+Down in paste mode — move `>` selection down in the focused locale's history.
    PasteNavDown,
    /// `d` in paste mode — remove the selected history entry for the focused locale.
    RemovePasteEntry,
    /// `p` / Enter in paste mode — paste all `>` selections into the current key.
    CommitPaste,
    /// Ctrl+Enter in paste mode — structural paste (all `>` selections), stay in paste mode.
    CommitPasteStay,
    /// Ctrl+Enter in paste mode — paste the focused locale's `>` value into the current cell.
    CommitPasteCell,
    /// Ctrl+P in paste mode — paste clipboard_last into the current cell, stay in paste mode.
    PasteHere,
    /// Ctrl+Y in paste mode — yank the current cell's value into the panel-focused locale's history.
    YankToFocusedLocale,
}
