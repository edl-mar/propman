# propman v2

A TUI for managing Java `.properties` translations (ratatui). The UI structure is inspired by
The Elm Architecture (messages, an update step, a view); it is not a strict implementation. A
rewrite from zero: `../v1-exploration` is a reference only and nothing is copied from it.

**Warn the user** when a design drifts away from the Elm Architecture or shows contradictory
behavior. A deviation needs a clear reason and the user's agreement first: the architecture was
not developed for fun, and every deviation must be a deliberate choice, not an accident.

This file only holds rules that were agreed or that the code already follows and that can be
checked. What is decided and what is still open is listed in
`docs/decisions-and-open-questions.md`.

## Language

Talk to the user in German. Everything in the repository (code, comments, docs, commit
messages) is English.

## Principles

**No assumptions about how the files are used.** propman does not assume how bundles are
merged, which framework or language reads them, how a project is laid out, or whether it is
under version control. Defaults are neutral. Whatever depends on usage is configured by the
user or derived from the files. propman reports facts ("key X is defined in A and B"), never
interpretations ("A wins").

*Why:* propman should be finishable. If it encoded how today's applications use properties
files (for example Spring's `ResourceBundleMessageSource`), it would need an update whenever
that changes, and a new framework or language would make it obsolete. The file format is
stable, usage is what ages.

*What we do assume:* the format of `.properties` files as read by `java.util.Properties`
(syntax, `\uXXXX` escapes, Java locale file names). Even naming conventions are only a default:
an ambiguous name is marked uncertain and the user can override it. Known language and country
codes are a hint, never a condition.

**Never guess silently.** If something cannot be decided from the files alone, take the most
likely reading, mark it as uncertain and make that visible. When names are created for files,
ask unless the assignment is certain or was set by the user.

**The roundtrip is the main guarantee.** Loading a file and saving it unchanged produces the
same bytes (encoding, BOM, line endings, missing final newline). Changing one line changes
only the bytes of that line. The golden roundtrip test (`crates/disk/tests/file.rs`) is never
committed red.

## Authority of documents

`docs/decisions-and-open-questions.md` is authoritative. `docs/drafts-and-ideas/` holds
unreviewed notes from earlier sessions: they can contradict each other and are not rules. Do not
turn a remark made in passing into a rule; if unsure whether something is decided, ask. Record a
decision in the decisions document when it is made.

Use the words of `docs/nomenclature.md` in code, docs and commit messages.

## Crates

```
propman -> tui -> app -> store -> columns
                    \-> disk
```

`disk` and `store` both use `types`, and never each other.

| Crate | Task | Must not |
|---|---|---|
| `columns` | generic index and column primitives | know anything about the project |
| `types` | plain value types shared by `disk` and `store` (`Encoding`, `LineEnding`, `Confidence`, `Fingerprint`) | contain logic beyond what defines a value, depend on another crate |
| `store` | the in-memory data, single source of truth | do IO or UI, depend on `disk` |
| `disk` | stateless file adapter: discovery, decoding, lines, atomic save, change detection | know `store` types |
| `app` | connects `store` and `disk`: load, save, conflicts, state, operations | depend on ratatui or crossterm |
| `tui` | views, rendering, input | depend on `store` or `disk` |
| `propman` | `main`, command line, startup | contain logic |

Cargo enforces the directions through the `Cargo.toml` files; change the dependencies of a crate
only when the task says so. `tui` reads through types that `app` re-exports and writes only
through operations of `app`. The internals of `store` are not designed yet; nothing in the
drafts about them is a rule.

## Rust and comments

- No `unsafe`, `unwrap()`, `expect()`, `todo!()` or `dbg!`: enforced by the workspace lints in
  `Cargo.toml`. Test files opt out of `unwrap` and `expect` explicitly with
  `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
- New dependencies go into `[workspace.dependencies]` and are used with `.workspace = true`.
  Ask before adding any external dependency.
- A comment explains why, never what. When in doubt, no comment.
- Every module that forms a boundary has a `//!` doc with its responsibility and invariants
  (2 to 5 lines). `///` only where name and signature are not enough.

## Working method

- Design first: discuss before building. For each step, write the tests as the specification
  first (they must compile and fail for the right reason), get the go-ahead, then implement.
- Before every commit all three must pass:
  ```
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
- Tests that pin the current behavior exist before a refactoring; if they are missing, write
  them first and commit them separately.
- No changes outside the crate the task concerns without saying so. No rearchitecting in passing:
  if a structure looks wrong, report it instead of changing it.
- New module boundaries, new crates and new dependencies between crates are decisions: discuss
  first, then record them in the decisions document.

## Layout of the repository

- `crates/`: the seven crates. Integration tests are in `crates/<crate>/tests/`, shared helpers
  (a temp dir without an extra dependency) in `tests/common/mod.rs`.
- `fixtures/`: static input for tests, compared byte for byte (`.gitattributes` sets `-text`).
  `edge/` holds minimal cases, one property per file; `project/` a realistic multi-bundle layout.
  Tests read only `fixtures/`.
- `playground/`: the user's sandbox, committed on purpose so that `git diff` after a save shows
  roundtrip bugs. Tests never read it; do not change it unasked.
- `tools/`: helpers outside the build, for example `Codes.java`, which regenerates the language
  and country code lists from a JDK.
- `docs/`: see above.

## Still open: do not design or anticipate

The filter system, the conflict UI, the UI for scopes, the format of the settings files, and
the order of locales or bundles. The tree and fragment views are open as well.

State: `disk` is complete for the first milestone (raw files). `store` is next.
