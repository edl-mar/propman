# propman

A terminal UI tool for managing Java `.properties` files, written in Rust.

## What this project does

propman scans a directory recursively for `.properties` files, groups them
by bundle (the file stem before the first `_`) and locale (everything after
the first `_`; files with no `_` get locale `"default"`), and presents them
as an interactive TUI. The user can navigate, filter, edit, create, rename,
and delete translations across all bundles and locales side by side.

## Module architecture

The app follows The Elm Architecture (TEA):

    Event → Message → Update → State → Render

```
tui           ratatui + crossterm — layout, rendering, raw keyboard event capture
keybindings   HashMap<KeyEvent, Message> per mode — translates raw events to messages
messages      Message enum — all possible actions in the app
update        (State, Message) → State — pure except SaveFile (flushes pending_writes)
state         AppState — complete app state (see fields below)
filter        FilterExpr AST, parse(), evaluate(), visible_locales()
render_model  build_display_rows() — converts bundle-qualified key slice → Vec<DisplayRow>
editor        CellEdit — wraps TextArea<'static> + original string for change detection
workspace     Workspace — owns all FileGroup/PropertiesFile/FileEntry data;
              single source of truth for keys and values
parser        line-by-line reader — preserves KeyValue, Comment, Blank entries;
              joins \-continuation lines into a single logical value string
writer        write_change(), write_insert(), write_delete() — targeted file rewrites
search        stub (nucleo fuzzy search planned; currently unused)
```

New features are added by introducing new `Message` variants and handling them
in `update` — the event handler and renderer stay largely untouched.

## Interaction model

The control flow in Normal mode follows a consistent pattern:

    selection (cursor) → action key → [confirmation sub-mode] → [scope toggle] → Enter

- **`Enter` on a locale cell**: open value editor (`Mode::Editing`)
- **`Enter` on the key column**: open rename editor (`Mode::KeyRenaming`)
- **`n`**: open new-key editor (`Mode::KeyNaming`)
- **`d` on a locale cell**: immediate locale-entry deletion (no sub-mode)
- **`d` on the key column**: open deletion confirmation (`Mode::Deleting`)
- **`/`**: focus filter bar (`Mode::Filter`)

Sub-modes with Tab-toggle (KeyRenaming, Deleting) offer `[exact]` vs
`[+children]` scope. The pane title always reflects the current scope.
Bundle-level Header rows are blocked from rename and edit (the bundle name
is the file name — renaming a bundle would rename the file on disk).

## AppState fields

```rust
pub workspace: Workspace,
pub display_rows: Vec<DisplayRow>,   // rebuilt on every filter/edit change
pub visible_locales: Vec<String>,    // subset of all_locales() when locale
                                     // selector is active; full list otherwise
pub cursor_row: usize,               // index into display_rows
pub cursor_col: usize,               // 0 = key column, 1..=n = visible_locales[n-1]
pub scroll_offset: usize,
pub mode: Mode,
pub filter_textarea: TextArea<'static>, // always present; lines()[0] is the query
pub unsaved_changes: bool,
pub pending_writes: Vec<PendingChange>, // flushed on SaveFile
pub quitting: bool,
pub edit_buffer: Option<CellEdit>,   // present while mode is Editing / KeyNaming
                                     // / KeyRenaming / Deleting
pub selection_scope: SelectionScope, // Exact / Children / ChildrenAll; Tab-cycles
                                     // in Normal, KeyRenaming, Deleting
pub status_message: Option<String>,  // one-shot; cleared on next keypress
pub show_preview: bool,              // Space toggles read-only value preview pane
pub dirty_keys: HashSet<String>,     // keys with unsaved changes; auto-set on any
                                     // mutation, cleared per-key on Ctrl+S
pub temp_pins: Vec<String>,          // hidden children surfaced while ChildrenAll
                                     // is active; discarded on mode exit
pub pinned_keys: HashSet<String>,    // manual bookmarks; bypass filter until unpinned
```

`AppState` does not derive `Clone` — `TextArea` is not `Clone`, and `Clone`
was never needed since `update` takes ownership of state.

## Modes

