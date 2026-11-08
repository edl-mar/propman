> ⚠️ **Teilweise veraltet (Stand 2026-11-07).** Die Konzepte gelten weiter, die Code-Skizzen nicht mehr unverändert.
> - **Gültig:** StringPool, History-Idee (lokaler Stack pro Zeile), Edit-Gruppierung, Trie-Edits, Speichern = Neubau, View/Overlay, Diff über Filter.
> - **Überholt:** `TypedId` mit `from_idx` (jetzt `columns::Idx` ohne `from_idx`), `define_id!` mit `u32` (Blueprint nutzt `usize` und privaten ID-Konstruktor), `unwrap()` in `current()`/`restore()` (Zugriff soll `Option` liefern), `EditBuilder` mit `&mut Store` (jetzt: Writer-Familie).
> - **Entschieden (2026-11-07):** Tabellen sind SoA, eine `Dense`-Spalte pro Feld. `main_v16.rs` mit Zeilen-Structs war nur ein Experiment. `HistoryTable` wird zur `HistoryColumn<Id, T>` (Spalte mit Historie).
> - Maßgeblich für Struktur und Regeln: `propman-v2-workspace-CLAUDE.md`, `blueprint_v3.rs`.

# propman-v2 — Architektur

Store-Kern und View-Architektur für den Neustart. Ergänzt (nicht ersetzt) `propman-architecture-notes.md`, die den Analyse-Stand der alten v1-Codebase dokumentiert.

---

## 1. Typisierte IDs per Makro

Viele strukturell identische Newtype-IDs (10–15 Tabellen erwartet) — ein `macro_rules!` statt Boilerplate pro Typ:

```rust
macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u32);
        impl $name {
            fn idx(self) -> usize { self.0 as usize }
        }
    };
}

define_id!(EntryId);
define_id!(KeyId);
define_id!(NodeId);
```

Bewusst **kein** `proc_macro`, kein `index_vec`-Crate — für ein Lern-Rewrite lohnt sich `macro_rules!` selbst zu schreiben (Makro-Grundlagen sind sonst nirgends im Projekt vorgekommen). Nachteil: IDE-"Go to Definition" springt in den Makro-Aufruf statt zu einer lesbaren `struct`.

**Zeitpunkt:** Erst bauen, wenn die dritte/vierte `define_id!`-artige Wiederholung tatsächlich nervt — nicht vorab.

---

## 2. StringPool

Ersetzt die v1-Idee "Kommentare/Werte direkt als `String`-Felder" durch eine deduplizierte Intern-Tabelle, analog zu v1s Segment-Interning, aber bewusst **anderer Name** (nicht `GlobalSegment` — das war strukturell was anderes: kurze Trie-Bausteine, keine freien Übersetzungstexte):

```rust
pub struct ValueId(u32);

value_strings: Vec<String>,              // append-only Content
value_index:   HashMap<String, ValueId>, // Dedup-Lookup
```

Vorteil: History-Einträge werden zu `u32` statt String-Klonen, Dirty-Checks werden zu Integer-Vergleichen, Cross-Entry-Copy kopiert nur eine ID statt Text.

---

## 3. HistoryTable — non-destruktive Speicherung

Zentrales generisches Muster, ersetzt v1s `entry_current`/`entry_original`-Zellen durch einen **lokalen, append-only Stack pro Zeile**:

```rust
pub trait TypedId: Copy {
    fn idx(self) -> usize;
    fn from_idx(i: usize) -> Self;
}

pub struct HistoryTable<Id: TypedId, T: Clone> {
    rows: Vec<Vec<T>>,   // ein Stack pro Zeile/Entry
}

impl<Id: TypedId, T: Clone> HistoryTable<Id, T> {
    pub fn new_row(&mut self, initial: T) -> Id { /* pusht neue Zeile, gibt neue Id */ }
    pub fn current(&self, id: Id) -> &T { self.rows[id.idx()].last().unwrap() }  // O(1), kein Cache-Feld nötig
    pub fn at_version(&self, id: Id, version: usize) -> Option<&T> { self.rows[id.idx()].get(version) }
    pub fn version_count(&self, id: Id) -> usize { self.rows[id.idx()].len() }
    fn push(&mut self, id: Id, value: T) { self.rows[id.idx()].push(value); }
}
```

