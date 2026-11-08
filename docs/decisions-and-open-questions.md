# Decisions and open questions

Living list, newest first within each section. A decision is only listed here once it was
agreed explicitly; everything else is an open question. Drafts in `drafts-and-ideas/` are
not authoritative.

## Decisions

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

### Where do `Encoding` and `LineEnding` live between `disk` and `store`?
`disk` and `store` must not know each other, but `store` has to keep both (and the
`Fingerprint`) to save a file again. Options: own types in both with conversion in `app`; a tiny
shared types crate; `store` generic over file metadata.

### Format of the settings files
Exact format, which crate reads and writes them, and the handling of the (probably needed)
TOML dependency are open; ask before adding a dependency. Concurrent writers can reuse
`atomic_write` and the fingerprint.
