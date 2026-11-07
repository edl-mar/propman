// =============================================================================
// BLUEPRINT (v3) — Data Unit Pattern, konsolidiert
// =============================================================================
//
// Ersetzt blueprint_v2.rs. Kein lauffähiges Beispiel — ein Nachschlage-
// Skelett mit Platzhaltern (Thing/ThingId), gedacht zum Kopieren pro neuer
// Datenstruktur im Rewrite.
//
// NEU GEGENÜBER v2:
//   - Das Wort "Owner" für das Ding, das die Felder tatsächlich besitzt.
//   - `detail` statt `storage` als Name für das versteckende Untermodul.
//   - `Dense<Id, T>` / `Idx` (nur `idx()`, kein `from_idx()`).
//   - checked/unchecked IDs — und wann sich das überhaupt lohnt.
//   - Group-Writer-Muster (mehrere Zeilen, eine Entscheidung pro Zeile).
//   - Die zentrale Regel: "wer kennt die Bedeutung" gegen Filter-/DSL-Antipatterns.
// =============================================================================
 
// -----------------------------------------------------------------------
// GRUNDBEGRIFFE
// -----------------------------------------------------------------------
//
// OWNER      Der Typ, der die eigentlichen Felder besitzt (`ThingTable`,
//            `Store`). Lebt immer im `detail`-Untermodul.
// DETAIL     Das Untermodul, das den Owner + seine Reader versteckt.
//            (Vorher `storage` genannt — `detail` ist der etabliertere,
//            aus C++/Boost bekannte Name für "Implementierungsdetail,
//            nicht anfassen". Bei uns ist das keine Konvention wie dort,
//            sondern vom Compiler erzwungen.)
// READER     Lebt INNERHALB von `detail` — Nachkomme, bekommt direkten
//            Feldzugriff auf den Owner, ganz ohne Brücke. Lesen ist immer
//            sicher, deshalb keine Beschränkung nötig.
// WRITER     Lebt AUSSERHALB von `detail`, aber INNERHALB der Dataunit
//            selbst — kein direkter Feldzugriff, nur benannte Methoden.
//
// -----------------------------------------------------------------------
// IDs — privates Tupel-Feld, konstruierbar NUR innerhalb der eigenen
// Dataunit. Kein Code außerhalb kann eine ID "erfinden" — normale Rust-
// Sichtbarkeitsregeln, ganz ohne Zusatzaufwand.
// -----------------------------------------------------------------------
 
mod columns {
    /// NUR `idx()` (ID -> Zahl), NIEMALS `from_idx()` (Zahl -> ID). Hätte
    /// `Dense` selbst die Fähigkeit, aus einer beliebigen Zahl eine ID zu
    /// konstruieren, bräuchte das einen `pub`-Trait-Methoden-Zugriff — und
    /// JEDER mit `use columns::Idx;` im Scope könnte sich damit eine ID
    /// erfinden. Die eigentliche Konstruktion bleibt exklusiv beim Owner.
    pub trait Idx: Copy {
        fn idx(self) -> usize;
    }
 
    /// Macht die Beziehung "diese Zeilen gehören zu dieser ID-Art" im
    /// Typsystem sichtbar — anders als ein nacktes `Vec<T>`, das nichts
    /// darüber aussagt, wer damit indizieren darf.
    pub struct Dense<Id, T> {
        data: Vec<T>,
        _marker: std::marker::PhantomData<Id>,
    }
 
