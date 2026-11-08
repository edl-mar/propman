# Decisions and open questions

Living list, newest first within each section. A decision is only listed here once it was
agreed explicitly; everything else is an open question. Drafts in `drafts-and-ideas/` are
not authoritative.

## Decisions

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
- **Shape of tables and IDs.** `main17.rs` and `blueprint_v3.rs` (drafts) were experiments about
  *visibility* (owner in a private module, reader, writer), not about how the data is stored.
  Row or column (SoA) storage, the integer types of the ids, the interface through which `app`
  writes without breaking invariants, and a macro for tables are open. Lines probably become
  rows of their own, each with its own history.
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
