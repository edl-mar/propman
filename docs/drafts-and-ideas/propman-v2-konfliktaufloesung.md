> ⚠️ **Teilweise veraltet (Stand 2026-11-07).** Modell und Klassifikationstabelle gelten weiter, die Code-Skizze nicht.
> - **Skizze fehlerhaft:** `at_version(id, 0)` liefert `Option<&T>` (Vergleich mit `&ours` und `*base` kompiliert nicht); `EditBuilder` hält `&mut Store` während `self.entry_value` gelesen wird (Borrow-Konflikt). Nicht kompiliert geprüft.
> - **Konzeptlücke:** `reload_from_disk` nimmt `(EntryId, ValueId)` entgegen, aber `disk` kennt keine IDs. Frische Disk-Inhalte müssen über Bundle + Key-String den IDs zugeordnet werden (Aufgabe von `app`); `ValueId`s sind nur im selben StringPool vergleichbar.
> - Maßgeblich für Struktur und Regeln: `propman-v2-workspace-CLAUDE.md`.

# propman-v2 — Konfliktauflösung bei externen Änderungen

Setzt das Edit/HistoryTable-Modell aus `propman-v2-architektur.md` voraus.

---

## Ausgangsproblem

User hat nicht-committete Änderungen in propman, während extern (z. B. via Git) eine neue Version der `.properties`-Dateien geladen wird. Sorge: ein Zustand, in dem weder Speichern (überschreibt externe Änderungen) noch Neuladen (verwirft eigene Änderungen) sicher möglich ist.

**Auflösung:** In diesem Modell kann dieser Zustand strukturell nicht entstehen — weil Neuladen nie destruktiv ist.

## Reload als ganz normaler Edit

```rust
fn reload_from_disk(&mut self, discovered: Vec<(EntryId, ValueId)>) -> (EditId, Vec<Conflict>) {
    let mut edit = self.begin_edit("reload");
    let mut conflicts = Vec::new();

    for (id, disk_value) in discovered {
        let ours = *self.entry_value.current(id);
        if disk_value == ours { continue; }              // Datei für diesen Entry unverändert — kein Rauschen

        let base = self.entry_value.at_version(id, 0);    // Stand beim letzten Laden/Neubau
        if base != &ours {
            conflicts.push(Conflict { entry: id, base: *base, ours, theirs: disk_value });
        }
        edit.write_entry(id, disk_value);                 // "theirs" wird trotzdem als neue Version sichtbar
    }
    (edit.commit(), conflicts)
}
```

Nichts wird überschrieben — die externe Version wird als **neuer Write** angehängt, die eigene bleibt als ältere Version im selben Entry-Stack vollständig erhalten. Beispiel:

```
Vorher:  [v0: "Save", v1: "Save Document" (eigener, ungespeicherter Edit)]
Reload:  extern → "Save File"
Nachher: [v0: "Save", v1: "Save Document" (unser), v2: "Save File" (extern)]
```

**"Basis" = Version 0, ohne eigenes Feld** — solange kein Save (= voller Neubau) dazwischen war. Nach einem Save wird Version 0 automatisch der neue, gerade gespeicherte Stand (korrekt: neue Basis nach dem Speichern).

## Konflikt-Klassifikation

Nur ein Fall verdient eine visuelle Warnung — die anderen lösen sich automatisch auf:

| Fall | Bedeutung | Aktion |
|---|---|---|
| `base == ours` | nur extern geändert | automatisch übernehmen, kein Konflikt |
| `base == theirs` | nur lokal geändert | eigene Version bleibt aktuell, kein Konflikt |
| `base != ours != theirs` | beide Seiten weichen ab | **echter Konflikt** — Markierung nötig |

## Wo das ans Modell stößt — nicht alles ist auf Wert-Ebene lösbar

Die obige Behandlung deckt nur **Wert-Änderungen** an bereits bekannten `EntryId`s. Zwei Kategorien bleiben schwieriger:

