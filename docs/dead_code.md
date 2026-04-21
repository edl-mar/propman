# Dead code and stale scaffolding inventory

Companion to `docs/architectural_debt.md`. Items here are either clearly
deletable now or need a comment/annotation corrected. They are distinct from
the architectural debt items (D1–D9), which require design work to resolve.

Items are grouped by action required.

---

## 1. Delete immediately

These have no callers, no planned callers, and carry no useful information.

### `messages.rs:48` — `ClearFilter` variant — unbound, not dead

```rust
#[allow(dead_code)] // implemented in update.rs; no keybinding assigned yet
ClearFilter,
```

`update.rs:1086` handles this message (clears the filter `TextArea`).
It is never *constructed* because no keybinding has been assigned yet.
CLAUDE.md TODO notes: "e.g. `Ctrl+Backspace` or `X` in Filter mode".

**Action:** Add a keybinding in `keybindings.rs`. The annotation and
handler are correct; the only missing piece is the key binding itself.
This is a feature gap, not dead code.

---

### `parser.rs:16` — `raw` field on `FileEntry::Comment`

```rust
Comment { line: usize, #[allow(dead_code)] raw: String },
```

Stored on parse (lines 39, 67) but never read anywhere in the codebase.
The writer does targeted line-number-based rewrites and never needs to
reproduce comment text. Every `Comment` entry allocates a `String` for no
benefit.

**Action:** Delete the `raw` field. Update the two construction sites in
`parser.rs` to `FileEntry::Comment { line: first_line }`.

---

## 2. Stale comments to update

These comments reference types or fields that no longer exist. The code is
correct; only the comment is wrong.

### `domain.rs:104` — refers to removed `workspace.merged_keys`

```rust
/// All bundle-qualified key strings across all bundles, in insertion order.
/// Replaces `workspace.merged_keys` for key iteration in ops.
pub fn all_qualified_keys(&self) -> impl Iterator<Item = String> + '_ {
```

`workspace.merged_keys` was removed in a previous refactoring.

**Action:** Drop the second sentence; replace with:
`/// Used by ops and filter evaluation to enumerate all known keys.`

---

### `delete.rs:117` — refers to removed `merged_keys`

```rust
/// Deletes `full_key`'s entry from a single locale file, leaving all other
/// locales untouched.  The key stays in `merged_keys` — its cells for other
/// locales continue to show values; the deleted locale shows `<missing>`.
```

**Action:** Replace `merged_keys` reference:
`/// The key remains in the store — other locale cells continue to show values.`

---

### `CLAUDE.md` module table — lists `render_model` and `search`

The module table in `CLAUDE.md` still has:
```
render_model  build_display_rows() — converts bundle-qualified key slice → Vec<DisplayRow>
...
search        stub (nucleo fuzzy search planned; currently unused)
```

`search.rs` was deleted. `render_model` was renamed `view_model`
(`src/view_model.rs`). The table already has a correct `view_model` entry
added recently, making the `render_model` line a duplicate under the wrong name.

**Action:** Remove the `render_model` and `search` lines from the module table.

---

## 3. `#[allow(dead_code)]` annotations to recategorise

These items are suppressed as dead code but are actually needed scaffolding
for planned work. The annotation itself is technically correct today, but the
framing should make intent clear so they aren't mistakenly deleted.

### `view_model.rs:64` — `key_id: Option<KeyId>` — D6 migration target

```rust
#[allow(dead_code)] // public API; not yet consumed by production code
pub key_id: Option<KeyId>,
```

This is the **target field** for architectural debt item D6 (replacing
`full_key: Option<Key>` and `prefix: Key` with store IDs). It is already
populated on every row build. The only remaining step is to consume it in
`update.rs` instead of calling `identity.full_key_str()`.

**Action:** Change the annotation comment to:
```rust
// D6: consume this instead of full_key_str() — see docs/architectural_debt.md
pub key_id: Option<KeyId>,
```
Do not remove the `#[allow(dead_code)]` yet; remove it when the first
consumer is wired up.

---

### `store.rs:~301–547` — trie accessor block — needed for handle API

```rust
// ── Prepared future-API methods (trie traversal for the planned
//    store-direct rendering layer). Not yet called by DomainModel. ──────────
#[allow(dead_code)]
impl Store {
    pub fn segment_str(&self, id: GlobalSegmentId) -> &str { ... }
    pub fn bundle_ids(&self) -> impl Iterator<Item = BundleId> { ... }
    // ... 20+ methods
}
```

