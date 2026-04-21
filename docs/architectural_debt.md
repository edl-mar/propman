# Architectural debt

Current state of the codebase against `docs/architecture.md`.
Each item includes the rule it breaks, where the violation lives, and a
concrete suggested fix.  Items are grouped by theme because most violations
are cross-cutting.

---

## Quick-reference table

| ID | Theme | Severity | Primary files |
|----|-------|----------|---------------|
| D1 | ~~Dual-write: workspace + domain model both mutated per op~~ ✅ DONE | — | — |
| D2 | Ops receive full `AppState`, access workspace directly | Critical | all ops |
| D3 | ~~`split_key()` called outside `domain.rs`~~ ✅ DONE | — | — |
| D4 | Ops call each other (rename → insert + delete) | High | rename.rs |
| D5 | ~~`PendingChange` / `pending_writes` deferred-write mechanism~~ ✅ DONE | — | — |
| D6 | ~~`Key` / `prefix: Key` in `RowIdentity` — `key_id` present but unused~~ ✅ DONE | — | — |
| D7 | ~~Ops read `view_rows` to extract string keys~~ ✅ DONE | — | — |
| D8 | `dirty_keys` / `pinned_keys` / `temp_pins` live on AppState as strings | Medium | state.rs |
| D9 | ~~`key.rs` types (`Key`, `KeySegment`, `KeyPartition`, …) still imported~~ ✅ DONE | — | — |
| D10 | Store model: no Entry entity, no interned translations, no change log | Foundation | store.rs, domain.rs |

**All D1–D10 items are now resolved.** The codebase is architecturally clean.

---

## D1 — ✅ DONE — Dual-write: workspace and domain model both mutated

**Rule:** Domain model is the single source of truth. Workspace is mutated
only at save time by `workspace.save()`.

Every op currently does the same two-step dance:

1. Mutate `state.workspace` (shift line numbers, retain/push `FileEntry`)
2. Mirror the same change into `state.domain_model`

This is backwards: the domain model should be mutated first; the workspace
should derive writes from it at save time.

**delete.rs — `delete_key_inner`**
```rust
// Lines 43-72: direct workspace mutation first
state.workspace.groups[*gi].files[*fi].entries.retain(|e| { ... });
// ... line-number shifting ...

// Lines 74-77: then the mirror
if let Some(key_id) = state.domain_model.find_key(full_key) {
    state.domain_model.delete_key(key_id);
}
```

**insert.rs — `insert_into_file`**
```rust
// Lines 33-38: workspace push first
state.workspace.groups[gi].files[fi].entries.push(FileEntry::KeyValue { ... });

// Lines 57-60: then the mirror
let key_id = state.domain_model.insert_key(&bundle, real_key);
state.domain_model.set_translation(key_id, &locale, value.to_string());
```

**rename.rs — `rename_key_in_workspace`**
```rust
// Lines 346-358: workspace key rename first
for group in &mut state.workspace.groups { ... *key = new_real.to_string(); ... }

// Lines 362-364: then the mirror
if let Some(key_id) = state.domain_model.find_key(old_key) {
    state.domain_model.rename_key(key_id, new_key);
}
```

**Fix (target state):**
```rust
// Op calls domain model only:
state.domain_model.delete_key(key_id);
// store records KeyMutation::Deleted internally

// At Ctrl+S:
workspace.save(&state.domain_model);
// workspace reads change_set(), locates FileEntry positions, writes diffs
```

The workspace stays as the pristine on-disk image until a save. No line-number
bookkeeping in ops at all.

---

## D2 — Ops receive full `AppState`

**Rule:** Ops call `DomainModel` methods only; they must not access workspace
fields or view state.

All three op files import and receive `&mut AppState`:
```rust
// delete.rs:3, insert.rs:3, rename.rs (implicit via AppState)
use crate::state::{AppState, PendingChange};

pub fn delete_key_inner(state: &mut AppState, full_key: &str) { ... }
```

Because ops receive `AppState`, they can (and do) reach into
`state.workspace`, `state.dirty_keys`, `state.pending_writes`,
`state.view_rows`, and `state.unsaved_changes`.

**Fix (incremental path):**

Once D1 is resolved the op signatures narrow naturally:
```rust
// Target: ops only need DomainModel
pub fn delete_key(dm: &mut DomainModel, key: KeyId) { ... }
pub fn insert_key(dm: &mut DomainModel, bundle: BundleId, real_key: &str,
                  locale: GlobalLocaleId, value: &str) { ... }
pub fn rename_key(dm: &mut DomainModel, key: KeyId, new_real_key: &str) { ... }
```

