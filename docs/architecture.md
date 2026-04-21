# Architecture rules

This document is the authoritative guide for refactoring and extending propman.
Agents working on this codebase must read it before touching any non-trivial code.

---

## The invariant

**`DomainModel` (backed by `Store`) is the single source of truth for all key
and value data.**

All other modules derive their view of the data from it. No module other than
`domain.rs` and `store.rs` may own or mutate key/value data directly.

---

## Layer hierarchy

Strict top-to-bottom dependency. Lower layers must never import from higher
layers.

```
┌─────────────────────────────────┐
│  workspace / parser / writer    │  I/O layer — filesystem only
└────────────────┬────────────────┘
                 │ loads at startup → feeds Store; saves at Ctrl+S ← reads Store
┌────────────────▼────────────────┐
│  store.rs                       │  data layer — IDs, trie, translations, mutations
└────────────────┬────────────────┘
                 │ all access mediated through DomainModel
┌────────────────▼────────────────┐
│  domain.rs                      │  logic layer — handles, queries, mutations
└────────────────┬────────────────┘
                 │ typed handles only; no strings, no IDs
┌────────────────▼────────────────┐
│  ops/  (delete, insert, rename) │  operation layer — one concern per file
└────────────────┬────────────────┘
                 │ operate on handles; never touch workspace
┌────────────────▼────────────────┐
│  update.rs                      │  dispatch layer — Message → op call
└────────────────┬────────────────┘
                 │ read-only; no mutations
┌────────────────▼────────────────┐
│  view_model.rs / widgets.rs     │  view layer — display only
└─────────────────────────────────┘
```

`AppState` (`state.rs`) is horizontal infrastructure — it holds the
`DomainModel`, navigation state, and the workspace. It sits alongside
update.rs, not above or below it.

`filter.rs` is a pure function library with no layer position — it may be used
anywhere.

---

## The string boundary rule

**Strings exist only at I/O boundaries.**

| Layer | Allowed | Forbidden |
|---|---|---|
| `workspace` / `parser` / `writer` | string keys, paths, values | — |
| `store.rs` | string params on public API (`insert_key`, `register_locale`) | string key manipulation inside |
| `domain.rs` | string params at the two named entry points (see below) | string key splitting/building anywhere else |
| `ops/` | **no strings** — receive and return handles | `workspace::split_key()`, `format!("{bundle}:{key}")`, etc. |
| `update.rs` | string from user input (TextArea) only | constructing or splitting key strings |
| `view_model.rs` | strings for display (segment labels, cell values) | passing strings back up to ops |

The two permitted string-to-ID translation points in `domain.rs`:
- `find_key(full_key_str) -> Option<KeyHandle>` — lookup by string
- `insert_key(bundle_str, real_key_str) -> KeyHandle` — creation

All other `domain.rs` methods accept and return handles or `DomainModel`-owned
data. `workspace::split_key()` may only be called inside these two entry points.

---

## Handle types

**Handles borrow `DomainModel` and therefore cannot be stored in owned
structs like `ViewRow`, and cannot be passed alongside `&mut DomainModel`.**
For stored identity and for op parameters, use IDs. For method dispatch,
create handles on demand and drop them before any mutation call.

```
stored / passed as params  →  KeyId / BundleId / NodeId  (Copy, no lifetime, opaque)
created on demand (local)  →  KeyHandle / BundleHandle …  (borrows &DomainModel)
```

A handle is a thin wrapper over an ID plus a borrow of `DomainModel`. It
exposes typed methods so callers never call `store.method(id)` directly and
never see the underlying integer.

| Handle | What it represents | Key methods |
|---|---|---|
| `KeyHandle<'_>` | a translatable key | `.bundle() -> BundleHandle`, `.segments() -> Vec<SegmentHandle>`, `.translation(locale) -> Option<&str>`, `.is_dirty()`, `.is_deleted()` |
| `BundleHandle<'_>` | a bundle | `.name() -> &str`, `.locales() -> impl Iterator<Item = LocaleHandle>`, `.keys() -> impl Iterator<Item = KeyHandle>` |
| `LocaleHandle<'_>` | a locale within a bundle | `.name() -> &str` |
| `SegmentHandle<'_>` | a key-path segment within one bundle | `.label() -> &str` |
| `GlobalSegmentHandle<'_>` | a segment shared across all bundles | `.label() -> &str` |

Rules for handles:
- Constructed only inside `domain.rs` via methods like `dm.key_handle(id)`.
- Short-lived: created locally for reading, never stored, never passed as
  function parameters.
- Since handles borrow `&DomainModel`, they cannot coexist with a
  `&mut DomainModel` borrow in the same scope. Pattern: create a handle,
  read what you need, drop it, then call the mutation with the ID.