    impl<Id: Idx, T> Dense<Id, T> {
        pub fn new() -> Self {
            Self { data: Vec::new(), _marker: std::marker::PhantomData }
        }
        /// Liefert nur die rohe Position, KEINE Id — Konstruktion bleibt
        /// Sache des Aufrufers, der dafür im richtigen Modul sein muss.
        pub fn push(&mut self, value: T) -> usize {
            let idx = self.data.len();
            self.data.push(value);
            idx
        }
        pub fn get(&self, id: Id) -> &T { &self.data[id.idx()] }
        pub fn get_mut(&mut self, id: Id) -> &mut T { &mut self.data[id.idx()] }
        pub fn iter(&self) -> impl Iterator<Item = (usize, &T)> + '_ {
            self.data.iter().enumerate()
        }
    }
 
    // ── OPTIONAL: Generationale Variante, NUR falls echtes Entfernen
    // (Slot-Wiederverwendung) gebraucht wird. Für Append-only-Systeme
    // (nichts wird wirklich entfernt, nur `deleted`-Flag) ist Dense oben
    // ausreichend — IDs werden dort NIE ungültig, weil Slots nie
    // wiederverwendet werden.
    //
    // Sobald Slots wiederverwendet werden dürfen, wird eine alte ID
    // "stale" statt ungültig — sie zeigt weiterhin auf einen belegten
    // Slot, aber auf ein ANDERES Element. Kein Absturz, aber leise
    // falsche Daten — schlimmer als ein Panic. Eine Generation pro Slot
    // löst das:
    //
    //   struct Slot<T> { value: Option<T>, generation: u32 }
    //   struct CheckedId<Id> { idx: usize, generation: u32, _marker: PhantomData<Id> }
    //
    // WICHTIG: Das braucht NUR eine Generation pro Slot — KEINE
    // zusätzliche Indirektion (kein Sparse-Set). Eine Indirektion
    // (dense/sparse-Trennung) wird erst nötig, wenn zusätzlich der
    // Speicher LÜCKENLOS bleiben soll (Kompaktierung) — das ist ein
    // eigener, unabhängiger Wunsch, meist erst bei sehr großen
    // Datenmengen relevant. Nicht vorsorglich bauen.
}
 
// -----------------------------------------------------------------------
// EINE EINZELNE DATA UNIT (Blatt) — z. B. eine Tabelle.
// -----------------------------------------------------------------------
 
mod thing_table {
    use crate::columns::{Dense, Idx};
 
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ThingId(usize); // Feld PRIVAT, absichtlich kein `pub`
    impl Idx for ThingId {
        fn idx(self) -> usize { self.0 }
    }
 
    /// `detail`: versteckt den OWNER (`ThingTable`) vor allem außer sich
    /// selbst und seinen Nachkommen (den Readern).
    mod detail {
        use super::{Dense, ThingId};
 
        struct Thing { /* Felder der Zeile */ }
 
        /// Der OWNER — besitzt die eigentlichen Daten.
        pub struct ThingTable {
            rows: Dense<ThingId, Thing>,
        }
 
        impl ThingTable {
            // ── Die einzigen drei `pub`-Methoden ─────────────────────
            pub fn new() -> Self { Self { rows: Dense::new() } }
            pub fn read(&self) -> ThingTableReader<'_> {
                ThingTableReader { table: self }
            }
            pub fn write(&mut self) -> super::ThingTableWriter<'_> {
                super::ThingTableWriter { table: self }
            }
 
            // ── `pub(super)`: reicht bis `thing_table` — genau dort
            // leben `ThingTableWriter`/`ThingWriter`.
            #[allow(dead_code)]
            pub(super) fn create(&mut self /* , ... */) -> ThingId {
                todo!() // ThingId(idx) — NUR hier konstruierbar
            }
 
