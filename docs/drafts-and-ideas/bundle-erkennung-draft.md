# Bundle- und Locale-Erkennung — Zusammenfassung der Heuristik

Stand: 2026-11-07. Diskussionsstand, nicht verbindlich. Verbindlich ist, was im Code und in den Entscheidungen mit Claude Code steht.

## Begriffe

- **Bundle** = (Ordner, Basisname). Bundles reichen nie über Ordnergrenzen.
- **Datei** = `Basisname` + optionaler Locale-Suffix + `.properties`.
- Java-Konvention: `basis[_sprache[_skript][_LAND[_variante]]].properties`. Die Datei ohne Suffix ist die Standarddatei (Fallback).

## Schritt 1 — Suffix parsen (rein, nur aus dem Dateinamen)

Den Dateinamen ohne `.properties` am `_` zerlegen und **von rechts** prüfen: zuerst die letzten drei Teile, dann zwei, dann einen. Die erste wohlgeformte Kombination gewinnt, der Rest davor ist der Basisname.

Wohlgeformt heißt:

- **Sprache:** 2–3 Buchstaben, Konvention klein (`de`, `fil`).
- **Skript** (optional): 4 Buchstaben, Konvention `Hans`.
- **Land** (optional): 2 Buchstaben, Konvention groß (`DE`), oder 3 Ziffern.
- **Variante** (optional): übriger alphanumerischer Teil.

Abweichende Schreibweise (`messages_DE`) ist wohlgeformt, senkt aber die Konfidenz, weil Java sie so nicht findet. Ein Punkt im Namen (`messages.de.properties`) ist keine Java-Konvention: ganzer Name = Basisname, keine Locale.

## Schritt 2 — Bekannt (Indiz, keine Bedingung)

Sprach- und Ländercodes werden gegen eine **Liste** geprüft. Die Liste ändert sich über die Zeit (z. B. Kosovo `XK`, alte Java-Sprachcodes `iw`, `in`, `ji`) und darf deshalb nie allein über die Zuordnung entscheiden. Java selbst prüft keine Liste.

- Quelle: JDK (`Locale.getISOLanguages()`, `Locale.getISOCountries()`), dreistellige Sprachcodes aus ISO 639, `XK` von Hand ergänzt.
- Im Dateikopf stehen Datum und Quelle.

## Schritt 3 — Gruppenindiz (zur Laufzeit berechnet, nicht gespeichert)

Ein Suffix wird glaubwürdiger, wenn:

- im selben Ordner eine Datei mit demselben Basisnamen **ohne** Suffix existiert,
- zum selben Basisnamen **mehrere** Locales existieren,
- dieselbe Locale in **mehreren Bundles** des gescannten Bereichs vorkommt.

## Konfidenz

| Fall | Deutung | Konfidenz |
|---|---|---|
| Kein wohlgeformter Suffix | Standarddatei | sicher |
| Wohlgeformt, bekannt, mit Gruppenindiz | Locale | sicher |
| Wohlgeformt, bekannt, ohne Indiz (z. B. `single_file_de`) | Locale (wahrscheinlichste Deutung) | unsicher |
| Wohlgeformt, unbekannt, mit Gruppenindiz | Locale | unsicher |
| Wohlgeformt, unbekannt, ohne Indiz | Standarddatei | sicher |
| Abweichende Groß-/Kleinschreibung | wie oben, aber eine Stufe niedriger | |
| Vom Nutzer gesetzt oder bestätigt | wie gesetzt | manuell |

Mehrdeutige Fälle sind grundsätzlich nicht aus dem Namen allein entscheidbar. Sie bekommen die wahrscheinlichste Deutung und sind sichtbar als unsicher markiert. **Nie still raten, wenn Namen gebildet werden:** Eine neue Datei zu einem Bundle benennt propman nur automatisch, wenn die Zuordnung sicher oder manuell ist, sonst fragt es.

## Korrektur durch den Nutzer

- Ordner-lokale Einstellungsdatei, keine Vererbung. Pro Eintrag ein Dateiname und die Abweichung (Bundle, Locale).
- Gespeichert werden nur Abweichungen von der Erkennung.
- Die Datei wird erst bei der ersten Abweichung angelegt. Einträge zu fehlenden Dateien bleiben als veraltet erhalten.

## Testfälle (Beispiele für die Spezifikation)

| Datei | Bundle | Locale | Konfidenz |
|---|---|---|---|
| `some_translations.properties` | `some_translations` | Standard | sicher |
| `some_translations_de.properties` (Basisdatei vorhanden) | `some_translations` | `de` | sicher |
| `single_file_de.properties` | `single_file` | `de` | unsicher |
| `another_single_file.properties` | `another_single_file` | Standard | sicher |
| `messages_de_DE.properties` | `messages` | `de_DE` | sicher (mit Gruppenindiz) |
| `messages_zh_Hans_CN.properties` | `messages` | `zh_Hans_CN` | sicher (mit Gruppenindiz) |
| `a_b_de.properties` | `a_b` | `de` | unsicher (ohne Indiz) |
| `log_in.properties` (ohne Gruppe) | `log` | `in` | unsicher |
| `messages_DE.properties` | `messages` | `DE` | unsicher |
| `messages.de.properties` | `messages.de` | Standard | sicher |
| `messages_xx.properties` (ohne Gruppe) | `messages_xx` | Standard | sicher |

Zu prüfen sind außerdem: dasselbe Bundle in zwei Ordnern bleibt getrennt, und die Reihenfolge der Dateien im Ordner ändert das Ergebnis nicht.
