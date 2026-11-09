# Decisions and open questions

Living list, newest first within each section. A decision is only listed here once it was
agreed explicitly; everything else is an open question. Drafts in `drafts-and-ideas/` are
not authoritative.

## Decisions

### The preview in the overview (2026-11-09)
- **The use case:** look into a file quickly, for example to check its encoding or its line
  endings, without leaving the list. The preview shows the raw lines of the selected file next
  to the list. It is derived from the state (the selected item and a scroll position), read
  only, has no focus and is not a view of its own.
- **On by default, `space` switches it.** It is drawn only if the terminal is at least 100
  columns wide; otherwise it is not drawn, and the setting is kept. When it is drawn, the list
  gets the left half. For a folder or a bundle the pane says that a file has to be selected.
- **Scrolling:** `ctrl+up` and `ctrl+down` by one line, `ctrl+u` and `ctrl+d` by half the
  height of the preview. It stops at the ends and starts at the top again whenever the selection
  changes.
- **What it shows:** a header with the name of the file, its encoding and its number of lines,
  then the lines with their numbers. The text of a line is cut with `...` if it is too long,
  control characters are shown as `·`, and the line ending is written as text at the right
  edge (`LF`, `CRLF`, `CR`, and `(none)` for a last line without a line break). `app` resolves
  all of this into plain values; the view only draws.

### Keys and colours live in one place each (2026-11-09)
- **Only `tui` knows a key or a colour.** `app` has messages and text that is already resolved.
  Both are handed to the loop and to the drawing as one value, `Ui { keymap, theme }`, so a
  settings file later only has to build that value; no other function changes.
- **`Key` is the program's own type** (code, ctrl, alt, shift), independent of the terminal
  library, with a text form (`ctrl+s`, `pgdn`) that a settings file can use. The case of a
  character is in the character, and AltGr (Ctrl and Alt on a character) is a plain character.
  Only the press of a key counts.
- **The keymap has a global layer and one layer per context**, the context being the current
  view. A key of the context wins over the same key of the global layer. This replaces the full
  table per mode of the first version, in which `Ctrl+C` stood nine times.
- **The line with the keys is made from the keymap**, so that it cannot name a key that is not
  bound.
- **The theme has roles, not colours:** folder, bundle, file, uncertain, excluded, selected. The
  style of uncertain, excluded and selected comes on top of the style of the role. The meaning
  never rests on a style (the marks are text), so a theme is decoration and is not tested for
  looks.
- **Not yet:** a settings file, names for messages (they come with the file, because they only
  matter there), and keys for text input (they come with editing).

### The bundle overview: the first view (2026-11-08)
- **A flat list, always expanded, with three kinds of items:** a folder, below it its bundles,
  below each bundle its files. The path of a folder is shown once, relative to the opened folder
  (`.` for the folder itself) and cut from the left if it is too long, so that its end stays
  visible. This tells apart two bundles with the same name in different folders (for example
  `src` and `target`), and it is where an action on a whole folder can attach later.
- **What an item shows.** Folder: the path and the number of bundles. Bundle: the name, the
  number of files, `[excluded]` if it is excluded. File: the locale or `(default)`, the file name,
  the encoding (`UTF-8`, `UTF-8+BOM`, `Windows-1252`, `Latin-1`) and the number of lines.
- **Marks, never by colour alone:** `>` for the selected item (and reversed), `?` for an uncertain
  file (and yellow), `[excluded]` for an excluded bundle (and dimmed). The colours are
  decoration and are not tested.
- **The list is a pure function of the session** (`app::overview`), as plain values with the text
  already resolved, so that a view only draws. Sorted by folder path, then bundle name; in a
  bundle the default file first, then the locales in alphabetical order. A file belongs to the
  bundle of its current assignment.
