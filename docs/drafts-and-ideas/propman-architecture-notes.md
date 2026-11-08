> ⚠️ **Historische v1-Analyse, teilweise veraltet (Stand 2026-11-07).** Nicht als Vorgabe für v2 verwenden.
> - **Weiter gültig:** Abschnitt 1 (Save-Bugs in v1), Abschnitt 2 (Fragment-Modell, "Kleben"-Regel), Idee des zustandslosen Adapters.
> - **Überholt:** Abschnitt 3 (Signaturen wie `discover(dir) -> Store`, `rename_bundle(store, ..)` koppeln `disk` an den Store), Abschnitt 4 (`Tree`/`File`/`Diff` und `DomainModel` — v2 nutzt `Raw`/`Fragment`/`Tree` und `Store`; `Change::Reorder` ist v1-spezifisch), Layer-Tabelle, alle Statuszeilen.
> - **Nur v1:** Abschnitt 5 (`update.rs`, `filter.rs`, `store.rs`). Die Beobachtung zu `filter.rs` (eine zentrale `semantics()` statt Dopplungen) ist für das spätere Filtersystem nützlich.
> - Neuere Notizen: `propman-v2-architektur.md`, `propman-v2-workspace-CLAUDE.md`.

# propman — Architektur-Notizen

Zusammenfassung der Design-Diskussion. Enthält auch verworfene/geänderte Ideen, damit der Gedankengang nachvollziehbar bleibt.

---

## 1. Save-Pfad: Atomarität & Korrektheit

**Status:** Nächster großer Schritt (vor allem Weiteren laut eigener Priorisierung).

### Gefundene Probleme

- **`writer.rs` (`write_insert`/`write_change`/`write_delete`)**: Der Kommentar über `write_change` behauptet, `fs::File::create` sei sicher gegen Crash-Truncation. Stimmt nicht — `File::create` öffnet mit `O_TRUNC`, trunkiert also sofort auf 0 Bytes, bevor neu geschrieben wird. Ein Crash mittendrin zerstört die Datei.
- **Fix-Pattern:** Temp-Datei im *selben* Verzeichnis (wichtig für atomares `rename()` — nur innerhalb desselben Mounts atomar) + `sync_all()` + `persist()` (z. B. via `tempfile`-Crate).
- **`workspace::save()` — Retry-nach-Teilfehler-Bug:** `dm.clear_changes()` läuft nur bei komplettem Erfolg (`all_ok`). Bei Teilfehler:
  - Bereits erfolgreiche **Inserts** werden bei jedem Retry **erneut eingefügt** (Duplikate).
  - Bereits erfolgreiche **Deletes** schlagen bei jedem Retry **erneut fehl** (`find_entry_idx` findet den Eintrag nicht mehr) → `all_ok` wird nie wieder `true` → Save ist dauerhaft kaputt bis Neustart.
  - **Fix-Richtung:** Erfolg pro Change tracken, granulares `Store::clear_change(entry_id)` statt globalem `clear_changes()`.
- **`change_set()` iteriert über `HashSet`s** (`inserted_entries`, `modified_entries`, `deleted_entries`) → undefinierte, zwischen Programmläufen wechselnde Reihenfolge (SipHash-Seed). Wirkt sich aus auf:
  - Git-Diff-Rauschen (unnötige Reordering-Diffs).
  - Tatsächliche Insertion-Points bei mehreren neuen Geschwister-Keys im selben Save (`insertion_point_for` hängt von der Verarbeitungsreihenfolge ab).
  - **Fix:** Sortieren nach `EntryId` (kodiert Einfüge-Reihenfolge) oder `indexmap::IndexSet` statt `HashSet`.

### Größere Architektur-Konsequenz (siehe unten, Abschnitt 4)