```
Normal      — navigation and action dispatch
Editing     — editing a cell value in the bottom pane (Enter commit, Esc cancel)
Continuation — sub-mode of Editing: \ was typed; Enter inserts a newline
KeyNaming   — typing a new key name; Enter creates the key, Esc cancels
KeyRenaming — editing the current key name; Tab toggles exact/+children scope
Deleting    — confirming key deletion; Tab toggles exact/+children scope
Filter      — typing in the filter bar; Esc returns to Normal (keeps query)
```

Escape cycles: `Normal → Filter → Normal`.
`ClearFilter` message exists but has no default binding yet.

## Key design decisions

**Bundle system**
Files are grouped by bundle (base filename before `_`). Keys are stored in
`merged_keys` as `"bundle:real_key"` (e.g. `"messages:app.title"`).
`workspace::split_key(full_key) -> (&str, &str)` splits on the first `:`.
`workspace.get_value(full_key, locale)` routes lookups through the correct
bundle group. Files on disk always store the bare real key — the bundle
prefix is an in-memory convention only.

Bundle-level Header rows (depth 0, prefix == bundle name) are shown without
locale columns and are blocked from value editing and rename.

Cross-bundle rename/move is supported: renaming a key to a different bundle
prefix runs `commit_cross_bundle_rename`, which snapshots values, deletes
from the source, and inserts into the destination. Locales with no matching
file in the destination are listed in the status bar.

**File order preservation**
The original file is never sorted or reformatted. Every entry is stored with
its original line number. On save, only the changed lines are rewritten.

**FileEntry model**

```rust
enum FileEntry {
    KeyValue { first_line: usize, last_line: usize, key: String, value: String },
    Comment  { line: usize, raw: String },
    Blank    { line: usize },
}
```

`KeyValue` spans `first_line..=last_line` to support `\`-continuation lines.
The parser joins continuation lines into a single value string (the `\` and
newline are preserved verbatim as the physical value; the display layer strips
them for rendering). The writer collapses multi-line values to a single line
on save (re-splitting is a future TODO).

**Render model**
`build_display_rows(keys: &[String]) -> Vec<DisplayRow>` groups keys by
bundle, emits a bundle `Header` at depth 0 for each bundle, then walks a
dot-split trie for the real keys within it:

```rust
enum DisplayRow {
    Header { display: String, prefix: String, depth: usize },
    Key    { display: String, full_key: String, depth: usize },
}
```

`Header.prefix` is bundle-qualified for within-bundle headers
(e.g. `"messages:app.confirm"`); for bundle-level headers it is just the
bundle name. `Key.full_key` is always bundle-qualified.

Trie rules (applied per bundle):
- A non-key node with ≥2 key-children emits a `Header`; single-child chains
  collapse (the child's display is the full relative path from the outer header).
- A key-node that also has children emits only a `Key` row; its children
  appear indented below it at depth+1.
- `depth` drives indentation (`"  ".repeat(depth)`).

Keys without `:` (legacy / test keys) take the bare path — no bundle header.

**Selection model**
`cursor_row` indexes into `display_rows`. All rows (Header and Key) are
navigable. `cursor_col` indexes into `visible_locales` offset by 1 (col 0 is
the key column). `<missing>` in red marks cells where the key is absent from
that locale file. Bundle-level Header cells are always blank.

**Filter system**
The filter bar is always visible. `/` focuses it. The bar is backed by a
permanent `TextArea<'static>`; the query is `filter_textarea.lines()[0]`,
parsed live into a `FilterExpr` AST on every keystroke.

```
Full format: [bundle, ...] [/ key_pattern, ...] [: locale[modifier], ...]

Section rules — each section runs to the next separator or end of input:
  line start → / or :   bundle selectors  (whitespace-separated; unquoted = prefix)
  after /    → :        key patterns      (whitespace-separated; unquoted = substring)
  after :               locale selectors  (whitespace-separated; unquoted = prefix)

modifiers         — ! = must be present, ? = must be missing, none = column hint only
:?                — AnyMissing shorthand (at least one locale missing)
/*pattern         — DanglingKey: unsaved key matching the pattern
/#                — DirtyKey: all keys with unsaved changes
/confirm#         — keys matching "confirm" AND dirty (# in key section = AND term)
:#                — dirty keys; all locale columns visible
:[de]#            — dirty keys; narrows visible columns to de
```