Tracking fields (`dirty_keys`, `unsaved_changes`) move to domain model
exception tables. `pending_writes` is eliminated (D5).

---

## D3 — ✅ DONE — `split_key()` called outside `domain.rs`

**Rule:** `workspace::split_key()` is allowed only inside `domain.rs` at the
two named string entry points (`find_key`, `insert_key`).

The `domain.rs` call at line 141 (`find_key`) **is compliant** — it is one of
the two permitted locations.

All other calls are debt:

| File | Line(s) | Function |
|------|---------|----------|
| `delete.rs` | 14 | `delete_key_inner` |
| `delete.rs` | 120 | `delete_locale_entry` |
| `insert.rs` | 68 | `apply_cell_value` |
| `insert.rs` | 153, 234, 240 | `commit_cell_edit`, `commit_cell_insert` |
| `rename.rs` | 13, 14, 41, 42 | `commit_exact_rename`, `commit_prefix_rename` |
| `rename.rs` | 124, 125, 160, 161 | `commit_cross_bundle_rename`, `commit_cross_bundle_prefix_rename` |
| `rename.rs` | 198, 199, 224, 225, 258, 259, 296, 297, 325, 326 | loops and helpers |

**Pattern:**
```rust
// WRONG — in ops
let (bundle, real_key) = workspace::split_key(full_key);
```

**Fix:**
When ops are rewritten to accept `KeyId` / `BundleId` (D2), the key is
already resolved — `split_key` calls disappear entirely. For cases that
still need to split a user-input string (e.g. `new_real_key` in rename),
that split happens in `update.rs` before calling the op, using a
`DomainModel` method:
```rust
// update.rs: parse user input string once
let (bundle_id, real_key) = state.domain_model.parse_key_str(&input)?;
ops::rename::rename_key(&mut state.domain_model, key_id, real_key);
```

---

## D4 — Ops calling each other

**Rule:** Ops must not call each other. Shared logic belongs in `domain.rs`
or `ops/common.rs`.

`rename.rs` calls into both `insert` and `delete`:

**rename.rs:113** — inside `snapshot_and_insert`:
```rust
ops::insert::apply_cell_value(state, &dest_full_key, &locale, value);
```

**rename.rs:144, 204** — inside `commit_cross_bundle_rename` and
`commit_cross_bundle_prefix_rename`:
```rust
ops::delete::delete_key_inner(state, old_key);
```

**Fix:**
`snapshot_and_insert` is logically a domain model operation (copy
translations from one key to another). Move it to `DomainModel`:
```rust
// domain.rs
pub fn copy_translations(&mut self, src: KeyId, dest_bundle: BundleId,
                          dest_real_key: &str) -> Vec<GlobalLocaleId> { ... }
```
`delete_key_inner` is already a thin wrapper; once ops accept `KeyId`
and call `dm.delete_key(id)` directly, there is no inner helper to call.

---

## D5 — ✅ DONE — `PendingChange` / `pending_writes`

**Rule:** The store tracks mutations internally (`KeyMutation` map, `deleted`
set, `segment_renames` map). `workspace.save()` derives writes from the
change set. No caller-assembled `PendingChange` queue.

Current state: every op assembles and pushes `PendingChange` entries:

```rust
// delete.rs:66
state.pending_writes.push(PendingChange::Delete {
    path: path.clone(), first_line: *fl, last_line: *ll,
    full_key: full_key.to_string(),
});

// insert.rs:47
state.pending_writes.push(PendingChange::Insert {
    path, after_line, key: real_key.to_string(), value: value.to_string(), full_key,
});

// rename.rs:369
state.pending_writes.push(PendingChange::Update { path, first_line, last_line,
    key: new_real.to_string(), value, full_key: new_key.to_string(),
});
```

`state.rs:14-41` defines the `PendingChange` enum and
`state.pending_writes: Vec<PendingChange>`.
`update.rs` flushes them at `Message::SaveFile`.

**Fix:**
This is the largest single piece of work. It requires:
1. Implementing `KeyMutation::Inserted / Deleted / Modified` tracking in
   `Store` (designed in `docs/store.md`).
2. Implementing `DomainModel::change_set()` that returns an iterator over
   `Change` (the replacement for `PendingChange`, derived from the store).
