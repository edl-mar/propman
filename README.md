# propman

A terminal UI for managing Java `.properties` files, written in Rust.

## What it does

propman scans a directory recursively for `.properties` files, groups them by **bundle** (the filename stem before the first `_`) and **locale** (everything after the first `_`; files with no `_` get locale `default`), and presents them as an interactive side-by-side table. You can navigate, filter, edit, create, rename, and delete translation entries across all bundles and locales without leaving the terminal.

## Installation

```
cargo build --release
```

The binary is at `target/release/propman`. Run it from (or point it at) the directory containing your `.properties` files:

```
propman [directory]
```

If no directory is given the current directory is used.

## Keybindings

| Key | Action |
|-----|--------|
| `↑↓←→` / `hjkl` | Navigate |
| `PgUp` / `PgDn` | Scroll fast |
| `Enter` on locale cell | Edit value |
| `Enter` on key column | Rename key |
| `n` | New key |
| `d` on locale cell | Delete that locale entry |
| `d` on key column | Delete key (with confirmation) |
| `Tab` (in rename/delete) | Toggle exact / +children scope |
| `Space` | Preview full value |
| `/` | Focus filter bar |
| `Ctrl+S` | Save to disk |
| `q` / `Ctrl+C` | Quit |

## Filter syntax

The filter bar (press `/`) accepts boolean expressions over typed terms.
Each term is prefixed by a sigil; terms can appear in any order.
Space = AND (higher precedence), comma = OR (lower precedence).

| Example | Meaning |
|---------|---------|
| `messages` | Keys in the `messages` bundle |
| `/error` | Keys containing `error` |
| `:de` | Show only the `de` locale column |
| `messages /error :de!` | `de` must be present, messages bundle |
| `/?` | Keys with at least one missing translation |
| `:de?, :si?` | Missing in de OR missing in si |
| `messages /confirm, errors /delete` | OR across bundles and key patterns |
| `#` | All dirty (unsaved) keys, narrow to dirty locale columns |
| `:?` | Per row: show only the missing locale columns |
| `:!` | Per row: show only the present locale columns |

Locale modifiers: `!` = must be present, `?` = must be missing.

See [docs/filtering.md](docs/filtering.md) for the full syntax reference.

## Stack

- [ratatui](https://github.com/ratatui-org/ratatui) + crossterm — TUI rendering
- [tui-textarea](https://github.com/rhysd/tui-textarea) — text input
- [walkdir](https://github.com/BurntSushi/walkdir) — directory scanning
- [anyhow](https://github.com/dtolnay/anyhow) — error handling