### Where IDs are allowed outside store/domain

IDs are opaque newtypes — callers cannot construct them from raw integers,
only receive them from the domain model API. Passing them as stable identity
tokens is safe and intentional in these places:

| Location | Allowed IDs | Reason |
|---|---|---|
| `ViewRow` identity fields | `KeyId`, `BundleId`, `NodeId` | `ViewRow` is owned; handles can't be stored due to lifetime |
| `update.rs` dispatch | same, read from `ViewRow` | Passed to ops or converted to a handle for short-lived reading |
| op function parameters | `KeyId`, `BundleId`, `GlobalLocaleId` | Handles can't be passed alongside `&mut DomainModel`; ops receive IDs and create handles locally |

Everywhere else (filter, workspace, writer) IDs must not appear.

---

## Workspace role

The workspace has exactly two responsibilities:

1. **Load** — at startup, parse every `.properties` file and populate `Store`
   via `Store::from_workspace(&workspace)`. After this call the workspace is
   the on-disk image of the data; the store is the in-memory image.

2. **Save** — at Ctrl+S, ask the `DomainModel` for its change set and write
   the minimal diffs to disk.

The workspace knows *where* in the files things live (original line numbers,
insertion-point heuristics for new keys) so that saves produce small, readable
diffs. The domain model knows *what* changed. Neither needs the other's
information beyond these two calls.

```
workspace.save(&domain_model)
  → domain_model.change_set() → Iterator<Change>
  → for each Change: locate lines in workspace entries, write minimal diff
```

`Change` covers the same cases as the old `PendingChange` but is derived from
the domain model's mutation tracking (`KeyMutation::Inserted / Deleted /
Modified`), not assembled piecemeal by callers.

**What this eliminates:**
- `pending_writes: Vec<PendingChange>` on `AppState` — gone.
- Manual line-number bookkeeping in ops (shifting subsequent entries after an
  insert/delete) — the workspace handles this internally at save time.
- Ops queuing `PendingChange` entries — ops only call `DomainModel` methods.

---

## Module rules

### `store.rs`

- Owns IDs, intern tables, trie structure, translations, and exception sets.
- Tracks all in-session mutations: `mutations: HashMap<KeyId, KeyMutation>`,
  `deleted: HashSet<KeyId>`, `segment_renames: HashMap<NodeId, GlobalSegmentId>`.
  See `docs/store.md` for the full planned mutation model.
- The only module that may construct ID values.
- Has no concept of UI, filter expressions, cursor, or modes.
- No dependencies on any other app module.

### `domain.rs`

- The only module that calls `Store` methods directly.
- Exposes handles — callers never receive raw IDs.
- Mutation methods take handles and update the store's mutation tracking:
  `dm.delete_key(handle)` marks the key `Deleted` in the store; it does not
  modify the workspace or queue any write.
- The string entry points (`find_key`, `insert_key`) are the only place
  `workspace::split_key()` may be called.
- Does not access `AppState`, `workspace` (except at build time), `view_model`,
  or `ops`.

### `ops/delete.rs`, `ops/insert.rs`, `ops/rename.rs`

- Each file handles exactly one concern. They must not call each other.
- If two ops share logic, extract that logic into `domain.rs` (belongs to the
  data model) or a private `ops/common.rs` (purely structural coordination).
- Accept handles from update.rs — never a key string, never a raw ID.
- Call `DomainModel` methods to mutate data. That is the complete mutation path.
- Do not access `workspace.groups` or `workspace.files` at all. The workspace
  is consulted only by `workspace.save()` at flush time.
- Do not use raw `(gi, fi, first_line, last_line)` index tuples anywhere.

### `update.rs`

- Pure dispatch: match on `Message`, call the right op or state method.
  No business logic lives here.
- The only place where user-input strings (from `TextArea`) are parsed into
  structured data. Parsing produces a handle or typed value which is passed
  to ops.
- Does not access `workspace.groups` or `workspace.files` directly.
- Reads `view_rows[cursor_row]` to obtain IDs (`key_id`, `bundle_id`,
  `node_id`). This is the correct and intended path. Those IDs are passed
  to ops or used to create short-lived handles for local reading.
  What is forbidden is reading a *string* field from `ViewRow` and passing
  it to ops or to `DomainModel`.

### `state.rs` / `AppState`

- Navigation state: cursor row/segment/locale, scroll offset, mode, visible
  locales, selection scope, clipboard, UI flags.
- Holds `domain_model: DomainModel` and `workspace: Workspace` as owned fields.
  No `pending_writes` — the domain model tracks what changed.
- `apply_filter` rebuilds `view_rows` from `domain_model`. It is the only place
  that calls `view_model::build_view_rows`.
- `cursor_key_for_ops()` returns a `KeyId` (not a `String`). Returning a
  `KeyHandle` would be a borrow conflict — the handle would borrow
  `domain_model` while the caller needs `&mut domain_model` to mutate.
- Navigation methods (`move_up`, `move_down`, `clamp_scroll`) do not mutate
  `domain_model`.

### `view_model.rs`

- Read-only. Reads `DomainModel` and state flags; never mutates anything.
- Produces `Vec<ViewRow>` for the renderer — derived, not authoritative.
- Each `ViewRow` carries **two kinds of fields**:
  - *Identity IDs*: `key_id: Option<KeyId>`, `bundle_id: BundleId`, `node_id:
    NodeId` — the stable opaque tokens that `update.rs` reads and passes to
    ops, or uses to create short-lived local handles for reading. Handles
    cannot be stored here because they borrow `DomainModel`.
  - *Display strings*: segment label strings, cell value strings, flags.
    These are rendering artifacts — they must not be parsed back into IDs,
    handles, or fed to ops.
- `update.rs` reading `view_rows[cursor_row].key_id` is the *correct* way to
  find the current cursor entity. What is forbidden is reading a string field
  from `ViewRow` and passing it to ops.
- Dirty, pinned, and dangling flags are derived at build time from the domain
  model's mutation and exception sets; they are not independently tracked here.

### `workspace.rs`

- Loads `.properties` files at startup; exposes the resulting `Workspace`
  struct for bootstrapping `Store`.
- At save: `workspace.save(&domain_model)` is the single write entry point.
  It asks the domain model for its change set, computes minimal diffs using
  in-memory line positions, and writes to disk.
- `split_key()` is an internal string utility — only called inside `domain.rs`
  at the named entry points. Not called in ops or update.
- After startup, workspace is not mutated to reflect in-memory changes; it
  stays as the on-disk image until a successful save.

---

## Mutation flow

```
User keystroke
  → Message  (keybindings.rs)
  → update.rs reads key_id from view_rows[cursor_row].key_id
  → ops function receives key_id (and &mut DomainModel)
  → ops creates short-lived handle if it needs to read:
       let h = dm.key_handle(key_id);  // read what's needed, then drop h
  → ops calls domain_model.delete_key(key_id)
               / domain_model.set_translation(key_id, locale_id, value)
               / domain_model.rename_key(key_id, new_name)
  → store records the mutation in its tracking tables
     (mutations map, deleted set, segment_renames map)
  → apply_filter() rebuilds view_rows from updated domain model

