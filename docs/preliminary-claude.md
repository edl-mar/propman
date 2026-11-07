# propman v2

TUI zum Verwalten von Java-`.properties`-Übersetzungen (ratatui, Elm-Architektur).
Neuentwicklung. Der alte Code (`v1-exploration`) dient nur als Referenz und wird nicht kopiert.

Diese Datei enthält nur Regeln, die sich prüfen lassen. Begründungen stehen in `docs/`.

## Crates und Abhängigkeitsrichtung

```
propman -> tui -> app -> store -> columns
                    \-> disk
```

| Crate | Aufgabe | Darf nicht |
|---|---|---|
| `columns` | `Idx`, `Dense`. Generisch, ohne Domäne. | irgendetwas aus dem Projekt kennen |
| `store` | Tabellen, IDs, History, Edit, Writer-Familie. Einzige Wahrheit über die Daten. | IO, UI, `disk` |
| `disk` | Zustandsloser Adapter: Discovery, Decode, Zeilen, atomares Speichern. | Store-Typen kennen |
| `app` | Load/Save/Reload, Konflikte, `AppState`, Ops, `Transition`. Verbindet `store` und `disk`. | ratatui, crossterm |
| `tui` | Views, Overlays, Rendering, Eingabe. | **von `store` oder `disk` abhängen** |
| `propman` | `main`, CLI-Argumente, Start. | Logik enthalten |

Die Regeln dieser Tabelle prüft Cargo über die `Cargo.toml` der Crates.
Ändere die Abhängigkeiten eines Crates nur, wenn die Aufgabe es ausdrücklich verlangt.

- `tui` liest über Typen, die `app` per `pub use` re-exportiert, und schreibt nur über Ops aus `app`.
- Ein neues `pub use` in `app` für Store-Typen ist eine Änderung der Lese-Oberfläche der UI.
  Nenne sie im Commit ausdrücklich.

## Store: Data Unit Pattern

Jede Tabelle ist ein Data Unit nach `docs/blueprint_v3.rs`.

- Der ID-Konstruktor ist privat. Nur das Tabellenmodul erzeugt IDs (`Idx` hat kein `from_idx`).
- Der Besitzer liegt in `mod detail`. Reader greifen dort auf Felder zu, Writer sind `pub(super)`.
- Mutationen laufen nur über Writer. Jede Mutation geht durch den `EditBuilder` und wird dabei erfasst.
- Es gibt kein Löschen, nur ein `deleted`-Flag. Historie wird nie gekürzt.
- `HistoryTable` hat nur `push`. `current()` ist das letzte Element.
- Cross-Table-Invarianten gehören in `Store`, nicht in die Tabelle.
  Der Store bietet absichtlich keine Operation an, die eine Invariante brechen würde.
- IDs überleben keinen Store-Rebuild. Zustand, der ein Speichern überleben muss, speichert
  domänenstabile Identität (Bundle + Key-String), keine IDs.
- Zugriff über ID liefert `Option`. Kein Indexzugriff, der paniken kann.

Neue Tabellen werden zuerst von Hand geschrieben. Ein Makro (`define_table!`) kommt erst
nach der zweiten oder dritten fertigen Tabelle.

## Rust-Regeln

- Kein `unsafe` (Workspace-Lint `forbid`).
- Kein `unwrap()`/`expect()` in Bibliothekscode. In Tests sind sie erlaubt.
- Pfade als `&Path` / `PathBuf`, nie als `String`. Ausgabe mit `.display()`.
- Kein `todo!()` und kein `dbg!` in committetem Code.
- Neue Abhängigkeiten stehen in `[workspace.dependencies]` und werden mit `.workspace = true` übernommen.
  Frage vor jeder neuen externen Abhängigkeit.

## Kommentare

- Ein Kommentar erklärt das Warum, nie das Was. Im Zweifel kein Kommentar.
- Jedes Modul, das eine Grenze bildet, hat ein `//!` mit Verantwortung und Invarianten (2 bis 5 Zeilen).
- `///` nur an öffentlichen Methoden, wo Name und Signatur nicht reichen. Kein Füllkommentar.
- Kein Banner, kein Changelog-Kommentar, kein auskommentierter Code.
- Wenn du Code änderst, prüfe die Kommentare und Modul-Docs drumherum und passe sie an oder lösche sie.

## Arbeitsweise

- Eine Änderung pro Commit. Ein Commit macht eine Sache, damit der Diff reviewbar bleibt.
- Vor einem Refactoring existieren Tests, die das bisherige Verhalten festhalten.
  Fehlen sie, schreibe sie zuerst und committe sie getrennt.
- Das Roundtrip-Verhalten ist die Hauptgarantie: Eingabe aus `testdata/` laden und unverändert
  speichern ergibt dieselben Bytes. Der Golden-File-Test darf nie rot committet werden.
- Keine Änderungen außerhalb des Crates, das die Aufgabe betrifft, ohne das vorher zu sagen.
- Kein Umbau der Architektur im Vorbeigehen. Wenn dir eine Struktur falsch erscheint,
  melde es und ändere es nicht selbst.
- Neue Modulgrenzen, neue Crates und neue Abhängigkeiten zwischen Crates sind Entscheidungen.
  Sie werden zuerst besprochen und dann als kurze Notiz in `docs/decisions/` festgehalten.

## Vor jedem Commit

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Alle drei müssen grün sein.

## Bewusst noch offen

Nicht entwerfen und nicht vorwegnehmen: Filtersystem, Konflikt-UI, Reihenfolge pro Locale
oder pro Bundle, Tree- und Fragment-Ansicht. Der aktuelle Meilenstein ist `RAW_FILE`
(Discovery, Decode, Zeilen, Roundtrip).
