> ⚠️ **Teilweise überholt (Stand 2026-11-08).** Maßgeblich ist `../decisions-and-open-questions.md`.
> - **Überholt:** der Eintrag zur Einstellungsdatei ("ordner-lokal, keine Vererbung"). Jetzt: ein Projektstamm, die äußerste `.propman.toml` gewinnt, dazu eine lokale Schicht.
> - **Übernommen:** die offenen Punkte stehen dort als offen, nicht als entschieden. Die Form der Tabellen (SoA oder Zeilen) ist nicht entschieden.

# propman v2 — Entscheidungen und offene Punkte

Stand: 2026-11-07. Ablage im Repo: `docs/open-points.md`.
Regel: Was hier unter "Offen" steht, ist bewusst noch nicht entschieden. Was nicht hier steht und nicht im Code existiert, ist vergessen worden und gehört hier eingetragen.

## Entschieden

- **Crates:** `columns`, `store`, `disk`, `app`, `tui`, `propman`. Richtung `propman -> tui -> app -> store -> columns` und `app -> disk`. `tui` kennt nur `app`.
- **Tabellen sind SoA:** eine `Dense`-Spalte pro Feld. Der Besitzer sichert gleiche Länge und legt Zeilen nur über eine Methode an. `main_v16.rs` war nur ein Experiment.
- **Encoding:** Erkennung in dieser Reihenfolge: BOM, gültiges UTF-8, sonst 8-Bit (Windows-1252, wenn Bytes 0x80–0x9F vorkommen, sonst Latin-1; bei den fünf undefinierten cp1252-Bytes Latin-1). Latin-1 ist verlustfrei, deshalb ist eine Fehlerkennung für den Roundtrip unkritisch.
- **Speichern nicht darstellbarer Zeichen:** als `\uXXXX`, Zeichen über U+FFFF als Surrogatpaar. In Kommentarzeilen nicht escapen (Warnung oder Fehler).
- **`decode(bytes, hint: Option<Encoding>)`**, `encode` akzeptiert jedes Encoding. "Neu interpretieren" (Datei mit anderem Encoding neu lesen) und "Konvertieren" (Encoding für das Speichern ändern) sind getrennte Aktionen.
- **Bundle = (Ordner, Basisname).** Erkennung dreistufig: wohlgeformter Suffix, bekannt (Liste), Gruppenindiz. Konfidenz: sicher, unsicher, manuell bestätigt. Die Liste ist nur ein Indiz und darf nie allein entscheiden.
- **Erste Ansicht:** Bundle-Übersicht mit Konfidenz (Farbe plus Zeichen, nicht nur Farbe).
- **Einstellungsdatei:** ordner-lokal, keine Vererbung. Wird erst bei der ersten Abweichung angelegt. Einträge über Dateinamen, nur Abweichungen. Einträge zu fehlenden Dateien bleiben als veraltet erhalten, Aufräumen nur auf Wunsch. Verschieben und Aufteilen später als propman-Funktion, weil nur so die Identität einer Datei sicher ist.
- **IO-Meilenstein:** Discovery, Einlesen, Bundle-Erkennung, atomares Speichern, Prüfung auf externe Änderung vor dem Speichern, Neuladen einer Datei ohne Merge, `propman check`.
- **Reihenfolge danach:** Store (`HistoryColumn`, `StringPool`, `RawFileTable`, Fragment und Entry, Edit/Writer), dann Reload als Edit für Wert-Änderungen.

## Offen

| Punkt | Warum offen | Wann entscheiden |
|---|---|---|
| Filtersystem | Braucht Tree und Entries. Hinweis aus v1: eine zentrale `semantics()` statt Dopplungen. | Nach der Tree-Ansicht |
| Konfliktauflösung und strukturelle Änderungen | Braucht Entries. Konzept: `propman-v2-konfliktaufloesung.md` (Skizze fehlerhaft). | Nach den Entry-Tabellen, direkt als erster Test des Store-Modells |
| Ort des `Encoding`-Typs | `disk` und `store` dürfen sich nicht kennen. | Wenn `app` ihn braucht |
| Encoding für Nicht-ASCII in reinen ASCII-Dateien | Datei ist gleichzeitig gültiges UTF-8 und Latin-1. Mögliche Antworten: Encoding der Nachbardateien, Ordner-Vorgabe. | Beim Speichern-Meilenstein |
| Anzeige von `ü` als `ü` | Gehört in die Parse-Schicht, ist unabhängig vom Encoding. | Beim Zeilenmodell |
| Reihenfolge pro Locale oder pro Bundle | Produktfrage. | Beim Fragment-Meilenstein |
| `define_table!`-Makro | Erst nach der zweiten oder dritten handgeschriebenen Tabelle. | Nach `RawFile`, Fragment, Entry |
| Neu interpretieren bei ungespeicherten Änderungen | Einfache Variante: vorher fragen. Alternative: Originalbytes als unveränderliche Basistabelle. | Beim Reload-Meilenstein |
| Einstellungsdatei: Name und Format | Vorschlag `.propman.toml`, braucht `toml` und `serde` (Rückfrage an dich). | Wenn die Datei geschrieben wird |
| Quelle der Locale-Liste | Vorschlag: aus dem JDK erzeugen (`Locale.getISOLanguages()`/`getISOCountries()`), `XK` ergänzen, Datum und Quelle im Dateikopf. | Beim Bau der Bundle-Erkennung |
| Neue Abhängigkeiten | `tempfile` (atomares Schreiben), `similar` (Zeilen-Diff), `toml`/`serde`. Jeweils vorher fragen. | Beim jeweiligen Meilenstein |
| `HistoryColumn`-API | Zugriff über `Option` statt `unwrap()`. ID-Typ `u32` oder `usize` (Blueprint nutzt `usize`). | Beim Store-Einstieg |
| Hub-`CLAUDE.md` im Elternordner | Wird zusätzlich zur Projekt-`CLAUDE.md` geladen und kann widersprechen. | Beim Anlegen prüfen |