1. **Strukturelle Änderungen** (neuer Key extern hinzugefügt, umbenannt, verschoben) — hat noch keine `EntryId`/`KeyId`, reines Zuordnungsproblem (Text-Diff, keine ID-Frage). Braucht eine Vorstufe:

   ```rust
   enum ReloadOutcome {
       Clean(EditId),                        // alles automatisch auflösbar
       NeedsFileView { diff: LineDiff },      // mehrdeutig — User entscheidet im FILE-View
   }
   ```

   `LineDiff` ist reiner Zeilen-Diff (RAW_FILE alt vs. neu, z. B. via `similar`-Crate) — braucht keine Store-IDs. RAW_FILE bleibt deshalb auch nach FRAGMENT/TREE als Diff-Grundlage im System relevant, nicht nur als Lerngerüst am Anfang.

2. **Genuin unauflösbare Fälle** (nicht algorithmisch besser lösbar, echte Mehrdeutigkeit):
   - Beide Seiten benennen denselben Key um, aber zu unterschiedlichen Zielen.
   - Eine Seite spaltet einen Wert in mehrere Keys auf (keine 1:1-Zuordnung mehr).
   - Beide Seiten ändern denselben Wert an unterschiedlichen Text-Stellen (unterhalb der Entry-Granularität).

## ConflictResolution — User übersetzt Diff-Hunks in normale Edits

```rust
enum ConflictResolution {
    Rename { old_key: KeyId, new_path: String },
    NewKey { path: String, value: ValueId },
    Deleted { key: KeyId },
    KeepOurs { key: KeyId },
    KeepTheirs { key: KeyId },
    ManualText(String),   // Fluchtluke für nicht klassifizierbare Fälle
}
```

Jede Variante mündet in denselben `EditBuilder`/`write_*`-Mechanismus wie ein normaler Edit — kein Sonder-Codepfad. Dadurch automatisch undo-fähig wie jeder andere Edit.

`ManualText` von Anfang an gleichwertig mitplanen, nicht als nachträglichen Notnagel — deckt die "genuin unauflösbar"-Fälle von oben ab, ohne dass propman versuchen muss, die Nutzer-Absicht zu erraten.

**Was hier bewusst noch offen bleibt:** die eigentliche Interaktionsgestaltung (Hunks anzeigen, Vorschläge machen) ist echte, neue UI-Arbeit — die Datenseite darunter ist vorbereitet, aber ungebaut.

## Speicher-Sperre — granular, nicht global

Kein OS-Filelock (würde andere Programme wie Git blockieren). Stattdessen:

```rust
fn save_file(&mut self, bundle: BundleId, locale: GlobalLocaleId) -> Result<(), SaveBlocked> {
    let unresolved = self.entries_in(bundle, locale)
        .filter(|&e| self.has_unresolved_conflict(e))
        .collect::<Vec<_>>();
    if !unresolved.is_empty() {
        return Err(SaveBlocked { entries: unresolved });   // nur die betroffenen Zeilen, nicht die ganze Datei
    }
    // normaler atomarer Write
}
```

Sperre ist so granular wie der einzelne Entry — der Rest der Datei/des Projekts bleibt normal speicherbar.

**mtime-Check statt Filelock:**
```rust
fn check_before_save(&self) -> Result<(), StaleFile> {
    let current = fs::metadata(&self.path)?.modified()?;
    if current != self.loaded_mtime { return Err(StaleFile); }
    Ok(())
}
```
Löst bei Abweichung idealerweise automatisch den Reload-Merge aus, statt Save nur stur zu verweigern.

## Filter-Integration — Konflikte werden sichtbar, kein neues UI-Konzept

```rust
FilterExpr::TouchedByEdit(reload_edit_id)
```

Derselbe Filterausdruck wie für "was hat mein letzter Rename verändert" (siehe Architektur-Notizen), angewandt auf den Reload-Edit. Unterscheidung "seit Reload geändert" (informativ) vs. "echter Konflikt" (blockiert Save) fällt aus der Klassifikationstabelle oben direkt ab — kein separater Mechanismus nötig.