3. Implementing `workspace.save(&domain_model)` to consume the change set
   and write minimal file diffs.
4. Deleting `PendingChange`, `state.pending_writes`, and all `.push()` calls.

Until this is done every `PendingChange::push` is a marker of this debt.

---

## D6 — ✅ DONE — `Key` / `prefix: Key` in `RowIdentity`; `key_id` present but unused

**Rule:** `ViewRow` carries identity IDs (`KeyId`, `BundleId`, `NodeId`),
not legacy `Key` objects.

`view_model.rs:56-80` — `RowIdentity` currently carries:
```rust
pub full_key: Option<Key>,   // legacy — generates string via to_qualified_string()
pub key_id:   Option<KeyId>, // target — already present, marked #[allow(dead_code)]
pub prefix:   Key,           // legacy — bundle + segments as a Key object
```

The `prefix: Key` field has no ID counterpart yet. It is used for:
- Bundle-header detection (`prefix.is_bundle_root()`)
- Ancestor-row lookup in Left-navigation (cursor segment walking in `state.rs`)
- Scope highlighting (descendant prefix matching)

**Partial migration already done:** `key_id` is present. Consumption is
the remaining step.

**Fix:**
1. Start using `identity.key_id` in all call sites that currently call
   `identity.full_key_str()` — callers in `insert.rs` (D7) are the most
   important.
2. Add `bundle_id: BundleId` and `node_id: NodeId` to `RowIdentity` to
   replace `prefix: Key`. The node_id is the trie node for the row's
   anchor point; bundle_id is its bundle.
3. Rewrite `is_bundle_header()`, `prefix_str()`, `bundle_name()` to use
   the new IDs.
4. Remove `full_key: Option<Key>` and `prefix: Key` once all consumers
   are migrated.

---

## D7 — ✅ DONE — Ops read `view_rows` to extract string keys

**Rule:** Ops must not access view state. `ViewRow` string fields must not
be passed to ops.

`insert.rs` — two functions read the cursor row directly from `AppState`:

**`commit_cell_edit` (lines 145–149):**
```rust
let full_key = match state.view_rows.get(state.cursor_row)
    .and_then(|r| r.identity.full_key_str())   // ← Key → String
{
    Some(k) => k,
    None => return,
};
```

**`commit_cell_insert` (lines 223–228):**
```rust
let full_key = match state.view_rows.get(state.cursor_row)
    .and_then(|r| r.identity.full_key_str())   // ← Key → String
{
    Some(k) => k,
    None => return,
};
```

Both functions also read `state.effective_locale_idx()` and
`state.visible_locales` — view / navigation state — from inside an op.

These functions are misplaced: they contain cursor-state reading that
belongs in `update.rs`, not in ops.

**Fix:**
Move cursor-state reading to `update.rs`; pass concrete IDs to the op:
```rust
// update.rs
let key_id  = state.view_rows[state.cursor_row].identity.key_id?;
// key_id is already in RowIdentity — just not yet consumed
let locale_id = state.visible_locale_id(state.effective_locale_idx()?)?;
ops::insert::set_translation(&mut state.domain_model, key_id, locale_id, new_value);
```
The op receives `KeyId` + `GlobalLocaleId` + `value` — no view state access.

---

## D8 — `dirty_keys` / `pinned_keys` / `temp_pins` on AppState as strings

**Rule:** Exception sets (dirty, pinned, temp_pins) live in the `Store` and
are accessed through `DomainModel`.

Current `AppState` fields (`state.rs:~206–220`):
```rust
pub dirty_keys:  HashSet<String>,  // bundle-qualified key strings
pub pinned_keys: HashSet<String>,
pub temp_pins:   Vec<String>,
```

These shadow the `Store`'s exception tables (`store.dirty: HashSet<KeyId>`,
etc.) using strings instead of IDs. Every mutation op also writes to
`state.dirty_keys` directly:
```rust
// delete.rs:79, insert.rs:46, rename.rs:367
state.dirty_keys.insert(full_key.to_string());
state.dirty_keys.remove(full_key);
```

**Fix:**
Once D1/D5 land and the store tracks `KeyMutation` internally:
- `dirty` state is `store.mutations.contains_key(&key_id)` — no separate
  `HashSet<String>` needed on `AppState`.
- `pinned_keys` and `temp_pins` can become `HashSet<KeyId>` inside the
  store's exception tables, accessed via `DomainModel`.