- **Selection and scroll position are indices in the state.** The height of the visible part
  arrives as a message from the event loop, at the start and whenever the terminal changes its
  size, because only the terminal knows it. The scroll position moves as little as possible to
  keep the selection visible. A page is as many items as fit, or one item while the height is
  unknown. The selection stops at the ends. An index is enough while the list does not reorder;
  with reload and moving files the selection can hold an id instead (a stale id gives `None`).
- **Keys:** `j` and `k` or the arrows, page up and page down, home and end.
- **The view enum exists from now on** with one variant, so that the next view is only another
  variant. `Session::exclude_bundle` exists; the key for it comes with the first operations.

### The user interface: structure and the first slice (2026-11-08)
The structure is inspired by The Elm Architecture (message, update, view), not a strict copy.
The state, the messages and `update` live in `app` (no terminal crate); the views, the mapping of
keys to messages and the event loop live in `tui`; `propman` reads the argument and starts it.
`tui` depends on `app` and the terminal library only, never on `store` or `disk`.

- **View and overlay are two orthogonal axes**, not one mode enum: a view is exclusive, an
  overlay lies over any view. This keeps the state space additive instead of multiplicative.
- **Views are an enum, not trait objects**, so the compiler reports every view that was not
  handled. `update` hands a message to small functions, one per view, that take the state by
  value and return it; a transition is a change of the current view. No `handle_op` with a
  context object, which would be impure and tends to become a bag of everything.
- **Narrow capability traits** only for operations that mean the same in several views (for
  example copy and paste); no universal address model for all views.
- **UI state** (selection, navigation, current view) lives in the app state and never in the
  store. A view is derived from the store each time. The UI may keep ids, because they carry a
  generation: a stale id gives `None` and cannot hit another row.
- "Introduce it when needed" is a hint, not a rule. The view enum comes around the second view,
  an overlay with the first dialog, a trait when two views share an operation; any of them may
  come earlier where that clearly saves a rebuild.
- **Keys:** only the press of a key counts, not the release (on Windows both arrive), and a
  character with Ctrl and Alt (what AltGr sends) counts as a plain character.
- **The first slice, in four steps:** a skeleton (loop, update, a small real view with the
  folder, the number of files and bundles, the problems and the key to quit); the bundle
  overview as the first view; the raw lines of a file; the first operations (change the
  encoding, exclude a bundle, save, quit). No filters, entries or conflict display yet.
- The only new external dependency is `ratatui`; `crossterm` is used through its re-export.
- **Open, a possible deviation from Elm:** in pure Elm `update` has no side effects and returns
  commands that the loop executes. For now `update` is pure and there are no side effects. When
  saving arrives (step 4) it has to be decided whether `update` calls the session directly or
  returns commands.

### Saving is: write, read back, take the snapshot; whether to save is decided by the bytes (2026-11-08)
A session loads a folder into the store (`Session::open`): discover the files, classify them,
read each one, one edit of the kind `Load` per file; problems are collected and do not stop the
rest. Saving a file (`Session::save_file`) writes its lines with the current encoding, expecting
the file on the disk to be the one that was read (else it is refused as changed or deleted),
then **reads the file again** with the encoding it was written in and lets the store take what is
now on the disk as a new snapshot (`reload_file`). One mechanism serves load, reload and save,
and the store always shows what is on the disk.

A character that the chosen encoding cannot hold is written as an escape (`\u20AC`); the new
snapshot then holds the escape as text and the old snapshot still holds the character. Both mean
the same for Java; on the level of values (later) they will compare equal after unescaping.

Whether a file has to be written is decided by comparing the bytes that its lines would give with
the bytes of the disk, not by the chosen encoding: a choice can be invisible on the disk (a file
with only ASCII looks the same in every encoding), and then it only lives in the settings.
Reinterpreting (read the same bytes differently) and converting (change what is written) stay
two different actions.

