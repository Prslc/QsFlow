<div align="center">

# QsFlow

</div>

## Overview

QsFlow is a Wayland-native application launcher and quick-search tool for Linux.
Type to search installed apps, Firefox bookmarks, web suggestions, GitHub, and inline
math — all from a single floating overlay.  Built with a Rust backend and QML frontend
powered by [Quickshell](https://github.com/outfoxxed/quickshell).

## Features

- **App Launcher** — fuzzy-search `.desktop` entries across XDG data directories.
- **File & Path Search** — walk `~/Desktop`, `~/Documents`, `~/Downloads` and home;
  open results in the default application. Files get type-specific icons by extension.
- **Clipboard History** — search and paste from `cliphist` history via `c` prefix.
- **Firefox Bookmarks & History** — read `places.sqlite` directly; no browser extension needed.
- **Web Suggestions** — live Google search autocomplete via `s` prefix.
- **GitHub Search** — quick link with `g` prefix.
- **Inline Calculator** — evaluate math expressions automatically; no prefix required.
- **Usage History** — frequently-launched items surface on empty input, ranked by count;
  press `Delete` to remove an entry.
- **GTK Theme Integration** — reads accent colors from your GTK4 theme CSS, falling
  back to built-in defaults.
- **Plugin System** — TOML-driven plugin registry; enable, disable, reorder, or remap
  keywords without touching code. New plugins are picked up automatically on startup.
- **Icon Resolution** — resolves application icons from Papirus, Breeze, Adwaita, and
  hicolor themes, plus Flatpak exports and legacy pixmaps. Results are cached per session.

## Requirements

- **Wayland** compositor with `wlr-layer-shell` support
- **[Quickshell](https://github.com/outfoxxed/quickshell)**
- Rust toolchain (to build the core backend)
- Firefox (optional, for bookmarks / history)
- [cliphist](https://github.com/sentriz/cliphist) (optional, for clipboard history)

## Installation

```bash
# clone and build
git clone https://github.com/Prslc/QsFlow.git
cd QsFlow/core
cargo build --release

# link the binary into PATH
ln -s "$(pwd)/target/release/qsflow-core" ~/.local/bin/qsflow-core
```

Then bind a hotkey (e.g. Alt+Space) to launch the shell:

```bash
quickshell -p /path/to/QsFlow/ui/MainShell.qml
```

## Usage

| Input | Action |
|-------|--------|
| `firefox` | fuzzy-search installed applications |
| `b <query>` | search Firefox bookmarks |
| `h <query>` | search Firefox history |
| `f <query>` | search files by name |
| `d <query>` | search files by path (multi-token fuzzy) |
| `c <query>` | search clipboard history (cliphist) |
| `s <query>` | Google suggestions |
| `g <query>` | GitHub search |
| `2 + 3` | inline calculator |
| _(empty)_ | show most-used items |
| `Enter` | launch selected result |
| `Delete` | remove history item |

## Configuration

On first run, `~/.config/qsflow/plugins.toml` is generated automatically:

```toml
[[plugins]]
id = "calculator"
keyword = ""

[[plugins]]
id = "app-search"
keyword = ""

[[plugins]]
id = "firefox-bookmarks"
keyword = "b"

[[plugins]]
id = "firefox-history"
keyword = "h"

[[plugins]]
id = "web-search"
keyword = "s"

[[plugins]]
id = "github"
keyword = "g"

[[plugins]]
id = "file-search"
keyword = "f"

[[plugins]]
id = "path-search"
keyword = "d"

[[plugins]]
id = "clipboard"
keyword = "c"
```

Reorder entries to change priority, change `keyword` to remap prefixes, or remove
a `[[plugins]]` block to disable it.

Theme colors are read from `~/.config/gtk-4.0/dank-colors.css` (falling back to
built-in defaults).

## Credit

- **[Quickshell](https://github.com/outfoxxed/quickshell)** — QtQuick shell toolkit that powers the Wayland overlay.
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — icon theme providing high-quality SVG icons.
- **[tokio](https://tokio.rs)** — async runtime driving the backend.
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — Firefox profile and usage database access.
- **[walkdir](https://github.com/BurntSushi/walkdir)** — recursive directory traversal for file and path search.