- The `enrich_rows` call in `view_model.rs` that currently receives
  `dirty_keys: &HashSet<String>` will switch to asking `dm.is_dirty(key_id)`.

---

## D9 — ✅ DONE — `key.rs` types still imported and used

**Rule:** `Key`, `KeySegment`, `KeyPartition`, `KeyData`, `ResolvedData`
must not be used. Every usage is debt.

**`state.rs:8`**
```rust
use crate::key::Key;
```
Used in `anchor_key() -> Key` (lines 281–289) and sibling navigation.

**`view_model.rs:5`**
```rust
use crate::key::Key;  // (also domain::KeyId, store IDs)
```
Used in `RowIdentity.full_key: Option<Key>` and `RowIdentity.prefix: Key`.

No direct use was found in `update.rs` or ops — they consume `Key` objects
transitively through `view_rows`.

**Fix:**
Completing D6 (replacing `full_key`/`prefix` with IDs in `RowIdentity`)
removes the `view_model.rs` usage. Completing the cursor-navigation
refactor in `state.rs` (using `node_id` from `ViewRow` instead of
constructing `Key` objects) removes the `state.rs` usage.
Once both are done, `key.rs` can be deleted.

---

## Cross-cutting concerns

### The `insert_into_file` (gi, fi) index pattern

`insert.rs:10` — `insert_into_file(state, gi, fi, real_key, value)` uses
raw `(gi, fi)` group/file index pairs threaded through the call stack.
These are fragile: they become stale if any group or file is added/removed
before the function is called. This pattern is a consequence of D2 (ops
accessing workspace directly) and will dissolve when D1 is resolved.

### New locale / bundle file creation

Currently `ensure_locale_file()` creates files **immediately** on disk
(not deferred). This is a structural change (new file, not a key mutation)
that sits outside the `KeyMutation` model.

In the target architecture `workspace.save()` handles all disk writes, so
new-file creation needs to be represented in the change set. The `Change`
type (replacing `PendingChange`) will need a `CreateFile { path, bundle,
locale }` variant alongside `Insert / Delete / Update`.

This is a design gap in the current `docs/store.md` and
`docs/architecture.md` — it should be addressed before implementing D5.

### Same-bundle rename: in-place file rewrite deferred

D10 Phase B implements same-bundle rename as **delete + insert** at the file level,
meaning the key may shift to its alphabetical position in the file after save.
The correct behaviour (in-place key-string rewrite, preserving original line order)
requires a `Change::RenameKey { old_key_id, new_key_id }` variant in `change_set()`
and a matching code path in `workspace.save()`. The store already tracks per-entry
original/current values; the missing piece is a `renamed_keys: Vec<(KeyId, KeyId)>`
field (populated by `DomainModel::rename_key` for same-bundle moves) and workspace
logic to rewrite key strings in-place rather than delete+insert.
Track as **D11** when scheduling.

### The `snapshot_and_insert` cross-op flow (rename → insert → delete)

The cross-bundle rename path in `rename.rs` does:
1. Call `snapshot_and_insert` → calls `ops::insert::apply_cell_value`
2. Call `ops::delete::delete_key_inner`

This is the most complex violation of D4 and drives some of D3. The
refactor target is a single `DomainModel::move_key(src_key, dest_bundle,
dest_real_key) -> MoveResult` that handles the snapshot, insert, delete,
and missed-locale reporting entirely inside the domain layer.

---

## D10 — Store model redesign: Entry table, interned translations, change log

**Replaces** the "Planned mutation model" in `docs/store.md` and subsumes D5 and D8.
**Must land after** D6–D2 (ops isolated from store internals), because D10 restructures
`store.rs` and `domain.rs` in place — it is the test of whether those layers are
isolated enough to absorb deep internal change without rippling outward.

---

### Three entities

The current store conflates two things under `KeyId`: the cross-locale named concept
and the locale-specific file line. This makes mutation tracking awkward and forces
workspace write logic to hunt for file positions at save time.

The target model separates them cleanly:

| Entity | ID | What it is |
|---|---|---|
| **Key** | `KeyId` | Named concept in the trie; exists once per bundle path |
| **Entry** | `EntryId` | Locale-specific instantiation; one per `(key, locale)` pair; maps to exactly one line in one file |
| **Translation** | `GlobalTranslationId` | Globally interned string value; multiple entries can share one ID |

---