Ctrl+S
  → workspace.save(&domain_model)
  → domain model exposes change_set() iterator
  → workspace locates file positions, writes minimal diffs
  → domain model clears flushed mutations from tracking tables
```

There is no `pending_writes` queue. There are no `PendingChange` entries
assembled by callers. Mutation tracking is the store's job; write-back is the
workspace's job.

---

## Legacy types scheduled for deletion

### `key.rs` — `Key`, `KeySegment`, `KeyPartition`, `KeyData`, `ResolvedData`

`key.rs` predates the store and must be deleted as part of the refactoring.
It was written to give keys a typed identity before `KeyId` and trie traversal
existed. Every responsibility it has is now covered by the store:

| `key.rs` concept | Replacement |
|---|---|
| `Key` (bundle + segments + metadata) | `KeyId` stored in `ViewRow`; handle created on demand |
| `KeySegment` | `GlobalSegmentId`; label via `handle.label()` |
| `KeyPartition` | `store.key_compressed(key_id)` → `Vec<Vec<GlobalSegmentId>>` |
| `ResolvedData.is_leaf` | `!store.node_keys(node).is_empty()` |
| `ResolvedData.child_count` | `store.node_children(node).len()` |
| `Key::parent()` | `store.node_parent(node)` |
| `Key::push()` | navigate to a child node by segment ID |
| `Key::split_last()` | `store.node_segment(node)` + `store.node_parent(node)` |
| `Key::common_prefix()` | trie walk to find the lowest common ancestor node |
| `Key::starts_with()` | prefix check via `store.key_segments()` |
| `Key::to_qualified_string()` | `store.key_qualified_str(key_id)` |
| `RowIdentity.prefix: Key` | `bundle_id: BundleId` + `node_id: NodeId` in `ViewRow` |
| `anchor_key() -> Key` in state.rs | returns a `NodeId` (or `KeyId`) from current `ViewRow` |

**Current usage sites to migrate:**
- `view_model.rs`: `RowIdentity.full_key: Option<Key>` → `Option<KeyId>`;
  `RowIdentity.prefix: Key` → `(BundleId, NodeId)`
- `state.rs`: `anchor_key() -> Key` → return the `NodeId` from the current
  `ViewRow`; sibling/parent navigation uses store node traversal instead of
  `Key::parent()` / string prefix matching

Do not add new uses of `Key`, `KeySegment`, or `KeyPartition` anywhere.

---

## Anti-patterns

Do not introduce new instances of any of these.

**Passing a key string into ops**
```rust
// WRONG
ops::delete::delete_key(state, "messages:app.title");