Die zugrunde liegende Ursache dieser Bugs: zwei parallele Repräsentationen derselben Information (Store-Change-Log ↔ Workspace-Zeilenmodell), die synchron gehalten werden müssen. Die Lösung war am Ende nicht "besser synchronisieren", sondern **die zweite Repräsentation eliminieren** (→ Fragment-Modell im Store).

**Empfohlene Reihenfolge:**
1. `save()` von "N Patches pro Change" auf "1 Rebuild + atomarer Write pro betroffener Datei" umbauen.
2. Erst danach (oder parallel) das Fragment-Modell einführen — macht Punkt 1 einfacher, nicht schwerer.

---

## 2. Store-Erweiterung: Fragmente statt Zeilennummern

**Status:** Design steht, noch nicht implementiert. Mehrfach revidiert — siehe Verlauf unten.

### Verlauf der Idee (zur Nachvollziehbarkeit)

1. *Erster Ansatz:* Reihenfolge separat als `file_order: HashMap<(BundleId, GlobalLocaleId), Vec<KeyId>>` im Store, Kommentare bleiben im Workspace.
2. *Korrektur:* Kommentare haben Bedeutung (Übersetzer-Notizen, oft locale-spezifisch) → gehören eigentlich auch in den Store, nicht nur Zeilennummern.
3. *Verworfen:* Kommentar/Whitespace als zusätzliche Felder direkt auf dem `Entry` (`entry_leading_comment`, `entry_leads_section`) — zu grob, löst das "Header ohne folgenden Key"-Problem nicht sauber.
4. **Finaler Ansatz:** Einheitliche Block-Liste — jedes unterscheidbare Stück Datei-Inhalt (Kommentar, Leerzeile, Entry) ist eine eigene Adressierbare Einheit in derselben Sequenz. Das macht `file_order` überflüssig (die Sequenz selbst *ist* die Reihenfolge) und braucht keine Sonderfälle für Header/Trailer (die sind einfach Blocks vor dem ersten / nach dem letzten Entry-Block).

### Finale Struktur

```rust
pub struct FragmentId(u32);

pub enum FragmentContent {
    Comment(String),  // roher Text inkl. Sigil (# / !), mehrzeilig \n-gejoint
    Blank,
    Entry(EntryId),
}

// SoA, analog zum Rest des Stores
fragment_content: Vec<FragmentContent>,
file_fragments:   HashMap<(BundleId, GlobalLocaleId), Vec<FragmentId>>,
```

- **Name:** `Fragment` (nicht `Token` — Token impliziert Transienz/Parse-Zeit-Lebensdauer; hier braucht es eine stabile, langlebige, von Operationen referenzierbare Identität). Kandidaten `Block`/`Section`/`Paragraph`/`Chunk` verworfen (klingen nach Mehrzeilen-Gruppen oder kollidieren mit Store-Vokabular).

### "Kleben"-Regel fürs Verschieben (Move/Reorder)

Kommentar/Leerzeilen "kleben" nicht als gespeichertes Feld, sondern werden **aus der Fragment-Sequenz abgeleitet** (gleiche Heuristik wie bei der Reihenfolge selbst: nur speichern, was nicht herleitbar ist):

```rust
fn glued_span(&self, fragments: &[FragmentId], entry_idx: usize) -> RangeInclusive<usize> {
    let mut start = entry_idx;
    while start > 0 {
        match &self.fragment_content[fragments[start - 1].idx()] {
            FragmentContent::Entry(_) => break,   // vorheriger Entry — Grenze
            FragmentContent::Blank    => break,   // Leerzeile — Grenze (wichtig!)
            FragmentContent::Comment(_) => start -= 1,
        }
    }
    start..=entry_idx
}
```

Wichtig: **Leerzeile bricht die Kette**, nicht nur der vorherige Entry — sonst würde z. B. ein Lizenz-Header vor dem ersten Key beim Verschieben dieses Keys versehentlich mitwandern.

### Offene Fragen (bewusst nicht heute entschieden)

