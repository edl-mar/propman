# propman — Workspace-Setup (Git Worktrees)

Nachschlagewerk für den Drei-Worktree-Aufbau: `v1-exploration` (Referenz), `propman-v2` (aktive Neuentwicklung), `workspace-setup` (gemeinsame Claude-Code-Config).

---

## Ziel-Struktur

```
E:\repos\propman\              ← Main Worktree, Branch: workspace-setup
├── CLAUDE.md                   ← wird automatisch in beide Kind-Sessions geladen
├── propman-v2\                 ← Linked Worktree, Branch: master
└── v1-exploration\             ← Linked Worktree, Branch: v1-exploration (alter Stand, read-only)
```

## Schritt 1 — alten master umbenennen

```bash
cd propman
git branch -m master v1-exploration
```

Benennt nur den Zeiger um — Ordnerinhalt unverändert. Macht den Namen `master` frei für einen Neuanfang.

## Schritt 2 — leeren master-Worktree anlegen

Braucht Git ≥ 2.42 für `--orphan` bei `worktree add`:

```bash
git worktree add --orphan -b master ../propman-v2
```

Ohne `--orphan` (ältere Git-Version): `git worktree add -b master ../propman-v2 <erster-commit-hash>` — startet beim ersten propman-Commit statt komplett leer.

**Warum das geht, ohne den alten Stand zu verdrängen:** ein Branch kann nicht in zwei Worktrees gleichzeitig ausgecheckt sein. Weil `master` durch Schritt 1 frei wurde, kann der neue Worktree ihn beanspruchen, während `propman/` unbehelligt auf `v1-exploration` bleibt.

## Nested Worktrees — Main-Worktree bleibt "detached HEAD"

Nach Schritt 1+2 zeigt `git worktree list` den Main Worktree (`propman/`) als `(detached HEAD)` — **erwartet, kein Fehler.** Beide Branches, die man braucht (`master`, `v1-exploration`), sind in den Linked Worktrees ausgecheckt; für den Main Worktree bleibt nichts mehr übrig.

`git status` in `propman/` zeigt `propman-v2/` und `v1-exploration/` als untracked (kosmetisch, weil sie physisch als Unterordner liegen). Stummschalten:

```bash
echo propman-v2/ >> .git/info/exclude
echo v1-exploration/ >> .git/info/exclude
```

`propman/` selbst nie direkt bearbeiten — nur als administrativer Hub + gemeinsamer Config-Ordner nutzen.

## Schritt 3 — workspace-setup-Branch für gemeinsame Config

```bash
cd E:\repos\propman
git checkout --orphan workspace-setup
git rm -rf --cached .     # falls der alte Detached-HEAD-Stand noch getrackt war
git clean -fd             # räumt alte Dateien weg — betrifft NICHT propman-v2/, v1-exploration/
                            # (durch .git/info/exclude geschützt); alter Stand ist ohnehin
                            # sicher in v1-exploration's Historie erhalten

# CLAUDE.md anlegen, dann:
git add CLAUDE.md
git commit -m "Gemeinsames Claude-Code-Setup für den propman-Workspace"
git push --set-upstream origin workspace-setup
```

## Kernwissen: CLAUDE.md-Discovery ist dateisystem-, nicht git-basiert

Claude Code läuft beim Start vom Arbeitsverzeichnis den Ordnerbaum nach **oben** bis zur Wurzel und lädt jede gefundene `CLAUDE.md` — unkonfigurierbar, kennt keine Git-Worktree-Grenzen. Deshalb wird eine `CLAUDE.md` in `propman/` (physischer Elternordner) automatisch mitgeladen, wenn Claude Code in `propman-v2/` oder `v1-exploration/` gestartet wird — obwohl das git-technisch unabhängige Worktrees sind.

**Caveat:** gilt bestätigt nur für `CLAUDE.md`/`CLAUDE.local.md`. `settings.json` (Permissions, `additionalDirectories`, Hooks) wird **nicht** von Elternverzeichnissen geerbt — braucht explizite Pflege pro Worktree oder `--add-dir`.

## Wo Claude Code starten

- **Fokussiert an `propman-v2` arbeiten (Standard-Empfehlung):** `cd propman-v2 && claude`. Bei Bedarf `v1-exploration` einmalig dazuholen: `/add-dir ../v1-exploration` in der Session.
- **`v1-exploration` dauerhaft verfügbar:** in `propman-v2/.claude/settings.json`:
  ```json
  { "permissions": { "additionalDirectories": ["../v1-exploration"] } }
  ```
- **Beide gleichberechtigt im Blick:** Claude Code direkt in `propman/` starten — sieht beide Unterordner automatisch, kein `--add-dir` nötig.

## GitHub: Default-Branch-Ablauf beim Umbenennen

GitHub lässt den aktuellen Default-Branch nicht löschen. Reihenfolge:

1. `git push origin v1-exploration`
2. GitHub → Settings → Branches → Default Branch → temporär auf `v1-exploration`
3. `git push origin --delete master` (jetzt erlaubt)
4. `git push origin master` (neuer, leerer Branch)
5. GitHub → Settings → Branches → Default Branch → zurück auf `master`

## Regel für den Umgang mit v1-exploration

Nur lesend referenzieren, nie zurückmergen — sonst geht der Lerneffekt des Neustarts verloren.