`visible_locales` is narrowed to the matched locales when any `LocaleStatus`
selector is present; all locales are shown otherwise.

See `docs/filtering.md` for the full syntax and AST.

**PendingChange / write model**
Edits are committed in-memory immediately and appended to `pending_writes`.
Ctrl+S flushes them to disk. Failed writes are kept for retry. `[+]` in the
status bar reflects `unsaved_changes`.

```rust
enum PendingChange {
    Update { path, first_line, last_line, key, value, full_key }, // rewrite existing entry
    Insert { path, after_line, key, value, full_key },            // append new entry
    Delete { path, first_line, last_line, full_key },             // remove entry
}
```

`full_key` is the bundle-qualified key name used to rebuild `dirty_keys` after a
save: once all pending writes for a key flush successfully, the key leaves `dirty_keys`.

`insert_into_file(state, gi, fi, real_key, value)` is the shared helper used
by cell insert and cross-bundle move. It finds the insertion point, bumps
subsequent line numbers in-memory, and queues a `PendingChange::Insert`.

When entries are deleted, line numbers of all subsequent entries in the same
file are shifted down immediately so in-memory state stays consistent.

**Dirty tracking**
A key is dirty when it has unsaved changes in the current session. `dirty_keys`
is a `HashSet<String>` of bundle-qualified key names; every mutation path
(`ops::insert`, `ops::delete`, `ops::rename`) inserts the affected key immediately.
On Ctrl+S, `dirty_keys` is rebuilt from the keys still referenced by `pending_writes`
— keys whose writes flushed successfully are automatically removed.

Dirty keys bypass the filter (always visible, like pinned keys) and are shown with
a yellow key name and `#` prefix in the key column. Individual locale cells with a
pending write show a yellow `#[locale]` tag instead of the normal gray `[locale]`.
Per-cell dirty state is derived at render time from `pending_writes` + a path→locale
map — no separate per-cell state is stored.

The `#` sigil in the filter DSL (key section `/#`, locale section `:#`) narrows the
visible set to dirty keys. All terms are ANDed, so `/#` = all dirty keys, `/confirm#`
= dirty AND matching "confirm", `:[de]#` = dirty AND narrow to de column.

**Selection scope**
`SelectionScope` cycles (Tab) through three states in Normal, KeyRenaming, and
Deleting modes:

| Scope | Affected keys | Hidden children |
|---|---|---|
| `Exact` | key under cursor only | — |
| `Children` | cursor key + visible children | silently unaffected |
| `ChildrenAll` | cursor key + ALL children | temp-pinned on scope enter |

`temp_pins` holds hidden children surfaced by `ChildrenAll` so they appear in the
table for the duration of the operation. On commit they are discarded — dirty
tracking marks the changed keys, which keeps them visible automatically. On cancel
all temp pins are cleared.

**Deletion**
- `d` on a locale cell: removes that one `(key, locale)` entry immediately,
  leaving the key in `merged_keys` (other locales still visible).
- `d` on the key column: enters `Mode::Deleting`. The pane shows the key
  (read-only). Tab cycles selection scope. Enter confirms; Esc cancels. Dangling
  (unsaved) keys are dropped without a file write.

**Cell editor / KeyNaming / KeyRenaming**
The bottom edit pane is used for Editing, KeyNaming, KeyRenaming, and
Deleting modes. It is backed by a `CellEdit` wrapping `TextArea<'static>`.
The pane height grows with the number of lines (capped at 8).