### The raw unit: what is on the disk, one way in (2026-11-08)
The raw unit (`store::raw`, a module of the crate `store`) represents what is on the disk, with
its interpretation (encoding, line ending, bundle assignment). It is read-only for the user: it
is only ever filled from the disk (load, reload). There is **one** mutable model, the fragments
and entries of a later unit, which refers to the raw lines; the raw lines themselves are never
edited. This avoids two models kept in sync, which was the cause of the save bugs of v1.

Saving will assemble the file from the fragments: an untouched fragment gives back its original
raw lines unchanged (bytes, line endings, indentation, continuation lines), a changed one is
built again, keeping the separator and the white space. For later, as an invariant to test: the
fragments of a file split its lines without loss, so assembling without any change gives the
same bytes. The raw lines also remain the basis for the line diff after a structural change on
the disk and for reinterpreting a file with another encoding. Inserting and reordering are
operations of the fragments, not of the raw lines, so a raw line has no position of its own:
the order of the lines of a file is the order in which they were created.

Until entries exist, the store offers no operation that changes raw lines for the user, and
saving a file writes its raw lines unchanged (in the chosen encoding).

### The first data model: three tables in the raw unit (2026-11-08)
| Table (id) | Column | Value |
|---|---|---|
| Bundle (`BundleId`) | `folder` | `PathId` |
| | `name` | `TextId` |
| | `excluded` | `bool` (initial `false`) |
| File (`FileId`) | `path` | `PathId` (where it was found) |
| | `assignment` | `BundleAssignment` |
| | `encoding` | `Encoding` |
| | `fingerprint` | `Fingerprint` |
| | `bytes` | `BytesId` (the original bytes) |
| | `lines` | `LineSpan` (the lines of the current load, see below) |
| Line (`LineId`) | `content` | `TextId` (without the line ending) |
| | `ending` | `LineEnding` |

- All columns are history columns, also those that never change; the identity columns just never
  get a second version.
- **The lines of a file are one value of the file**, a `LineSpan` (first row, count) in the line
  table, with a history. Every load of a file creates its lines as consecutive new rows and a
  new span; the current lines of a file are those of the last span, in the order inside the
  span. A reload therefore never changes old lines and is simply a new snapshot of the disk: the
  raw unit has no local changes, so nothing can conflict there, and a changed order in the file
  is just a different order in the new span. The old spans stay as versions and are the basis
  of the line diff. A raw line does not know its file; that is derived from the spans. The
  conflict only appears one level up, when fragments and entries of the user have to be mapped
  onto the new snapshot (by the identity of an entry, not by the position of a line).
- `BundleAssignment` is one value: the bundle, the locale (`Option<TextId>`) and the confidence.
  They always change together, and a confirmation by the user changes only the confidence,
  which counts as a deviation.
- The original bytes are stored from the start (in the byte pool). `disk::Loaded` therefore has
  to return the raw bytes as well.
- A table creates its row once for all its columns, in one method (`add_file`, `add_line`,
  `add_bundle`). Loading a file is one edit of the kind `Load` that creates the file and all its
  lines.
- The unit guards its own invariants: a bundle is unique by (folder, name) and `add_bundle`
  returns the existing one; the assignment of a file refers to an existing bundle of the same
  generation; the lines of a file are created with the file. Guarantees between units belong to
  the `Store`.
- The pools are owned by the `Store` and shared by all units, so equal text has the same id
  everywhere. Operations of a unit get the edit and the pools together in a small context.
- The columns are private. `app` only sees the `Store`: reading methods, and writing through a
  store level edit with fixed operations (add a file, set the encoding, set the assignment,
  exclude a bundle, reload, mark saved, undo). There is no way past the invariants, and the
  visibility tricks of the draft sketches are not needed.
- A reload (`reload_file`) brings the bytes, the fingerprint, the detected encoding and, if the
  lines differ, a new span, all from the source disk in one edit. It keeps the encoding the user
  chose, leaves the assignment alone and appends nothing if the disk says the same as before.