            // ── FEHLT HIER ABSICHTLICH, WENN NICHT SICHER: z. B. kein
            // `delete()`, wenn Löschen andere Tabellen verwaisen ließe.
            // Auf DIESER Ebene kennt der Owner nur sich selbst — er kann
            // gar nicht wissen, ob eine Operation für ihn ALLEIN sicher
            // ist. Genau das ist der Grund, eine Operation NICHT
            // anzubieten, statt sie zu verstecken: der Owner weiß es
            // schlicht nicht besser. Die Garantie (falls nötig) sitzt
            // eine Ebene höher, in der kombinierenden Dataunit.
        }
 
        /// READER lebt HIER, als Nachkomme — direkter Feldzugriff auf den
        /// Owner, keine Brücke nötig, weil Lesen nie riskant ist.
        pub struct ThingTableReader<'a> {
            table: &'a ThingTable,
        }
        impl<'a> ThingTableReader<'a> {
            pub fn row(&self, id: ThingId) -> ThingReader<'a> {
                ThingReader { table: self.table, id }
            }
            // weitere sichere, tabellenweite Reads: exists(id), find_by_x(...)
        }
 
        pub struct ThingReader<'a> {
            table: &'a ThingTable,
            id: ThingId,
        }
        impl ThingReader<'_> {
            // pub fn field(&self) -> &FieldType { ... }
        }
    }
    pub use detail::{ThingReader, ThingTable, ThingTableReader};
 
    /// WRITER lebt AUSSERHALB von `detail` — kein direkter Feldzugriff
    /// mehr, nur die explizit benannten `pub(super)`-Methoden des Owners.
    pub(super) struct ThingTableWriter<'a> {
        table: &'a mut ThingTable,
    }
    impl<'a> ThingTableWriter<'a> {
        /// `self` PER WERT (nicht `&mut self`) — `&mut ThingTable` ist
        /// NICHT Copy, muss also aus `self` HERAUSBEWEGT werden, um die
        /// volle Lifetime `'a` weiterzugeben statt sie auf einen kurzen
        /// Reborrow zu verkürzen.
        pub fn row(self, id: ThingId) -> ThingWriter<'a> {
            ThingWriter { table: self.table, id }
        }
        pub fn create(&mut self /* , ... */) -> ThingId {
            todo!() // &mut self reicht — Rückgabewert ist OWNED
        }
    }
 
    pub(super) struct ThingWriter<'a> {
        table: &'a mut ThingTable,
        id: ThingId,
    }
    impl ThingWriter<'_> {
        // pub fn set_field(&mut self, ...) { ... }
        // Bewusst KEIN `delete()`, wenn das unsicher wäre — es sei denn,
        // diese EINE Dataunit kann es tatsächlich allein garantieren
        // (siehe Checkliste unten).
    }
}
 
// -----------------------------------------------------------------------
// KOMBINIERENDE DATA UNIT — hält mehrere Tabellen. DIESELBE Regel gilt
// REKURSIV: nur new()/read()/write() sind `pub`. `user_table`/`post_table`
// (die Blatt-Dataunits) sind hier KINDER, nicht Geschwister — dadurch
// reicht `pub(super)` auf IHRE Writer automatisch bis genau hierher.
// -----------------------------------------------------------------------
 
mod app {
    mod storage_of_things {
        // --- hier lägen thing_table, other_table als Kinder -----------
        // use thing_table::{ThingId, ThingReader, ThingTable};
 
        // pub struct Store { things: ThingTable, others: OtherTable }
        // impl Store {
        //     pub fn new() -> Self { ... }
        //     pub fn read(&self) -> StoreReader<'_> { ... }
        //     pub(super) fn write(&mut self) -> super::StoreWriter<'_> { ... }
        //
        //     // Dünne Weiterleitungsmethoden — der einzige Ort, an dem
        //     // ThingWriter (Blatt-Writer) außerhalb seiner eigenen
        //     // Definition noch direkt angefasst wird. Das darf hier
        //     // passieren, weil `storage_of_things` jetzt der direkte
        //     // ELTERNTEIL von `thing_table` ist, nicht nur Geschwister.
        //     pub(super) fn set_thing_field(&mut self, id: ThingId, ...) {
        //         self.things.write().row(id).set_field(...);
        //     }
        // }
    }
    pub use storage_of_things::{Store, StoreReader, ThingId, ThingReader};
 
    // ── Ab hier: die Store-Writer-Familie. Liegt im GENAU GLEICHEN
    // Modul wie die äußerste Dataunit (z. B. `App`) — braucht deshalb
    // GAR KEIN Sichtbarkeits-Schlüsselwort. Plain private reicht, weil
    // "gleiches Modul" automatisch Zugriff gibt — UND weil `storage_of_
    // things` ein Nachkomme von `app` ist, kann der Owner (`Store`) diese
    // Typen trotzdem konstruieren, egal wie tief er selbst verschachtelt
    // ist. Private Sichtbarkeit reicht IMMER zu allen Nachkommen.
 