### New store fields

```rust
// --- Entry table ---
entry_key:                  Vec<KeyId>,
entry_locale:               Vec<GlobalLocaleId>,
entry_original_translation: Vec<GlobalTranslationId>,  // load-time; never mutated
entry_current_translation:  Vec<GlobalTranslationId>,  // updated on mutation

// --- Entry lookup ---
key_entries: HashMap<KeyId, Vec<EntryId>>,
cell_entry:  HashMap<(KeyId, GlobalLocaleId), EntryId>,  // fast cell lookup

// --- Entry exception sets ---
deleted_entries:  HashSet<EntryId>,  // non-destructive deletion
inserted_entries: HashSet<EntryId>,  // entries with no original file backing

// --- Key exception sets (maintained in sync with key_mutations in change log) ---
deleted_keys:  HashSet<KeyId>,  // fast O(1) key_visible predicate
inserted_keys: HashSet<KeyId>,  // keys with no file backing yet

// --- Global translation pool (interned, never shrinks) ---
translation_values: Vec<String>,
translation_index:  HashMap<String, GlobalTranslationId>,  // deduplication
translation_users:  HashMap<GlobalTranslationId, Vec<EntryId>>,  // for global propagation

// --- Ordered change log ---
change_log:     Vec<ChangeRecord>,   // indexed by ChangeId; append-only within session
key_change_log: HashMap<KeyId, Vec<ChangeId>>,  // per-key chronological index
```

`cell_translations: HashMap<(KeyId, GlobalLocaleId), String>` is deleted entirely.

---

### Change log structure

```rust
pub struct ChangeId(u32);

pub struct ChangeRecord {
    pub entry_mutations:   Vec<EntryMutation>,
    pub segment_mutations: Vec<SegmentMutation>,
    pub key_mutations:     Vec<KeyMutation>,
    pub file_mutations:    Vec<FileMutation>,
}

pub struct EntryMutation {
    pub entry_id: EntryId,
    pub before:   Option<GlobalTranslationId>,  // None = entry did not exist
    pub after:    Option<GlobalTranslationId>,  // None = entry deleted
}

pub struct SegmentMutation {
    pub node_id: NodeId,
    pub before:  GlobalSegmentId,
    pub after:   GlobalSegmentId,
}

pub struct KeyMutation {
    pub key_id: KeyId,
    pub kind:   KeyMutationKind,
}

pub enum KeyMutationKind {
    Inserted,
    Deleted,
    /// Same-bundle exact rename: workspace rewrites the key string in place
    /// rather than deleting the old line and appending a new one.
    /// Cross-bundle rename is represented as Deleted (old) + Inserted (new).
    RenamedFrom { old_key_id: KeyId },
}

pub struct FileMutation {
    pub bundle_id: BundleId,
    pub locale_id: GlobalLocaleId,
    pub kind:      FileMutationKind,
}

pub enum FileMutationKind { Created }
```

Every user-level operation (edit cell, rename segment, delete key, create locale file)
appends exactly one `ChangeRecord` to `change_log` and updates `key_change_log` for all
affected keys.

---

### Global vs local translation change

Because translations are globally interned, two entries that happen to contain the same
string share a `GlobalTranslationId`. When the user edits a cell they can choose scope:

- **Local**: intern the new string → get (or create) its `GlobalTranslationId` → update
  only `entry_current_translation[this_entry]`. One `EntryMutation` in the record.
- **Global**: same new ID, but update `entry_current_translation` for **all** entries in
  `translation_users[old_tid]`. One `ChangeRecord` with N `EntryMutation`s.

The reverse index `translation_users` makes global propagation O(n affected entries)
rather than a full table scan.

---

### Dirty tracking (replaces D8)

`is_dirty(key_id)` = `key_change_log.contains_key(&key_id)`.

No `dirty_keys: HashSet<String>` on `AppState`. No `HashSet<KeyId>` in the store.
Dirty is derived from the change log — if a key has any change records it is dirty.

`pinned_keys` and `temp_pins` remain on `AppState` but migrate from `HashSet<String>`
to `HashSet<KeyId>` / `Vec<KeyId>` as part of this work (no string identity needed once
entries carry stable IDs throughout).

---

### `change_set()` for workspace.save() (replaces D5)

```rust
// domain.rs
pub fn change_set(&self) -> impl Iterator<Item = Change> + '_;
```