- The bundle lives in the raw unit as long as it only groups files. When the tree comes, it
  refers to the `BundleId` of the raw unit.

### The store: one history per value, sources on edits (2026-11-08)
This replaces the earlier split into facts and choices; both live in the same history now.
- **Values.** Every value is a column of small, trivially copyable entries (ids, enums, flags).
  Each row has a history of versions: the current value is the last one, version 0 is the initial
  value (what the disk said, or the default, for example `false` for "excluded"). Facts from the
  disk and choices of the user are versions of the same history.
- **Pools.** Large values (raw file bytes, text, paths) live in append-only pools with stable
  ids and deduplication, so equal content has the same id and comparing values is comparing
  numbers. Pools are generic and free of the domain (in `columns`); the store instantiates them.
  The original bytes of a file in the byte pool are the likely answer to "reinterpret with unsaved
  changes"; that is decided together with the lines.
- **Sources.** Every version belongs to an edit (which has a kind) and carries a **source**:
  the disk, a settings layer (shared, local) or the user. That is normally the source of its
  edit; only a version that a reload appends again for a higher ranked source carries that
  source, so the source is kept per version. A value comes from outside by an edit of its source
  (loading the disk, reading a settings file, a reload) or from the user. Layers are data (an
  ordered list of sources), so profiles later mean more sources, not a new structure, and the
  store does not hold the form of the settings.
- **Base is a query**, not stored: "the latest version from source S" (the disk, the shared file,
  the local file), falling back to version 0. What goes into a settings file is the deviation
  from the base below that layer, computed in `app`. There are no separate base histories.
- **Marker.** Saving into a source appends the current value again, with that source and the
  edit kind save, so that the base query reflects what was written. A reload that finds nothing
  new appends nothing, because the value of the base stays the same.
- **Precedence versus order.** The history is chronological; the effective value follows the
  rank disk < shared < local < user. A reload appends the new disk value and then, if the
  highest ranked source that has a version holds a different value, appends that value again.
  Only the highest ranked source is needed: effective values and bases stay the same without
  redundant versions.
- **Reload rule, fixed per column.** *Precedence*: an outside value never beats a higher ranked
  one (encoding, bundle assignment). *Conflict*: if both sides changed, the user decides (lines,
  later entry values). If only one side changed, that change is taken automatically in both
  cases. Automatic resolutions are ordinary edits: visible and undoable.
- **A conflict is derived, never stored.** While it is unresolved the external value is the
  current one and the user's version sits one step back; any later version from another source
  resolves it, by restoring either side (restoring always appends, even an equal value, because
  it records the decision; setting the current value again appends nothing). A further external
  change does not hide an unresolved conflict.
- **IDs** carry the generation number of the store, and lookups return `Option`: a stale id gives
  `None`, so "ids do not survive a rebuild" is enforced by the type and not only by discipline.
  The integer types are a detail.
- Files are addressed by path and bundles by (folder, base name), not by ids.
- A move is a later structural change: a column `moved_to` with the same history; the effective
  path is `moved_to` if set, otherwise the path where the file was found. The disk is only
  touched when saving. Moving across file systems is not a plain rename, and moving one file of
  a bundle to another folder changes its bundle; both are left for when moves are built.
- A goal, not a requirement: the store need not be rebuilt when saving (a save only appends
  markers), so that the history survives saving. To be checked along the way.
- Order of work: first as a specification in tests (loading with layers, reload with precedence,
  reload with conflict, saving with a marker), before the data layout is fixed.

### Generic building blocks in `columns`, the rules in `store` (2026-11-08)
`columns` holds what is free of the domain:
- `Idx`: an id gives its index, but cannot be made from one; only the owner of a table creates
  its ids.
- `HistoryColumn<Id, T, M>`: rows with an append-only stack of versions; each version holds a
  value and a small piece of metadata `M`. Operations: add a row, append, current value, all
  versions, access by reference, and "the latest version whose metadata matches".