// RIGHT
let key_id = state.view_rows[state.cursor_row].key_id?;
ops::delete::delete_key(&mut state.domain_model, key_id);
```

**Calling `workspace::split_key()` outside `domain.rs`**
```rust
// WRONG (in ops or update)
let (bundle, real_key) = workspace::split_key(full_key);

// RIGHT — splitting is internal to domain.rs's two entry points
```

**Constructing or casting raw integers into IDs**
```rust
// WRONG — fabricating an ID from a raw integer
let key_id = KeyId(42);

// RIGHT — IDs are only obtained from the domain model API
let key_id = view_rows[cursor_row].key_id?;         // from ViewRow
let key_id = dm.insert_key(bundle_id, real_key).id; // from a creation call
```

**Manually queuing write changes in ops**
```rust
// WRONG — ops assembling PendingChange entries
state.domain_model.delete_key(key_id);
state.pending_writes.push(PendingChange::Delete { path, first_line, last_line, full_key });

// RIGHT — ops mutate the domain model; workspace derives writes at save time
dm.delete_key(handle);  // store records this as KeyMutation::Deleted
// that's it — workspace.save() will figure out the file writes
```

**Direct workspace mutation to track in-memory changes**
```rust
// WRONG
state.workspace.groups[gi].files[fi].entries.push(FileEntry::KeyValue { ... });
state.domain_model.insert_key(...);

// RIGHT — domain model is the only write target; workspace is only written at save
dm.insert_key(bundle_handle, "app.title");
```

**Ops calling other ops**
```rust
// WRONG (in rename.rs)
ops::delete::delete_key_inner(state, ...);
ops::insert::apply_cell_value(state, ...);

// RIGHT — extract shared logic to domain.rs or ops/common.rs
```

**Extracting a string from ViewRow and passing it to ops**
```rust
// WRONG — string field used as an ops argument
let key_str = state.view_rows[state.cursor_row].full_key_str.clone();
ops::delete::delete_key(&mut state.domain_model, &key_str);

// RIGHT — read the ID field and pass it
let key_id = state.view_rows[state.cursor_row].key_id?;
ops::delete::delete_key(&mut state.domain_model, key_id);
```

---

## Refactoring target for ops

Target signatures after full refactoring. No strings, no raw IDs, no workspace
access, no cross-op calls:

```rust
// delete.rs
pub fn delete_key(dm: &mut DomainModel, key: KeyId) { ... }
pub fn delete_locale_entry(dm: &mut DomainModel, key: KeyId, locale: GlobalLocaleId) { ... }

// insert.rs
pub fn insert_key(dm: &mut DomainModel, bundle: BundleId, real_key: &str, locale: GlobalLocaleId, value: &str) { ... }
pub fn set_translation(dm: &mut DomainModel, key: KeyId, locale: GlobalLocaleId, value: &str) { ... }

// rename.rs
pub fn rename_key(dm: &mut DomainModel, key: KeyId, new_real_key: &str) { ... }
pub fn copy_key(dm: &mut DomainModel, key: KeyId, new_real_key: &str) { ... }
```

Ops accept IDs because `KeyHandle<'_>` borrows `DomainModel` and cannot
coexist with the `&mut DomainModel` parameter in the same scope. Inside an op,
create a short-lived handle for reading (`let h = dm.key_handle(key); ... drop(h);`),
then call the mutation. `new_real_key` and `value` are the only string
parameters — they come from user input and are parsed inside `domain.rs`.

---

## Checklist for new code

Before adding a new message handler or op:

- [ ] Does update.rs extract a typed handle before calling ops?
- [ ] Does the op accept a handle, not a string and not a raw ID?
- [ ] Does the op call only `DomainModel` methods?
- [ ] Is there any `pending_writes.push(...)` in the new code? (It should not be.)
- [ ] Does the op avoid accessing `workspace.groups` directly?
- [ ] Does the op avoid calling other ops?
- [ ] Is `workspace::split_key()` absent from ops and update?
- [ ] If view_rows are read in update.rs, is it to get an *ID* field (not a string)?
- [ ] Is the view layer (`view_rows`, `ViewRow`) absent from ops?
