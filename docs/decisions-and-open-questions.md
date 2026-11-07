# Decisions and open questions

Living list, newest first within each section. A decision is only listed here once it was
agreed explicitly; everything else is an open question. Drafts in `drafts-and-ideas/` are
not authoritative.

## Decisions

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
own bundle". The choices are stored in the per-folder `.propman.toml` (no inheritance).

### No mirroring of files or bundles (2026-11-08)
propman does not keep two files or bundles in sync (for example `src/` and `target/`): when
both change, resolving the conflict would be too hard. A "copy content" function may come
much later.

## Open questions

### How are the bundles merged in the Java application? (important, 2026-11-08)
In the user's current project the application merges the entries of **all bundles into one
big map**. A key that exists in two bundles then collides: which entry wins depends on the
load order, and nobody notices.

propman currently treats bundles as separate namespaces (keys are bundle-qualified), so two
entries with the same key in different bundles sit side by side without any hint. v1 never
addressed this (checked on 2026-11-08: it only merges locale files within a bundle for
display).

To decide:
- Warn about keys that are defined in more than one bundle? Which bundles count as "the same
  map": all of them, or only those of one folder?
- Is the merge mode a per-project setting (flat map, per-bundle lookup, ...)?
- How does this interact with excluding a bundle from the session (an excluded duplicate
  should not count as a collision)?

It affects key identity in `store`, filtering and the UI, so it should be settled before the
store design is fixed.

### Symlinks
`discover` follows them, `atomic_write` replaces the link itself by a regular file, so a
shared linked file silently becomes a copy. Proposal: do not follow symlinks in `discover`
and report them as skipped. No real use case is known yet. Hard links are broken by
rename-based replacement either way; document that.

### Where do `Encoding` and `LineEnding` live between `disk` and `store`?
`disk` and `store` must not know each other, but `store` has to keep both to save a file
again. Options: own enums in both with conversion in `app`; a tiny shared types crate;
`store` generic over file metadata.

### `.propman.toml`
Name, format and which crate reads it are open; parsing probably needs a dependency (ask
first). Entries are keyed by file name; entries for missing files are kept as stale.