**Warum lokal statt global:** eine frühere Variante hatte einen einzigen, app-weiten `Generation`-Zähler (globales Write-Log). Verworfen zugunsten lokaler Stacks pro Zeile — echte Entkopplung, kein geteilter Zähler, den jede Tabelle kennen müsste. `WriteId`/`Generation` als eigenes Feld entfällt komplett; `current()` ist immer `.last()`.

**Dirty-Check wird zur Ableitung:**
```rust
fn is_dirty(&self, id: EntryId) -> bool { self.entry_value.version_count(id) > 1 }
```
Kein `entry_original`-Feld mehr nötig — Version 0 **ist** das Original.

**Undo/Redo als ein einziges Konzept — "Pick from History":** kein Cursor/Positions-Zeiger. Restore ist ein ganz normaler Edit, der einen alten Wert erneut ans Ende pusht:
```rust
fn restore(&mut self, id: EntryId, from_version: usize) {
    let old = self.entry_value.at_version(id, from_version).unwrap().clone();
    self.entry_value.push(id, old);   // normaler Write
}
```
Löst das Linear-vs-Baum-Problem klassischer Undo/Redo-Cursor: nichts wird je gelöscht, verworfene "Redo"-Pfade bleiben einfach unverändert im Vec liegen, nur nicht mehr über einen Zeiger erreichbar — aber über ihre Position weiterhin adressierbar (z. B. für Copy-from-history).

---

## 4. Edit / Write / TouchedSlot — Gruppierung mehrerer Writes

Ein **Edit** = eine Nutzer-wahrgenommene Aktion (Rename, Paste, einzelne Zelländerung), kann mehrere **Writes** über verschiedene Tabellen hinweg bündeln:

```rust
pub enum TouchedSlot {
    Entry(EntryId, usize),   // usize = Version VOR dem Write, fürs Zurückrollen
    Key(KeyId, usize),
    Node(NodeId, usize),
}

pub struct EditRecord { kind: &'static str, touched: Vec<TouchedSlot> }

pub struct EditBuilder<'s> { store: &'s mut Store, kind: &'static str, touched: Vec<TouchedSlot> }

impl<'s> EditBuilder<'s> {
    pub fn write_entry(&mut self, id: EntryId, value: ValueId) { /* pusht + trackt TouchedSlot */ }
    pub fn commit(self) -> EditId { /* schreibt EditRecord */ }
}
```

`TouchedSlot` ist ein kleines, geschlossenes Enum — lebt **nur** an dieser einen Sammelstelle. Die eigentlichen Store-Methoden bleiben stark typisiert (`set_translation(EntryId, ...)`), kein type-erasure im Rest des Codes.

**Batch-Undo wird mechanisch:**
```rust
fn undo_edit(&mut self, edit: EditId) {
    for slot in &self.edits[edit.idx()].touched {
        // Wert VOR diesem Write erneut pushen — pro betroffener Zeile derselbe restore()-Mechanismus
    }
}
```

`EditBuilder` hält `&mut Store` exklusiv, solange er lebt — "ein Edit läuft gerade" wird vom Borrow-Checker erzwungen, nicht zur Laufzeit geprüft.

**Namensklarstellung:** ursprünglich "global edit"/"lokaler edit" genannt — verworfen zugunsten `Edit` (Gruppierung) / `Write` (atomarer Push), weil "global" fälschlich "betrifft den ganzen Store" suggerierte.

---

## 5. Non-destruktive Trie-Edits

Sorge: lassen sich Umbenennungen im Trie (gemeinsame Präfixe = gemeinsame Identität) non-destruktiv abbilden?

**Antwort: ja, größtenteils schon durch bestehende Struktur.**

- `find_or_create_child` ist bereits rein additiv (neue Segmente → neue `NodeId`s, angehängt; gemeinsame Präfixe → Wiederverwendung über `node_children`-Lookup). Kein Umbau nötig.
- `key_node` wird zu `HistoryTable<KeyId, NodeId>`. Ein Rename = neuen Pfad auflösen/anlegen (additiv) + `key_node.push(key, new_node)` — ein ganz normaler Write, kein Sonderfall. Der alte Pfad bleibt komplett unangetastet und begehbar.
- `node_keys` (Rückwärts-Index "welche Keys enden hier") bleibt ein **gepflegter Cache**, darf ganz normal destruktiv mutiert werden — er ist nicht die Wahrheit, nur eine Ableitung von `key_node`.
- **Einziger echt destruktiver Punkt in v1:** Pruning leerer Trie-Äste nach einem Delete (`node_children[parent].remove(&seg)`). Lösung: **während der Session gar nicht pruning** — tote Äste bleiben stehen, harmlos. Der volle Store-Neubau nach jedem Save (siehe unten) räumt automatisch auf.

