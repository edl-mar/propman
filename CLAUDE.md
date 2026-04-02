# propman

A terminal UI tool for managing Java `.properties` files, written in Rust.

## What this project does

propman scans a directory recursively for `.properties` files, groups them
by locale (everything after the first `_` in the filename stem is treated as
a locale identifier; files with no `_` get locale `"default"`), and presents
them as an interactive TUI. The user can navigate, filter, and edit
translations across all locales side by side.

## Module architecture

The app follows The Elm Architecture (TEA):

    Event → Message → Update → State → Render

tui           ratatui + crossterm — rendering and raw keyboard event capture
keybindings   HashMap<KeyEvent, Message> — translates raw events to semantic messages
messages      Message enum — all possible actions in the app
update        (State, Message) → State — mostly pure; SaveFile is the one place
              that performs I/O (flushing pending_writes to disk)
state         complete app state — see AppState fields below
filter        filter parser and evaluator — FilterExpr AST, parse(), evaluate(),
              visible_locales()
render_model  build_display_rows() — converts a key slice into Vec<DisplayRow>
editor        CellEdit — wraps TextArea<'static> + original string for the cell editor
workspace     groups PropertiesFile sets, locale detection, merged key index;
              single source of truth — owns all FileEntry vecs
parser        line-by-line reader — preserves KeyValue, Comment, Blank entries;
              joins \-continuation lines into a single value string
writer        write_change() — rewrites a line range, preserves everything else
search        stub (nucleo fuzzy search planned; currently unused)

New features are added by introducing new Message variants and handling them
in update — the event handler and renderer stay largely untouched.

## AppState fields

```rust
pub workspace: Workspace,
pub display_rows: Vec<DisplayRow>,   // filtered + grouped; rebuilt on filter change
pub visible_locales: Vec<String>,    // subset of all_locales() when a locale
                                     // selector is active; full list otherwise
pub cursor_row: usize,               // index into display_rows
pub cursor_col: usize,               // 0 = key column, 1..=n = visible_locales[n-1]
pub scroll_offset: usize,
pub mode: Mode,                      // Normal | Editing | Filter
pub filter_textarea: TextArea<'static>, // always present; lines()[0] is the query
pub unsaved_changes: bool,
pub pending_writes: Vec<(PathBuf, usize, usize, String, String)>,
                                     // (path, first_line, last_line, key, new_value)
                                     // flushed on SaveFile
pub quitting: bool,
pub edit_buffer: Option<CellEdit>,   // present only while mode == Editing
```

`AppState` does not derive `Clone` — `TextArea` is not `Clone`, and `Clone`
was never needed since `update` takes ownership of state.

## Modes

```
Normal   — navigation, entering Edit/Filter mode
Editing  — editing a cell in the bottom pane; Esc cancels, Enter commits
Filter   — typing in the filter bar; Esc returns to Normal (keeps query)
```

Escape cycles: Normal → Filter → Normal. Clearing the filter is explicit
(ClearFilter message, no default binding yet).

## Key design decisions

**File order preservation**
The original file is never sorted or reformatted. Every entry (key-value,
comment, blank line) is stored with its original line number. On save, only
the changed lines are rewritten — diffs stay minimal and readable.

**FileEntry model**

    enum FileEntry {
        KeyValue { first_line: usize, last_line: usize, key: String, value: String },
        Comment  { line: usize, raw: String },
        Blank    { line: usize },
    }

`KeyValue` spans `first_line..=last_line` to support `\`-continuation lines.
For single-line values `first_line == last_line`. `Comment` and `Blank` are
always exactly one line.

The parser joins `\`-terminated continuation lines into a single value string.
`\` continuation is a file-format detail for long lines — the logical value is
always a single string. The writer collapses continuation lines when saving an
edited value (re-splitting is a future TODO).

**Render model**
`build_display_rows(keys: &[String]) -> Vec<DisplayRow>` converts the flat key
list into a trie and emits a flat sequence of header and key rows:

    enum DisplayRow {
        Header { prefix: String },
        Key    { display: String, full_key: String },
    }

A `Header` is emitted at branch points only: a trie node gets a header when
all of its children are leaf keys and there are ≥2 of them. Single-child
chains collapse silently. Nodes with mixed-depth children get no header.

`Key.display` is ".suffix" when the key falls under a header, or the full key
when it stands alone. The renderer indents suffixes with 2 spaces.

**Selection model**
`cursor_row` indexes into `display_rows`. All rows (Header and Key) are
navigable. `cursor_col` indexes into `visible_locales` (offset by 1; col 0 is
the key column).

`Key` rows show actual values. A cell with no value in this locale but present
in others is shown as `<missing>` in red. Header row cells are empty (the key
does not exist anywhere yet).

**Filter system**
The filter bar is always visible at the bottom. `/` focuses it. The bar is
backed by a permanent `TextArea<'static>` in `AppState`; the query is
`filter_textarea.lines()[0]`, parsed live into a `FilterExpr` AST on every
keystroke and applied to `workspace.merged_keys` to produce `display_rows`.

Locale selectors also control column visibility: only locales matched by any
`LocaleStatus` term are shown in `visible_locales`. When there are no locale
selectors, all locales are visible.

See `docs/filtering.md` for the full syntax and internal representation.