- Pools (later).
- A reference to a value is generic: `VersionRef<Id>`, an id with a version number.

`store` holds what is specific to propman: sources, edit kinds, the journal, the rules
(precedence, conflict, markers, reload) and the typed ids of its tables. Several columns of one
table share one id type, and a row is created once per table. The first version of
`store::history` mixes both levels to specify the rules and is split afterwards.

### Shared value types live in a small crate `types` (2026-11-08)
`disk` and `store` must not know each other, but `store` has to keep the encoding, the line
endings, the confidence and the fingerprint to save a file again. They are defined once in the
crate `types` (`Encoding`, `LineEnding`, `Confidence`, `Fingerprint`), used by both, and `disk`
re-exports them under their old paths. The crate only holds plain value types, with no behavior
beyond what defines the value (for example `LineEnding::as_str`); anything with logic stays in
`disk` or `store`, so that it does not become a dumping ground.

`app` does all mapping between the sides, and no value type needs mapping any more: loading
turns the results of `disk` into rows of the store (borrowed lines are copied into owned ones),
saving builds borrowed line views from the store for `disk`, and settings files are read and
written by `disk` as text while `app` parses them and turns them into edits from the settings
sources. `tui` only sees what `app` passes on.

### Everything is non-destructive; history instead of undo (2026-11-08)
Nothing the user does destroys information. Instead of a classic undo built from commands,
every value keeps a history of its versions: restoring an older version is an ordinary new
edit that puts the old value at the end, and nothing is removed from the history during a
session. Several writes that belong to one user action are grouped into one edit. External
changes (for example new file versions after a `git pull`) are handled with the same history
and changes: they are added as new versions and never overwrite local ones. How conflicts are
resolved is described in the decision on the store (sources, precedence, a rule per column); the
conflict UI is still open.

### Project root and settings files (2026-11-08)
Only the user knows where the project root is, so propman never guesses and never assumes
version control.
- **Finding the root:** on start, search the parent folders of the opened folder for a
  `.propman.toml`. If there are several, the **outermost** one wins (like a Cargo workspace
  root over its crates). No file found: the session runs **without a root** and keeps all
  settings (encodings, bundle assignment, scopes, exclusions) in memory only.
- **Creating a root:** never automatically. The user is asked when saving or quitting, and only
  if there is something non-default to remember (so looking at downloaded files never asks).
  Never at a drive root or the home folder without explicit confirmation. If the location is
  not writable, the session continues without a root and says so. The UI always shows which
  root is in effect.
- **Paths** in the settings are relative to the folder of the root file and use `/`, so a
  project can be moved or cloned. Entries for files that no longer exist are kept as stale.
- **Nested roots:** when a new root is created above existing ones, offer to take over their
  settings (paths re-based mechanically; no conflict is possible because the new root is
  empty). The inner files are left untouched (they may be tracked in their own repository);
  deleting them is only offered. The other direction (outer to inner) is never offered,
  because a scope may span several modules.
- **Layers:** `.propman.toml` is the shared layer and marks the root. `.propman.local.toml`,
  in the same folder, is the personal layer and overrides the shared one; it only holds
  deviations and is only read next to an existing `.propman.toml`. Settings are read as an
  ordered list of layers, so further layers can be added later. An entry of a higher layer
  replaces the entry of a lower layer as a whole, it is not merged field by field.
- **Which layer gets a new setting** (to be checked in the UI): encoding, bundle assignment
  and scopes go to the shared layer (they describe the project); bundle exclusions go to the
  local layer (they are a personal view). The user can move a setting afterwards.
- Profiles with arbitrary names are not built now. If needed later, inheritance is declared
  inside the file (`extends = "..."`), not encoded in the file name (a name like
  `.propman-privat-otherprofile.toml` is ambiguous, the same problem as bundle versus locale).

