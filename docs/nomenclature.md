# Nomenclature

The words used in the code and in the documents. The code and the docs are English; the German
column is for talking with the maintainer. If two things need different words, they get
different words here first.

A term is only listed once it is used or agreed. Where a term is not final, the entry says so.

## Files and bundles

| Code | Deutsch | Meaning, and what it is not |
|---|---|---|
| bundle | Bundle | The files with the same folder and base name. Identity is (folder, base name). A bundle never spans folders. |
| base name | Basisname | The file name without locale suffix and extension. |
| locale | Locale | The suffix of a file name (`de`, `de_DE`); `None` for the default file. |
| default file | Standarddatei | The file without locale suffix. Not `_default`: that is an ordinary base name. |
| confidence | Konfidenz | How sure the assignment of a file to a bundle and locale is: certain, uncertain (later also manual). |
| scope | Scope | A group of bundles that one application loads together. A bundle can be in several scopes. Not a Java term. |
| collision | Kollision | The same key in at least two bundles of one scope. Not a "conflict". |
| excluded | ausgeschlossen | A bundle that the user takes out of the session. |
| project root | Projektstamm | The folder of the outermost `.propman.toml`. |
| layer | Schicht | A settings file: shared (`.propman.toml`) or local (`.propman.local.toml`). A layer is a source. |

## File content

| Code | Deutsch | Meaning, and what it is not |
|---|---|---|
| encoding | Kodierung | How the bytes of a file become text. The BOM belongs to the encoding `Utf8Bom`, not to the text. |
| line | Zeile | One physical line of a file, with its own line ending. In German, "Zeile" only ever means this. |
| line ending | Zeilenende | `Lf`, `Cr`, `CrLf`, or `None` for an unterminated last line. |
| fingerprint | Fingerabdruck | A stamp (length, modification time) plus a hash of the raw bytes, taken when a file is read. |
| detection | Erkennung | The encoding or bundle assignment that propman works out by itself. |
| hint | Vorgabe | A forced encoding that replaces the detection. |

## The data model

| Code | Deutsch | Meaning, and what it is not |
|---|---|---|
| table, column, row, id | Tabelle, Spalte, Datensatz, ID | A table has several columns that share one kind of id. In German, "Datensatz" is a table row, never a text line. |
| history | Historie | All versions of one value in one row of a column. Only ever appended to. |
| version | Version | One entry of a history: a value, the edit that created it and the source it carries. |
| initial value | Anfangswert | Version 0 of a history. |
| current value | aktueller Wert | The last version. This is also the value that counts; there is no separate "effective" value. |
| edit | Edit | One user action or one process that may bundle several writes. Not a text editing session. |
| edit kind | Art des Edits | Load, Reload, SettingsRead, User, Save, Restore. |
| source | Quelle | Where a version comes from: disk, shared layer, local layer, user. Ranked: disk < shared < local < user. |
| base | Basis | The latest version from a given source, else version 0. A query, nothing stored. |
| marker | Marke | A version that repeats the current value under the source it was saved to, so that the base of that source catches up. |
| dirty | ungespeichert | Current value differs from the base of a source. Always relative to one source. |
| reload | Neu laden | Bringing values from the disk. A process, not a state. |
| policy | Regel | What happens when both sides changed: `Precedence` (the higher ranked source wins) or `Conflict` (the user decides). Fixed per column. |
| conflict | Konflikt | Both sides changed in a column with the policy `Conflict`. A derived state, never stored. |
| restore, undo | wiederherstellen, zurücknehmen | Both only append. Restore repeats one old value; undo repeats what an edit had changed. |
| pool | Pool | Append-only storage with deduplication and stable ids for large values (bytes, text, paths). |
| unit | Einheit | A self-contained area of the store with its own tables and guarantees, for example the raw unit. A guarantee between units belongs to the store. |
| raw unit | Rohdatei-Einheit | The unit that represents what is on the disk, with its interpretation. Only ever filled from the disk; the user does not edit it. |
| view | Ansicht | One of the exclusive screens of the program; for now the bundle overview. An overlay (later) lies over any view. |
| overview item | Eintrag der Übersicht | One line of the bundle overview: a folder, a bundle or a file. Not an entry (a key) and not a line of a file. |
| item id | Id eines Eintrags | What identifies an item of the overview whatever its place in the list: the id of a file or a bundle, or the path of a folder. Not a key (of a property). |
| level | Ebene | What one row of a view is: a file, a bundle, later a line or an entry. Not a scope of bundle merging. |
| group | Gruppe | A header row that gathers rows by a field (folder, bundle; later a key prefix). |
| cursor | Cursor | The row the user is on. Not the selection, and not what was called "the selected row" earlier. |
| mark | markieren | To put the entry under the cursor into a fixed set by hand (`Space`, shown as `*`). One way to make a selection. |
| selection | Auswahl | The ids of entries of one kind that an action works on: the marked entries, else the entry under the cursor; later also a condition. A fixed set is written in brackets. |
| action | Aktion | What is done to a selection; its precondition is the kind of the selection. Not an effect: an action may need none. |
| command | Befehl | An action with a name and a label that can be typed in the command line and bound to a key. Not a message: a message is what `update` gets, a command stands for one. |
| candidate | Kandidat | A command that fits what is typed, with the places of the letters that were hit. |
| command line | Befehlszeile | The overlay that `:` opens, in which commands are typed with completion. |
| effect | Effekt | What `update` asks the outside world to do (run the editor, read a file again), as plain data. At most one per message. Not a message: the result of it comes back as one. |
| alert | Meldung | A box over the top of the screen that says what went wrong (for example that no editor is set) and has to be dismissed first. An overlay, not a view. Not a problem of loading, and not a toast (a short success message, later). |
| bundle assignment | Zuordnung | The value that says which bundle and locale a file belongs to, with the confidence. |
| generation | Generation | The number of a store, carried by every id. An id from another generation gives `None`. |

## Informal words

"Fakt" (a value that comes from the source disk) and "Entscheidung" (a value from the user or a
layer) are used in conversation only. They are not types, and there is no separate storage for
them: both are versions in the same history.

## Words to avoid

- **"Merge"** on its own. It can mean how an application combines its bundles, or a three-way
  merge. For what happens on a reload, say "reload" and "resolve".
- **"Conflict" for a collision**, and the other way round.
- **"Sync"**. Say "saved", "marker" and "base".