- Reihenfolge pro Locale oder bundle-weit kanonisch? (Produktfrage, nicht rein technisch.)
- Exaktes Bewahren des Kommentar-Rohformats (Sigil, Whitespace) — **Antwort: ja**, roh speichern und unverändert zurückschreiben, sonst unnötige Diffs auf unangetasteten Kommentarzeilen.

### Bewusster Kompromiss

Das verwässert die ursprüngliche Doc-Kommentar-Regel in `store.rs` ("only strings cross the store boundary at the workspace I/O layer") — gerechtfertigt, weil Kommentare echte, nicht herleitbare Nutzerdaten mit Bedeutung sind, nicht nur physische Zeilenbuchhaltung.

---

## 3. Workspace → Store, Adapter wird stateless

**Status:** Design steht, noch nicht implementiert.

### Verlauf

1. *Erster Impuls:* Workspace-Rolle schrumpfen auf reinen I/O-Adapter (Discovery, Parsen, atomares Schreiben), aber noch als Struct mit eigenem State.
2. **Finale Entscheidung:** Adapter komplett **stateless** — keine eigene Datenhaltung mehr (kein `PropertiesFile`/`FileGroup` mehr nötig), nur noch freie Funktionen, die den `Store` lesen/schreiben. Eliminiert die Synchronisierungs-Achse, die die Bugs aus Abschnitt 1 verursacht hat.
3. Konsequenz: Auch **Pfade** (`PathBuf`) wandern in den Store (nicht wie ursprünglich angenommen draußen bleiben) — sonst bräuchte der Adapter doch wieder eigenen State. Neue Heuristik dafür: *"würde das Speichern hier eine zweite Kopie erzeugen, die synchron gehalten werden muss?"* — wenn nein (weil nur noch eine Stelle existiert), gehört es in den Store, auch wenn es reine I/O-Buchhaltung ist und von keiner Domänenlogik gelesen wird.

### Finale Struktur

```rust
// store.rs — zusätzlich
bundle_dir:  Vec<PathBuf>,
locale_path: HashMap<(BundleId, GlobalLocaleId), PathBuf>,
```

```rust
// disk.rs (Umbenennung von workspace.rs)
pub fn discover(dir: &Path) -> anyhow::Result<Store>;
pub fn save_all(store: &mut Store) -> bool;
pub fn create_locale_file(store: &mut Store, bundle: BundleId, locale: &str) -> io::Result<()>;
pub fn rename_bundle(store: &mut Store, bundle: BundleId, new_name: &str) -> io::Result<()>;
```

### Namensfragen

- `Workspace` → `disk.rs`/`fs_io.rs` (Name noch nicht final, aber Konzept klar: reiner Adapter, kein Datenmodell mehr).
- `Store` → **bleibt `Store`**. `Database` erwogen und verworfen: falsche Konnotation (Persistenz, ACID, Schema-Migrationen, "überlebt Prozess-Neustart") — trifft alles nicht zu, `Store` ist rein transient und wird pro Session neu aufgebaut. `Store` passt zudem zum Elm/Redux-Vokabular, das die App ohnehin als Vorbild hat.

### Zu beachtender Fallstrick bei `rename_bundle`/`ChangePath`

`fs::rename` ist nur atomar, wenn Quelle und Ziel im selben Verzeichnis/Mount liegen. Umbenennen im selben Ordner: einfaches `rename` reicht. Verschieben über Mount-Grenzen: braucht Copy-to-temp + rename + delete-original (gleiches Muster wie der atomare Single-File-Write aus Abschnitt 1). Als zwei unterschiedliche Operationen behandeln.

---

## 4. UI-Architektur: View/Overlay-Trennung, Mode als Trait

**Status:** Konzept steht, noch nicht implementiert. Wird nach Save-Pfad + Store-Umbau angegangen.

### Kernkorrektur am ursprünglichen Brainstorming

