> ⚠️ **Teilweise veraltet (Stand 2026-11-07).** Das Konzept (discover/read getrennt, zweistufiges Lesen) gilt weiter.
> - **Überholt:** Abschnitt "Projekt-Setup" (`cargo init`) — jetzt Cargo-Workspace mit sechs Crates, `disk` ist das Crate `propman-disk`.
> - **Offen:** Wo lebt der `Encoding`-Typ? `disk` und `store` dürfen sich nicht kennen; `store` soll das Encoding aber speichern.
> - Maßgeblich für Struktur und Regeln: `propman-v2-workspace-CLAUDE.md`.

# propman-v2 — Datei-I/O

Alles rund um `disk.rs` (stateless Adapter) und den Ladevorgang.

---

## Grundprinzip: I/O so schmal wie möglich, Rest pur

`disk.rs` bleibt **stateless** — freie Funktionen, kein eigener Struct-State (siehe Architektur-Notizen, "Speichern = Neubau"). Innerhalb dessen: unreine I/O-Schritte so klein wie möglich halten, alles andere als pure, testbare Funktionen.

## Discovery vs. Reading — zwei getrennte Funktionen

Nicht in einer Funktion vermischen — unterschiedliche Lebensdauer/Änderungsfrequenz:

```rust
pub struct DiscoverOptions { pub recursive: bool }

pub fn discover(root: &Path, opts: &DiscoverOptions) -> impl Iterator<Item = PathBuf> + '_ {
    let mut walker = WalkDir::new(root).follow_links(true);
    if !opts.recursive { walker = walker.max_depth(1); }
    walker.into_iter()
        .filter_map(|e| e.ok())          // bewusste Entscheidung für den Prototyp — siehe unten
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
}
```

`WalkDir` statt Handroll-Rekursion: eingebauter Schutz gegen Symlink-Zyklen. `impl Iterator<Item = PathBuf> + '_` statt eigenen Iterator-Typ von Hand zu implementieren.

**`filter_map(|e| e.ok())` schluckt Fehler lautlos** — für ein erstes Experiment ok, aber bewusste Entscheidung, kein Versehen. Für produktiveren Code:
```rust
for entry in WalkDir::new(path) {
    match entry {
        Ok(e) => { /* ... */ }
        Err(err) => eprintln!("Konnte Eintrag nicht lesen: {err}"),
    }
}
```

## Encoding-agnostisches Lesen — zweistufig

**Nicht** verwechseln mit der discover/read-Trennung oben — das ist eine zweite, unabhängige Aufteilung *innerhalb* des Lesens einer einzelnen Datei:

```rust
// Stufe 1 — I/O + Encoding-Erkennung (unrein, macht Syscalls)
pub fn decode_file(path: &Path) -> io::Result<(Encoding, String)> {
    let bytes = fs::read(path)?;
    match std::str::from_utf8(&bytes) {
        Ok(s) => Ok((Encoding::Utf8, s.to_string())),
        Err(_) => Ok((Encoding::Latin1, iso_8859_1_to_string(&bytes))),  // klassische .properties-Konvention
    }
}

// Stufe 2 — reine Transformation, kein I/O, kein Dateisystem
pub fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(String::from).collect()
}
```

**Warum getrennt:**
- `split_lines` wird dadurch pur und ohne Testdatei testbar (`assert_eq!(split_lines("a\nb"), vec!["a","b"])`).
- Encoding muss als Store-Feld erhalten bleiben (`raw_file_encoding: Vec<Encoding>`, SoA-Spalte parallel zu `raw_file_path`/`raw_file_lines`), sonst würde eine ISO-8859-1-Datei beim Speichern versehentlich als UTF-8 rausgeschrieben — Diff-Verfälschung.

**Für den aktuellen Milestone:** reicht `fs::read_to_string(path)?.lines()...` (nur UTF-8) völlig. Die Zweiteilung erst umsetzen, wenn tatsächlich eine nicht-UTF-8-Testdatei auftaucht — Naht ist aber bewusst schon vorgesehen, damit der Umbau lokal bleibt.

## CLI-Argument: Pfad optional entgegennehmen

Kein `clap` für "ein optionaler Pfad" — Standardbibliothek reicht:

```rust
let path: PathBuf = env::args()
    .nth(1)                                    // Index 0 = Programmpfad selbst, daher nth(1)
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("."));    // _else: Default nur bei Bedarf gebaut
```

`clap` erst einführen, sobald echte Flags/Optionen gebraucht werden (`--dry-run`, `--locale de,en`), nicht vorher.

`cargo run -- <arg>` — das `--` trennt Cargo-eigene Flags von den ans Programm weitergereichten Argumenten.

## Testdaten-Ordner

`examples/` ist bei Cargo reserviert (jede `.rs`-Datei dort wird automatisch als `cargo run --example`-Target erkannt) — kollidiert konzeptionell mit "Testdaten zum Scannen", auch wenn's technisch nicht bricht (nur `.rs`-Dateien werden eingesammelt). Umbenannt zu **`testdata/`**.

## Path/PathBuf — Kurzreferenz

Eigene, ausführlichere Notiz vorhanden (`path-and-pathbuf.md`). Kernpunkte:
- `&Path` als Funktionsparameter (nicht `&PathBuf`) — Deref-Coercion erlaubt trotzdem `&PathBuf`-Übergabe.
- `PathBuf` speichert intern `OsString`, nicht `String` — Pfade sind nicht garantiert UTF-8.
- `.display()` fürs Formatieren in `println!`/Fehlermeldungen (kein `Display`-Trait auf `Path` selbst, aus demselben Grund).

## Projekt-Setup

```bash
cargo init --name propman   # NICHT `cargo new` — legt keinen eigenen Unterordner an
```