### Bundle merging: no assumption, implicit scope first (2026-11-08)
In the user's project the application loads all bundles at once, and a key present in two
bundles collides. propman makes **no assumption about how the application merges** (it cannot
know which bundles are merged together, and a bundle such as `common.properties` may be part of
several groups).
- A **scope** is a group of bundles that one application loads together. A bundle can be in
  several scopes. A **collision** is the same key in at least two bundles of the same scope.
  propman reports it ("key X is defined in A and B") but does not claim which entry wins.
- First step: only one **implicit scope** containing all bundles that are not excluded. User
  defined scopes come later and are stored in the project root's settings.
- Key identity does not change: a key stays (bundle, key text). Collisions are a derived index
  from key text to bundles per scope. The store must therefore allow the same key text in
  several bundles, allow an index across bundles, and carry an "excluded" flag per bundle.
  An excluded bundle does not count towards collisions.

### Change detection lives in `disk` as a `Fingerprint` (2026-11-08)
`app` does no IO, so detecting outside changes belongs to `disk`. A fingerprint is a stamp
(length, modification time, taken before reading) plus a hash of the raw bytes that were read;
the hash is the authority (a changed modification time alone is a false alarm, e.g. after
`git checkout`). `read_file` returns it, `write_file` takes the expected one, refuses with
`Changed` or `Deleted` and returns the fingerprint of what it wrote. The hash is FNV-1a 64
written by hand so that it stays stable across runs and Rust versions (a saved session could
one day store it; `DefaultHasher` makes no such promise); add an algorithm tag if it ever
changes. What happens on a conflict is up to `app`. A small race between the check and the
rename remains.

### Discovery is neutral (2026-11-08)
`discover` finds every `.properties` file below the root (hidden files and directories
excepted) and has no special handling for names or paths such as `target`, `build` or `out`.
Build output that duplicates a bundle is dealt with by the user, not by the scanner.

### Excluding and re-assigning bundles is a user action (2026-11-08)
In the first (setup) view the user can assign bundle membership and encoding per file and
exclude bundles from the session. This covers "ignore this copy" and "treat this copy as its
own bundle". The choices are stored in the settings of the project root (see above).

### No mirroring of files or bundles (2026-11-08)
propman does not keep two files or bundles in sync (for example `src/` and `target/`): when
both change, resolving the conflict would be too hard. A "copy content" function may come
much later.

## Open questions

### Raw files: use cases and ideas (2026-11-09)
Ideas from a talk, **not decisions**. The preview itself is decided (see above).
- **Edit a file in `$EDITOR`** with `enter` on a file (for example when it cannot be parsed),
  instead of building an editor. This is the first real effect: the terminal is left, a process
  runs, the file is reloaded as a new snapshot. It forces the question whether `update` calls
  the session or returns an effect (Elm's `Cmd`). Open on the way: no `$EDITOR` set (do not
  assume one, say so), jumping to a line (depends on the editor, do not assume).
- **A raw view over all files** in the order of the overview, opened with `right`, with the
  files one after the other. It must be a virtual list (only the visible rows are made; the
  numbers of lines are in the store), and its position is a file and a line, not an index. The
  overview and the raw view follow each other: opening starts at the selected file, going back
  selects the file one is in. `left` should stay free for horizontal scrolling of long lines, so
  `esc` goes back.
- **Search in the raw view across all files.** The first text input of the program: keys that
  are not bound must then reach a text field (the keymap has no such case yet). A search jumps,
  a filter hides; they can share the input field. How to search (substring, case, regular
  expression) is not assumed. The filter system itself is still open.
- **Jumps to the borders of folders, bundles and files.** Direct keys are enough at first; an
  overlay (the first one) only pays off with more choices.

### Bundle merging: what is left
- Is a collision any shared key in any locale, or only per locale? Start with any locale, since
  the locale fallback depends on the consumer.