„Mode" ist eigentlich **zwei orthogonale Achsen**, keine:

1. **View** (exklusiv, „welche Darstellung der Daten"): Tree, File, (später) Diff.
2. **Overlay** (kann über jeder View liegen, nicht exklusiv zur View): Filter, Paste-Panel, Delete-Bestätigung, Key-Naming, Rename.

Das heutige `Mode`-Enum vermischt beides → würde bei neuen Views kombinatorisch wachsen (View × Overlay). Trennung macht den Zustandsraum additiv statt multiplikativ.

```rust
pub struct AppState {
    pub view:    ViewState,      // Tree(..) | File(..) | Diff(..)
    pub overlay: OverlayState,   // None | Filter(..) | Paste(..) | Confirm(..)
    // ...
}
```

### Operationen: zwei Kategorien

- **Kategorie A — view-spezifische Bedeutung:** z. B. „Insert" heißt im Tree „neuer Key", im File-View „Zeile an Position X". Muss jede View selbst implementieren, keine gemeinsame Logik möglich.
- **Kategorie B — gleiche Bedeutung, view-spezifischer Datenzugriff:** z. B. Yank/Paste. Dafür ein schmaler Capability-Trait pro Operation:

```rust
trait Selectable {
    fn selected_value(&self, dm: &DomainModel) -> Option<String>;
    fn apply_value(&mut self, dm: &mut DomainModel, value: String);
}
// yank()/paste() existieren EINMAL, generisch über den Trait — nicht pro View dupliziert.
```

**Wichtig für später (Diff-View etc.):** Kein Versuch, ein universelles Adressmodell für alle Views zu bauen (Tree über `KeyId`, File über Zeilenindex, Diff über Wertepaare) — das würde in eine Abstraktion ausarten, die für keinen Fall richtig passt. Stattdessen pro Operation ein schmaler Trait, den jede View passend zu ihrem eigenen Datenmodell implementiert.

### View-Dispatch: Enum, kein `dyn`

```rust
pub enum OpKind { Navigate, StartEdit, Yank, Paste, Delete, Rename, NewKey, /* ... */ CommitReorder }

pub trait ViewMode {
    fn supported_ops(&self) -> &'static [OpKind];
    fn handle_op(&mut self, op: OpKind, ctx: &mut OpContext) -> Transition;
    fn render(&self, area: Rect, buf: &mut Buffer, ctx: &RenderContext);
}

pub enum ViewState { Tree(TreeState), File(FileViewState), Diff(DiffState) }
// match-Dispatch in ViewState — enum, nicht Box<dyn ViewMode>, weil die
// View-Menge geschlossen und klein ist. Exhaustiveness-Check vom Compiler
// bei jeder neuen View geschenkt; keine Box/dyn-Indirection nötig.
```

`supported_ops()` beantwortet direkt: „welche Operationen unterstützt ein Modus" — Router kann Tasten vorab gegen die aktuelle View validieren, UI kann anzeigen, was verfügbar ist.

### Modus-Übergänge mit Datenübergabe (Tree → File-View Beispiel)

Kein direktes Setzen von `state.mode` aus `tree::update` heraus (würde Kopplung zwischen Modulen reinschmuggeln). Stattdessen expliziter Rückgabewert:

```rust
enum Transition { Stay, SwitchTo(Mode) /* bzw. später: SwitchView / Overlay / Back */ }
```

`tree::update` baut beim Verlassen ein typisiertes Payload (z. B. `{ bundle, key_ids }`), der Router wendet die Transition zentral an. `file_view` muss die Interna von `tree` nicht kennen — nur das fertige Payload verarbeiten können.

### Warnung, bewusst festgehalten: `Change::Reorder`

Reorder (User verschiebt Keys im File-View) ist mit dem heutigen `Change`-Enum (`Insert`/`Update`/`Delete`, alle an `(KeyId, Locale)`-Wertänderungen geankert) **nicht darstellbar**. Braucht eine vierte Variante:

```rust
pub enum Change {
    Insert { entry_id: EntryId },
    Update { entry_id: EntryId },
    Delete { entry_id: EntryId },
    Reorder { bundle: BundleId, locale: GlobalLocaleId },  // Reihenfolge selbst kommt aus file_fragments
}
```

Verworfene Alternative: sofortiger Write ohne Pending-Change-Review (Option A aus der Diskussion) — verworfen, weil inkonsistent mit dem Rest der App (überall sonst: pending → review → `Ctrl+S`) und weil es das Nicht-Atomar-Problem aus Abschnitt 1 an einer zweiten Stelle reproduziert hätte.

**Mit dem Fragment-Modell (Abschnitt 2) wird das fast trivial:** die Ziel-Reihenfolge steht schon vollständig in `file_fragments` — kein Line-Shifting, keine `insertion_point_for`-Heuristik mehr nötig, weil `save()` ohnehin komplett neu aufbaut statt zu patchen.

---

## 5. Kleinere, unabhängige Refactoring-Notizen

Nicht dringend, aber notiert:

- **`update.rs` (~970 Zeilen, eine `match`-Funktion):** Aufteilen nach `Mode`, gespiegelt an der Struktur von `keybindings.rs` (`update_normal`, `update_editing`, …). Wird durch die ViewMode-Trait-Umstellung (Abschnitt 4) ohnehin weitgehend miterledigt.
- **`filter.rs` (~960 Zeilen, davon ~40 % Tests — gesünder als die Zeilenzahl suggeriert):**
  - `split_on_commas`/`split_on_whitespace` sind derselbe Tokenizer zweimal geschrieben (nur anderes Trennzeichen) → zu einer generischen Funktion zusammenziehen.
  - Die „ist das ein Column-only-Directive"-Klassifikation ist unabhängig an drei Stellen kodiert (`evaluate`, `column_directive`, `visible_locales_opt`) → ein zentraler `fn semantics(expr) -> Semantics { Row, ColumnOnly }`. Ein Test-Kommentar im Code deutet darauf hin, dass genau diese Dopplung schon mal zu einem Bug geführt hat.
  - Kleinigkeit: Kommentar bei `split_on_commas` („Parentheses are reserved for future use") ist veraltet — Klammer-Gruppierung ist bereits aktiv implementiert.
- **`store.rs` wird mit Fragments/Pfaden weiter wachsen** → guter Kandidat für Aufteilung in ein `store/`-Modul (`segments.rs`, `trie.rs`, `entries.rs`, `fragments.rs`, `paths.rs`), sobald die neuen Tabellen drin sind. Nicht vor der Implementierung nötig — aber der Schnitt zeichnet sich schon ab, sobald die Konzepte klar sind (Beobachtung aus dem Gespräch: gute Modul-Grenzen ergeben sich meist aus klaren Konzepten, nicht aus Datei-Länge).

---

## Layer-Übersicht (Spring-Analogie, mit Vorbehalt)

| Datei/Modul | Spring-Analogie | Hinweis |
|---|---|---|
| `store.rs` | Repository | Keine Interfaces nötig — eine Impl, kein DI |
| `domain.rs` | Service-Fassade | Auch hier: eine Impl, kein Interface |
| `ops/*` | Use-Case-Methoden (`@Service`) | Freie Funktionen statt Objektmethoden |
| `update()` + `ViewMode`/`Overlay` | Controller | **Hier lohnt sich der Trait** — echte Laufzeit-Varianz |
| `widgets`, `view_model` | View/Template | Rein lesend, kein State |

Grundprinzip („functional core, imperative shell"): Store/Domain/Ops sind reiner Daten-/Funktionskern ohne Polymorphie. Nur an der View/Update-Grenze gibt es echte, zur Laufzeit umschaltbare Varianz — nur dort lohnt sich ein Trait.