    // struct StoreWriter<'a> { store: &'a mut Store }
    // struct StoreThingWriter<'a> { store: &'a mut Store, id: ThingId }
    // impl StoreThingWriter<'_> {
    //     fn set_field(&mut self, ...) { self.store.set_thing_field(self.id, ...) }
    //     // Bewusst KEIN delete() — nicht nur Disziplin: `Store` bietet
    //     // dafür schlicht KEINE Methode an, also gibt es keinen Weg,
    //     // versehentlich doch dranzukommen.
    // }
}
 
// =============================================================================
// checked/unchecked IDs — WANN sich das lohnt
// =============================================================================
//
// Eine Dataunit kann für eine ID nur EINE Frage beantworten: "existiert
// dazu etwas in MEINEM eigenen Speicher?" Mehr Information hat sie
// prinzipiell nicht — nicht, weil man sich nicht genug Mühe gegeben hätte,
// sondern weil die Antwort schlicht nicht in den Daten steckt, auf die sie
// Zugriff hat. Insbesondere: OB eine ID aus GENAU DIESER Instanz stammt,
// kann eine Dataunit strukturell nicht wissen — zwei Instanzen könnten
// beide eine "ThingId(3)" besitzen, beide bestehen den exists()-Check.
//
// Für ein APPEND-ONLY-System (Slots werden nie wiederverwendet, Löschen
// ist nur ein Flag) gilt: eine einmal ausgegebene ID ist FÜR IMMER ein
// gültiger Index. Ein separater "Checked"-Typ bringt hier NICHTS — jede
// ID, die überhaupt als Wert existiert, MUSS legitim vom Owner stammen
// (privates Tupel-Feld) und zeigt deshalb garantiert auf eine echte Zeile.
//
//     pub struct ThingId(usize);          // "irgendeine ID"
//     pub struct CheckedThingId(usize);   // würde hier NICHTS zusätzlich garantieren
//
// checked/unchecked lohnt sich ERST zusammen mit einer der beiden
// folgenden Erweiterungen — nicht vorher:
//
//   1. GENERATIONALE IDs (echtes Entfernen/Slot-Wiederverwendung erlaubt)
//      -> "Checked" prüft dann Slot-Belegung UND Generation.
//   2. MEHRERE INSTANZEN DERSELBEN DATAUNIT GLEICHZEITIG im Umlauf
//      -> bräuchte einen Instanz-Stempel (z. B. eine EINMALIGE UUID PRO
//         STORE-INSTANZ, nicht pro ID!), gegen den beim Zugriff verglichen
//         wird. Kostet keine teure UUID-Indirektion beim Indizieren (die
//         ID selbst bleibt ein billiger usize-Index), nur einen
//         zusätzlichen Vergleich beim Check.
//
// Beides erst bauen, wenn der jeweilige Bedarf ECHT ist (siehe Rule of
// Three unten) — nicht vorsorglich.
 
// =============================================================================
// GROUP-WRITER — mehrere Zeilen, eine Entscheidung PRO Zeile
// =============================================================================
//
// Für "lösche/bearbeite alle Zeilen, die X erfüllen" braucht man einen
// Writer, der über eine bereits getroffene, dem Owner bekannte Auswahl
// (nicht ein generisches Aufrufer-Prädikat!) iteriert:
//
//     struct SelectedThingsWriter<'a> { store: &'a mut Store, ids: Vec<ThingId> }
//     impl SelectedThingsWriter<'_> {
//         fn for_each(self, mut f: impl FnMut(DeleteThingWriter<'_>)) {
//             for id in self.ids {
//                 f(DeleteThingWriter { store: self.store, target: id });
//                 //        ^^^^^^^^^^ funktioniert NUR, weil `f(...)` ein
//                 // Funktionsaufruf ist — durch die elidierte Lifetime in
//                 // `impl FnMut(DeleteThingWriter<'_>)` erzwingt der
//                 // Compiler hier automatisch einen frischen Reborrow pro
//                 // Aufruf. Bei einem Struct-Literal OHNE umschließenden
//                 // Funktionsaufruf (z. B. direkt `.cascade()` im selben
//                 // Ausdruck) passiert das NICHT automatisch — dann bräuchte
//                 // es `&mut *self.store` von Hand.
//             }
//         }
//     }
//
// WICHTIG — zwei GRUPPEN von Auswahl-Kriterien sind NICHT dasselbe:
//   - "verified == false", "email endet auf X" -> KEINE zusätzliche
//     Garantie, dieselbe Konsequenz (Orphan-Entscheidung nötig) ->
//     TEILEN sich denselben Group-Writer-Typ.
//   - "hat keine Posts" -> ECHTE ZUSÄTZLICHE GARANTIE (kein Orphan-Risiko
//     möglich) -> verdient einen EIGENEN, eigenständigen Writer-Typ mit
//     MEHR Fähigkeiten (z. B. Löschen ohne Rückfrage) — kein
//     "Sonderfall" des ersten Musters, sondern ein bewusst ANDERER Typ.
//
// Regel: gleiche Konsequenz -> gleicher Writer-Typ, egal wie die Auswahl
// zustande kam. Andere Garantie -> anderer, eigener Writer-Typ.
 