---

## 6. Speichern = Neubau, nicht Patchen

Ersetzt v1s inkrementelles Zeilen-Patchen (Quelle der Retry-Duplikations-Bugs) durch: **Save schreibt atomar, danach wird der komplette Store aus Disk neu aufgebaut.**

**Vorteil:** kein Zwischenzustand ("manche Changes gespeichert, manche nicht") kann je entstehen — nur "alter Store" oder "frisch aufgebauter Store", nie ein hybrider, potenziell inkonsistenter Zustand.

**Konsequenz — IDs überleben Save nicht:** `KeyId`/`EntryId` werden beim Neubau neu vergeben. Alles, was einen Save überstehen soll (Cursor, Selection, offene Konflikte), muss über **domänenstabile Identität** referenzieren (z. B. `(Bundle-Name, Key-String)`), nicht über Store-interne IDs — nach dem Neubau wird gegen den frischen Store neu aufgelöst (`store.find_key("app.title")`).

**Konsequenz für Selection/Navigation:** gehört bewusst **nicht** in den Store, sondern in `AppState` (`pending_selection: Option<Selection>`) — sonst verwässert man die Grenze "Store = Datei-Domäne" und jeder Store-Test bräuchte unnötig UI-Session-Kontext.

---

## 7. View/Overlay-Architektur (unverändert aus erster Diskussionsrunde)

- **Zwei orthogonale Achsen**, kein gemeinsames `Mode`-Enum: `view: ViewState` (exklusiv: `Raw`/`Fragment`/`Tree`), `overlay: OverlayState` (kann über jeder View liegen: Filter, Paste, Confirm).
- **Enum-Dispatch statt `dyn Trait`** — Views sind eine geschlossene, selbst kontrollierte Menge:
  ```rust
  trait ViewMode {
      fn supported_ops(&self) -> &'static [OpKind];
      fn handle_op(&mut self, op: OpKind, ctx: &mut OpContext) -> Transition;
      fn render(&self, area: Rect, buf: &mut Buffer, ctx: &RenderContext);
  }
  enum ViewState { Raw(RawState), Fragment(FragmentState), Tree(TreeState) }
  ```
- **Zwei Kategorien von Operationen:** view-spezifische Bedeutung (jede View implementiert selbst, z. B. "Insert") vs. gleiche Bedeutung/anderer Datenzugriff (schmaler Capability-Trait wie `Selectable`, Logik existiert nur einmal).
- **Transition mit generischem Payload statt Typ pro View-Paar:**
  ```rust
  enum Transition { Stay, SwitchTo(ViewKind) }
  ```
  Payload (z. B. Selektion) liegt in `AppState::pending_selection`, nicht im `Transition`-Enum selbst — vermeidet multiplikatives Wachstum, wenn mehrere Quell-Views demselben Ziel unterschiedlich geformte Daten liefern könnten.

---

## 8. Diff-View kommt geschenkt

Eine eigene "Diff"-View-Variante ist wahrscheinlich unnötig: `TREE`/`FRAGMENT`-View plus ein aktiver Filter auf dem Edit-Log liefert dasselbe:

```rust
enum FilterExpr {
    // ... bestehende Terme
    TouchedByEdit(EditId),     // z. B. "was hat dieser eine Rename verändert"
    ChangedSinceSave,          // dirty seit dem letzten Checkpoint
}
```

Details zum Filter-Konzept selbst bewusst zurückgestellt (eigenes, großes Kapitel für später).

---

## Offene Punkte für später

- `find_or_create_path`/Segment-Auflösung im Detail für Rename/Copy-Fälle
- Unbeschränktes Wachstum von `entry_value`/`node_*`-Tabellen zwischen Saves — akzeptiert, weil Save+Neubau aufräumt
- Konfliktauflösung bei externen Dateiänderungen → siehe `propman-v2-konfliktaufloesung.md`