- May a scope get an order ("this bundle takes precedence") later?
- Merge rule for list valued settings across layers (replace or append), for example scopes.
- Where user defined scopes are shown and edited in the UI.

### Scanning a huge tree (for example `C:\`)
Ctrl+C is enough to abort. What matters is that the UI shows that a lot is being scanned
(directories visited, files found) so that it is not mistaken for a hang. `discover` currently
returns everything at the end, so it will need a progress hook (for example a callback).

### Symlinks
`discover` follows them, `atomic_write` replaces the link itself by a regular file, so a
shared linked file silently becomes a copy. Proposal: do not follow symlinks in `discover`
and report them as skipped. No real use case is known yet. Hard links are broken by
rename-based replacement either way; document that.

### Format of the settings files
Exact format, which crate reads and writes them, and the handling of the (probably needed)
TOML dependency are open; ask before adding a dependency. Concurrent writers can reuse
`atomic_write` and the fingerprint.

### History and the store: what is open
- **Shape of tables.** `main17.rs` and `blueprint_v3.rs` (drafts) were experiments about
  *visibility* (owner in a private module, reader, writer), not about how the data is stored.
  The tables of the raw unit are decided (see above). Still open: a macro for tables, to be
  considered after the second or third hand written table, and the fragment model with its own
  order of lines.
- **Concept notes in the drafts** (`propman-v2-architektur.md`: version stack per row, edits,
  saving as atomic write followed by a rebuild from disk with domain-stable identity, string
  pool; `propman-architecture-notes.md`: a fragment model in which comments and blank lines are
  addressable units). Not authoritative and partly outdated (see their headers).
- **Structural conflicts** (a key renamed or split outside): not solvable on the level of values.
  `propman-v2-konfliktaufloesung.md` sketches a line diff and a manual way out; open.

### Saving a value that is in conflict
The draft notes block saving exactly the affected entries while a conflict is unresolved (not
the whole file, no OS file lock). Not decided; it belongs to the conflict UI.

### Saving into a layer can be shadowed
If the user may choose where a setting is saved, saving into the shared file while the local file
holds a different value for it leaves the local value effective after a reload, so the change
seems lost. The UI has to warn or remove the local entry as well.

### Confirmation by the user
Confirming a detected assignment does not change its value, only its origin. To count as a
deviation it needs a third state next to certain and uncertain (`Manual`) in `types::Confidence`.

### Reinterpret versus convert
"Reinterpret" (read a file again with another encoding) and "convert" (change the encoding
used for saving) are separate actions. `decode` already accepts a hint. Reinterpreting with
unsaved changes: ask first, or keep the original bytes as an immutable base.

### Encoding for non-ASCII characters in pure ASCII files
A pure ASCII file is valid UTF-8 and valid Latin-1; it is detected as UTF-8, so typing an `ü`
writes UTF-8. Alternatives: the encoding of the neighbouring locale files, a folder default, or
asking. To be decided with the saving UI.

### Escapes in comment lines
`encode` works on text and escapes unrepresentable characters everywhere, including comment
lines, where Java does not process escapes (a `€` stays literal there). Options: do not
escape in comments and warn or fail. This needs to know which line is a comment, so it belongs
after the parse layer.

### Showing escapes
Showing `ü` as `ü` belongs to the parse layer and is independent of the encoding.

### Further topics from the notes (undecided)
- Order of entries per locale or per bundle (a product question), with the entry milestone.
- Filter system: needs the tree and entries; a hint from v1 is one central `semantics()`
  instead of duplicated classification.
- Conflict UI: uses history and changes (see the decision above); details later.
- First view: an overview of bundles with their confidence, shown by colour *and* a symbol,
  not by colour alone (a note from the drafts, not confirmed).
- A command line check without the UI (`propman check`).
- Possible dependencies, each to be asked first: `similar` (line diff for structural changes
  from outside), `toml` and `serde` (settings files).