// =============================================================================
// ÖFFENTLICHE HÜLLEN — App-eigenes API-Design, unabhängig von Store-Typen
// =============================================================================
//
// Die äußerste Dataunit (App) darf ihre PUBLIC API frei gestalten, auch
// wenn intern etwas wie DeleteThingWriter (privat!) existiert:
//
//     pub struct RetireThingAction<'a> { inner: DeleteThingWriter<'a> }  // inner bleibt privat
//     impl RetireThingAction<'_> {
//         pub fn delete_orphaned(self) -> usize { self.inner.cascade() }
//         pub fn reassign_orphaned_to(self, new: ThingId) -> Result<usize, String> { ... }
//     }
//     // ermöglicht: app.retire_thing(id).reassign_orphaned_to(other)
 
 
// =============================================================================
// CHECKLISTE beim Anlegen einer neuen Dataunit:
//
// [ ] ID: Tupel-Feld PRIVAT — nichts von außen konstruierbar.
// [ ] Owner + Reader leben in `detail`. Writer liegen AUSSERHALB `detail`,
//     aber innerhalb der Dataunit selbst.
// [ ] impl-Block des Owners: nur new()/read()/write() sind `pub`.
// [ ] Kombinierende Dataunits (Store, App): Blatt-Dataunits (user_table,
//     post_table) sind KINDER, nicht Geschwister — dadurch reicht
//     `pub(super)` auf deren Writer automatisch bis zur richtigen Ebene,
//     ohne `pub(in path)` schreiben zu müssen.
// [ ] Gefährliche Operationen (Cross-Structure) → gar nicht erst auf dem
//     Blatt-Writer anbieten. Fehlt eine Operation ganz (kein delete()) →
//     oft Absicht: "kann DIESE Dataunit das allein garantieren?"
// [ ] Wiederverwendbare/Bulk-fähige Logik → PRIVATE Methode auf dem
//     Owner, NIE im Reader/Writer dupliziert.
// [ ] Neuer Auswahl-Bedarf (z. B. "unverifiziert seit X Tagen")? → NEUE,
//     BENANNTE Methode auf dem Owner/Reader. NIEMALS ein generisches
//     Filter-Prädikat vom Aufrufer entgegennehmen (siehe Regel oben).
// [ ] Zwei Auswahl-Kriterien mit GLEICHER Konsequenz → EIN Group-Writer-
//     Typ. Eine ECHTE zusätzliche Garantie → EIGENER Writer-Typ, keine
//     Variante des ersten.
// [ ] checked/unchecked IDs erst einführen, wenn generationale IDs ODER
//     mehrere Instanzen gleichzeitig ein ECHTES Bedürfnis sind — nicht
//     vorsorglich.
// [ ] Bevor ein Trait/eine generische Abstraktion gebaut wird: "wer kennt
//     hier die Bedeutung?" Wenn die Antwort "die Dataunit selbst" ist,
//     eine benannte Methode bauen, kein generisches Prädikat/Trait.
// =============================================================================
 
fn main() {
    println!("Skelett zum Nachschlagen — kein lauffähiges Programm.");
}