Derived by diffing `entry_original_translation` vs `entry_current_translation`.
The exception-set check must come first — `entry_original_translation` is undefined
for `inserted_entries` and must not be read for them:

```
1. For each entry in inserted_entries  → Change::InsertTranslation { entry_id, translation_id }
2. For each entry in deleted_entries   → Change::DeleteTranslation { entry_id }
3. For all other entries where
   entry_current_translation[e]
     != entry_original_translation[e]  → Change::UpdateTranslation { entry_id, translation_id }
4. For each RenamedFrom { old } key    → Change::RenameKey { new_key_id, old_key_id }
   (workspace rewrites key string in the existing file entries for old_key_id)
5. For each SegmentMutation            → Change::RenameSegment { node_id, before_segment_id }
   (workspace fans out to all entries under the renamed node)
6. For each FileMutation::Created      → Change::CreateFile { bundle_id, locale_id }
```

**Ordering invariant**: `CreateFile` changes must be yielded before any
`InsertTranslation` for the same `(bundle_id, locale_id)`, so workspace creates
the file before trying to write entries into it. workspace may also do a two-pass:
collect and execute all `CreateFile` entries first, then process the rest.

`workspace.save(&dm)` consumes the iterator, looks up file positions from the workspace's
own `FileEntry` data (keyed by bundle + locale + real key string), writes minimal diffs,
then calls `dm.clear_mutations()` which resets the change log and copies
`entry_current_translation` into `entry_original_translation` — after which the app
reloads from disk for a clean slate.

---

### Implementation invariants

These must hold after every mutation call — they are not derived lazily.

**`translation_users` stays in sync**: when `entry_current_translation[e]` changes
from `old_tid` to `new_tid`, remove `e` from `translation_users[old_tid]` and add
it to `translation_users[new_tid]`. This is what makes global propagation O(n affected
entries) rather than a full scan.

**`key_change_log` is updated for all keys affected by a `SegmentMutation`**: when a
segment is renamed, eagerly walk all keys under the renamed node and push the new
`ChangeId` to each key's log. This is O(n keys under node) at commit time, but it
ensures `is_dirty(key_id)` is correct without a log scan.

**Exception sets stay consistent with key_mutations**: every `KeyMutationKind::Inserted`
pushed to the change log adds `key_id` to `inserted_keys`; every `Deleted` adds to
`deleted_keys`; every `RenamedFrom` adds the new key to `inserted_keys` and the old
key to `deleted_keys`. On undo these additions are reversed.

**State machine for net no-ops**: if an entry in `inserted_entries` is subsequently
deleted in the same session, remove it from both `inserted_entries` and `deleted_entries`
— it was never on disk so no write is needed. Same for a key in `inserted_keys` that is
then deleted: remove from both sets and emit no `Change` for it.

---

### What this eliminates

| Removed | Replaced by |
|---|---|
| `cell_translations: HashMap<(KeyId, GlobalLocaleId), String>` | entry table + translation pool |
| `PendingChange` enum + `pending_writes: Vec<PendingChange>` | `change_log` + `change_set()` diff |
| `dirty_keys: HashSet<String>` on `AppState` | `key_change_log.contains_key` |
| `dirty: HashSet<KeyId>` planned for store | same |
| `(gi, fi, first_line, last_line)` index tuples in ops | workspace looks up FileEntry at save time |
| Manual line-number shifting in ops | gone entirely |

---

## Suggested fix order

Dependencies drive this sequence.

1. **D6** ✅ — `node_id: NodeId` + cached string fields in `RowIdentity`; `key_id` consumed;
   `Key` objects removed from view_model.rs and state.rs navigation.
2. **D9** ✅ — `key.rs` deleted; `mod key` removed from main.rs.
3. **D7** ✅ — ops already accepted `DomainModel` + `KeyId`; no view_rows access in ops.
4. **D3** ✅ — `split_key` moved to `domain.rs` as `pub fn split_key`; `workspace::split_key`
   made private; all callers (rename.rs, common.rs, update.rs) now use `domain::split_key`.
5. **D4** ✅ — `snapshot_and_insert` logic moved to `DomainModel::copy_translations_to`.
6. **D2** ✅ — ops now take `&mut DomainModel`.
7. **D10** ✅ — entry table + change log; `pinned_keys`/`temp_pins` migrated to `KeyId`-based
   store exception tables; subsumes D5 and D8.

**D5 and D8 are not separate steps** — they are implemented as part of D10.