These are **not** dead. They are the query surface that `DomainModel` handle
methods (`BundleHandle::name()`, `KeyHandle::translation()`, etc.) will
delegate to when the handle types are implemented (architecture.md handle
section). The comment "not yet called by DomainModel" accurately describes
the current state.

**Action:** Update the block comment to make the dependency explicit:
```rust
// Store query API — called by DomainModel handle methods once implemented.
// See docs/architecture.md "Handle types" and docs/architectural_debt.md D2.
```

---

### `store.rs:~556–562` — `KeyHandle` — needed for handle API

```rust
pub struct KeyHandle<'a> {
    id: KeyId,
    store: &'a Store,
}

#[allow(dead_code)]
impl<'a> KeyHandle<'a> { ... }
```

This is the prototype for the `KeyHandle<'_>` type described in
`docs/architecture.md`. The lifetime-borrowing design has since been
acknowledged as incompatible with `ViewRow` storage, but the type is still
the right shape for short-lived read handles created inside ops.

**Action:** Update the comment:
```rust
// Short-lived read handle — created inside ops for the duration of a read,
// dropped before any &mut DomainModel call. See docs/architecture.md.
```

---

### `workspace.rs:71, 130, 143, 157` — diff-at-save scaffolding — needed for D5

Four methods marked identical:

```rust
#[allow(dead_code)]
pub fn locales(&self) -> impl Iterator<Item = &str> { ... }

#[allow(dead_code)]
pub fn get_value<'a>(&'a self, full_key: &str, locale: &str) -> Option<&'a str> { ... }

#[allow(dead_code)]
pub fn bundle_locales(&self, bundle: &str) -> Vec<String> { ... }

#[allow(dead_code)]
pub fn all_locales(&self) -> Vec<String> { ... }
```

These are scaffolding for `workspace.save(&domain_model)` — the planned
redesign that replaces `PendingChange` (D5 in `architectural_debt.md`).
The `workspace.save()` implementation will use these to enumerate files and
locate entries when applying the domain model's change set.

**Action:** Update each comment to reference the planned work:
```rust
// Used by workspace.save() — see docs/architectural_debt.md D5.
```

---

## 4. `key.rs` — large module, partially in use, scheduled for deletion

`key.rs` (~590 lines) carries the `#![allow(dead_code)]` crate-level lint
suppressor. It was written as a forward-looking trie API but was superseded
by the store. Current usage:

| Item | Where used | Status |
|------|-----------|--------|
| `Key::bundle_root()` | `view_model.rs:147` | Active — replace with `BundleId` (D6) |
| `Key::from_parts()` | `view_model.rs:169,174` | Active — replace with `KeyId`/`NodeId` (D6) |
| `Key` type (field) | `RowIdentity.full_key`, `.prefix` | Active — replace per D6 |
| `Key` type | `state.rs::anchor_key()` | Active — replace per D6/D9 |
| Everything else | nowhere | Dead scaffolding |

The `Key` methods used by `view_model.rs` and `state.rs` are the only live
surface. Everything else (`KeySegment`, `KeyPartition`, `KeyData`,
`ResolvedData`, and ~25 navigation methods) has no caller.

**Action:** Do not touch until D6 is underway. Once `RowIdentity.full_key`
and `RowIdentity.prefix` are replaced with `KeyId`/`NodeId`, the live
surface shrinks to zero and the file can be deleted. Tracked as D9 in
`architectural_debt.md`.

---

## Summary

| Action | Item | Effort |
|--------|------|--------|
| Delete | `ClearFilter` variant (`messages.rs:48`) | 1 line |
| Delete | `raw` field (`parser.rs:16`) + 2 construction sites | 3 lines |
| Update comment | `domain.rs:104` — stale `merged_keys` ref | 1 line |
| Update comment | `delete.rs:117` — stale `merged_keys` ref | 1 line |
| Update CLAUDE.md | Remove `render_model` and `search` from module table | 2 lines |
| Recategorise annotation | `view_model.rs:64` — `key_id` is D6 target | comment only |
| Recategorise annotation | `store.rs` accessor block — handle API foundation | comment only |
| Recategorise annotation | `store.rs` `KeyHandle` — handle type foundation | comment only |
| Recategorise annotation | `workspace.rs` 4 methods — D5 scaffolding | comment only |
| Defer | `key.rs` entire module — delete after D6/D9 land | major |