**Write model**
Edits are committed in-memory immediately on Enter. Each committed edit is
appended to `state.pending_writes`. Ctrl+S flushes `pending_writes` to disk
via `writer::write_change` (one call per edit). Failed writes are kept in
`pending_writes` so the user can retry. The `[+]` indicator in the status bar
reflects `state.unsaved_changes`.

Editing a `<missing>` cell (key not present in the locale file) is currently
a no-op — new key creation is not yet implemented.

**Cell editor**
Pressing Enter on a locale cell opens a bottom editor pane. The pane title
shows `key [locale]`. The pane is backed by a `CellEdit` which wraps
`TextArea<'static>` and stores the original value for change detection.
Enter commits; Esc cancels. The pane height grows dynamically with the number
of continuation lines (capped at 8 total lines).

Continuation lines: typing `\` enters `Mode::Continuation` (status bar shows
`CONT  `). Enter in that sub-mode strips the `\` and opens a new line in the
TextArea. Esc cancels the continuation and leaves `\` as a literal character.
Any other key also cancels continuation and is typed normally. On commit,
`current_value()` joins all TextArea lines with `""` (no separator) — the
`\` continuation is a file-format detail; the logical value is always a single
string. The writer currently writes the value as a single line; re-splitting
with `\` continuations on save is a future TODO.

The value shown in the table cell while editing is the last saved workspace
value (reversed highlight), not the live edit — the bottom pane is the source
of truth during editing.

**Keybindings**
Keybindings are mode-aware: `keybindings.rs` exposes a `Keybindings` struct
with one `HashMap<KeyEvent, Message>` per mode (`normal`, `editing`,
`continuation`, `filter`). No keybinding is hardcoded in the rendering or
update logic.

`handle_key` in `tui.rs` selects the map for the current mode, dispatches any
match, then falls through to `TextInput` / `FilterInput` for unbound keys in
text modes (arrows, printable chars, etc. all reach the TextArea).

On Windows, crossterm fires both Press and Release events; the event loop
filters to `KeyEventKind::Press` only.

On Windows, AltGr is emitted as `Ctrl+Alt`. `normalize_altgr()` in `tui.rs`
strips those modifiers from `Char` keys before lookup and TextArea dispatch,
so characters like `\`, `@`, `[`, `]` typed via AltGr work correctly.

## Default keybindings

```
Normal:       ↑↓←→/hjkl navigate  PgUp/PgDn  Enter edit  Esc→Filter
              / filter  Ctrl+S save  q/Ctrl+C quit

Editing:      Enter commit  Esc cancel  \ continuation  Ctrl+S save  Ctrl+C quit
              (all other keys → TextArea: arrows move cursor, backspace, etc.)

Continuation: Enter new line  Esc cancel \
              (all other keys → cancel continuation, key typed normally)

Filter:       Enter close  Esc close  Ctrl+S save  Ctrl+C quit
              (all other keys → filter TextArea)
```

## Stack

- ratatui 0.29 + crossterm 0.28 — TUI (cross-platform, including Windows)
- tui-textarea 0.7 — cell editor pane and filter bar TextArea
- walkdir — recursive directory scanning
- anyhow — error handling
- nucleo — fuzzy search (planned; current key matching is substring)
- no async runtime — synchronous TUI app

## Working style

This project is developed collaboratively. Before writing code, think through
the design. Prefer small, focused changes. When something is unclear or there
are multiple reasonable approaches, say so — don't silently pick one.

Keep the code readable for someone who knows Rust concepts but is not an
expert. Avoid overly clever signatures. If a lifetime or trait bound is
non-obvious, add a short comment.

When making changes that touch the file format or data model, be especially
careful — the writer must never corrupt a file.

## TODO

### Near-term

- **ClearFilter keybinding**: `ClearFilter` message exists but has no default
  binding. Add one (e.g. Ctrl+Backspace or a dedicated key).
- **New key creation**: editing a `<missing>` cell should append the new
  key=value to the locale file and add it to the workspace in-memory.
  Editing an empty cell on a `Header` row should do the same, pre-filling
  `prefix.` as the key prefix.
- **Save error display**: when `write_change` fails, surface the error in the
  status bar rather than silently keeping `[+]`.
- **Viewport height**: `clamp_scroll` uses a hardcoded `VIEWPORT = 20`.
  Pass the actual terminal height through from the renderer.
- **Re-split continuation lines on save**: `writer::write_change` currently
  collapses multi-line values to a single line. It should re-split long values
  with `\` continuations to preserve file readability.

### Medium-term
- **nucleo fuzzy matching**: replace the current `key.contains(pattern)`
  substring match with nucleo for key pattern filtering.
- **Separator style preservation**: `writer::write_change` always writes
  `key=value`. It should detect and preserve the original separator (`=` vs
  `:`) and surrounding whitespace.
- **Column visibility for Any modifier**: `:de` (no `?`/`!`) narrows the
  column list correctly but there is no visual "focus" hint differentiating
  shown-by-filter columns from hidden ones.

### Future
- **Multi-selection**: Shift+A selects all visible rows; actions apply to all
  selected entries (e.g. bulk-copy default value to a missing locale).
- **New key flow** (`n`): opens an input pre-filled with the sibling prefix of
  the row under the cursor, with fuzzy search over existing keys.
- **User config file**: load keybindings from a TOML config at startup.
- **OR in filter expressions**: `FilterExpr::Or` is reserved in the AST but
  not yet parsed or evaluated.

## Further documentation

- [docs/filtering.md](docs/filtering.md) — filter syntax, AST, evaluation