- *Editing*: Enter commits, Esc cancels. `\` enters `Mode::Continuation`
  where Enter inserts a real newline instead of committing.
- *KeyNaming*: `n` in Normal pre-fills `"bundle:"` or `"bundle:prefix."`;
  Enter validates (non-empty, unique, has `.` or `:`) and creates the key.
- *KeyRenaming*: Enter on col 0. Tab toggles exact/+children. Cross-bundle
  rename (different prefix before `:`) triggers a move instead.
- *Deleting*: read-only display of the key; Tab and Enter only.

**Keybindings**
`keybindings.rs` exposes a `Keybindings` struct with one
`HashMap<KeyEvent, Message>` per mode. Unbound keys fall through to
`TextInput` / `FilterInput` in text modes; silently ignored in Normal /
Deleting. No keybinding is hardcoded in rendering or update logic.

On Windows: crossterm fires both Press and Release; loop filters to Press.
AltGr emits as Ctrl+Alt — `normalize_altgr()` strips those modifiers from
Char keys so `\`, `@`, `[`, `]` etc. work correctly.

## Default keybindings

```
Normal:       ↑↓←→ / hjkl navigate  PgUp/PgDn
              Enter  edit value (locale col) or rename key (col 0)
              n      new key
              d      delete locale entry (locale col) or enter Deleting (col 0)
              /      focus filter bar
              Ctrl+S save   q / Ctrl+C quit

Editing:      Enter commit  Esc cancel  \ continuation
              Ctrl+S save  Ctrl+C quit
              (all other keys → TextArea)

Continuation: Enter new line  Esc cancel \
              (all other keys → cancel continuation, key typed normally)

KeyNaming:    Enter confirm  Esc cancel
              Ctrl+S save  Ctrl+C quit
              (all other keys → TextArea)

KeyRenaming:  Enter confirm  Tab toggle exact/+children  Esc cancel
              Ctrl+S save  Ctrl+C quit
              (all other keys → TextArea)

Deleting:     Enter confirm  Tab toggle exact/+children  Esc cancel
              Ctrl+S save  Ctrl+C quit
              (no text input — pane is read-only)

Filter:       Enter / Esc close (keeps query)
              Ctrl+S save  Ctrl+C quit
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

- **Manual pinning**: `m` pins/unpins a key (or prefix subtree) as a permanent
  bookmark; pinned keys bypass the filter (`@` indicator in key column). `M`
  clears all pins. `pinned_keys: HashSet<String>` is already in AppState; only
  the keybindings and message handlers remain. See `docs/pinned_keys.md`.
- **Value preview**: `Space` in Normal mode opens a read-only preview pane
  showing the full value of the selected cell (important for multiline values
  that are truncated in the table). Also useful on key rows to show the full
  key path. Same dynamic-height pane as the edit pane; Esc / Space closes it.
- **Filter by translation text**: extend the filter DSL to match against
  translation values, not just key names. Possible syntax: `=pattern` (any
  locale contains pattern) or `:de=pattern` (specific locale). New
  `FilterExpr::TextMatch` variant in filter.rs.
- **ClearFilter keybinding**: `ClearFilter` message exists but has no
  default binding (e.g. `Ctrl+Backspace` or `X` in Filter mode).
- **Save error display**: when a write fails, surface the error in the
  status bar rather than silently keeping `[+]`.
- **Viewport height**: `clamp_scroll` uses a hardcoded `VIEWPORT = 20`.
  Pass the actual terminal height through from the renderer.

### Medium-term

- **nucleo fuzzy matching**: replace `key.contains(pattern)` substring match
  with nucleo for key pattern filtering.
- **Re-split continuation lines on save**: `write_change` collapses
  multi-line values to a single line. It should re-split long values with
  `\` continuations to preserve file readability.
- **Separator style preservation**: `write_change` always writes `key=value`.
  It should detect and preserve the original separator (`=` vs `:`) and
  surrounding whitespace.
- **Column visibility hint**: `:de` (no `?`/`!`) narrows the column list but
  there is no visual distinction between "shown by filter" and "always shown".

### Future

- **"+N ignored" hint**: when `[+children]` scope is active and hidden children
  exist, show a summary row just below the focused entry (e.g. `  +3 hidden
  entries ignored`) instead of listing them individually.
- **Multi-selection**: Shift+A selects all visible rows; actions apply to all
  selected entries (e.g. bulk-copy default value to missing locales).
- **User config file**: load keybindings from a TOML config at startup.
- **OR in filter expressions**: `FilterExpr::Or` is reserved in the AST but
  not yet parsed or evaluated.
- **Create locale file**: when a cross-bundle move has no destination file for
  a locale, offer to create the file rather than just warning.

## Further documentation

- [docs/filtering.md](docs/filtering.md) — filter syntax, AST, evaluation
