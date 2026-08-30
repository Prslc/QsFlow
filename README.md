<div align="center">

# QsFlow (QuickShell)

<img src="images/application_default.png" alt="App Icon" width="150" height="150"><br>

English | [Chinese](docs/README_CN.md)

</div>

## Overview

QsFlow is a Wayland-native application launcher and quick-search tool for Linux.
Type to search installed apps, Firefox bookmarks, web suggestions, GitHub, and
inline math — all from a single floating overlay. Built with a Rust backend and a
QML frontend powered by [Quickshell](https://github.com/outfoxxed/quickshell).

## Screenshots

| Light theme — most-used items | Dark theme — fuzzy app search (`android`) |
|-----------------------------------|-------------------------------------|
| ![QsFlow — light theme](images/launcher.png) | ![QsFlow — dark theme](images/launcher-search.png) |

## Features

- **Fuzzy app launcher** — search `.desktop` entries across XDG data dirs.
- **Files & paths** — walk `~/Desktop`, `~/Documents`, `~/Downloads` and home; open in the default app.
- **Clipboard history** — search and paste from `cliphist` via `c`.
- **System commands** — `lock`, `reboot`, `shutdown`, `suspend`, `logout`.
- **Firefox bookmarks & history** — reads `places.sqlite` directly.
- **Web & GitHub** — Google suggestions (`s`) and GitHub links (`g`).
- **Inline tools** — calculator; Youdao translation (`tr`, Enter copies).
- **Usage history** — frequent items on empty input; `Delete` removes an entry.
- **GTK theme integration** — colors from your GTK4 theme CSS.
- **Plugin system** — TOML registry; enable, disable, reorder, or remap keywords.
- **Icon resolution** — Papirus, Breeze, Adwaita, hicolor + Flatpak; cached per session.

## Requirements

- **Wayland** compositor with `wlr-layer-shell` support
- **[Quickshell](https://github.com/outfoxxed/quickshell)**
- Rust toolchain (to build the core backend)
- Firefox (optional, for bookmarks / history)
- [cliphist](https://github.com/sentriz/cliphist) (optional, for clipboard history)

## Quick Start

```bash
git clone https://github.com/Prslc/QsFlow.git
cd QsFlow/core
cargo build --release
ln -s "$(pwd)/target/release/qsflow-core" ~/.local/bin/qsflow-core
```

Bind a hotkey (e.g. Alt+Space) to launch the shell:

```bash
quickshell -p /path/to/QsFlow/ui/MainShell.qml
```

The launcher opens full-screen with a dimmed backdrop and a centered card; click
outside the card or press `Esc` to close it.

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
| `tr <query>` | Youdao translation (Enter to copy) |
| `?` | show keyword modes, default functions, and hints |
| `lock` / `reboot` / `shutdown` | system commands |
| `2 + 3` | inline calculator |
| _(empty)_ | show most-used items |
| `Enter` | launch selected result |
| `Delete` | remove history item |

## Configuration

On first run, `~/.config/qsflow/plugins.toml` is generated automatically:

```toml
# ~/.config/qsflow/plugins.toml
[[plugins]]
id = "app-search"
keyword = ""       # empty = no prefix
enable = true

[[plugins]]
id = "firefox-bookmarks"
keyword = "b"

[[plugins]]
id = "translate"
keyword = "tr"
```

Reorder entries to change priority, edit `keyword` to remap prefixes, or set
`enable = false` to disable a plugin. Removed or unknown plugin IDs are ignored.

The Youdao translation plugin reads credentials from
`~/.config/qsflow/translate.toml` (a template is written on first run):

```toml
app_token = "..."
app_secret = "..."
lang_from = "Auto"    # optional, defaults Auto
lang_to = "English"   # optional, defaults English
```

Language values accept display names such as `Auto`, `English`,
`Chinese (Simplified)`. It requires `wl-copy` on `PATH` to copy results.

Theme colors are read from `~/.config/gtk-4.0/dank-colors.css` (falling back to
built-in defaults).

## JSON-RPC 2.0

The `qsflow-core` backend also speaks [JSON-RPC 2.0](https://www.jsonrpc.org/specification)
over the same stdin/stdout. Lines that parse to an object with
`"jsonrpc":"2.0"` and a `method` are handled as RPC requests and can be mixed
with the launcher's text protocol. Responses are newline-delimited JSON on
stdout.

```sh
printf '%s\n' '{"jsonrpc":"2.0","method":"search","params":{"text":"firefox"},"id":1}' | qsflow-core
# -> {"jsonrpc":"2.0","result":[...],"id":1}
```

| Method | Params | Result |
|--------|--------|--------|
| `search` | `"query"` or `{"text"}` / `{"query"}` | array of result items |
| `top` | — | most-used items |
| `select` | item object | `null` (records usage) |
| `forget` | `on_click` string or `{"on_click"}` | `null` |
| `run` | `"cmd"` or `{"cmd"}` | `null` |
| `list_plugins` | — | plugin metadata |
| `theme` | — | theme colors |
| `ping` | — | `"pong"` |

A request without an `id` is a notification (side effect only, no response).
Unknown methods return `-32601`; malformed requests `-32600`; bad params
`-32602`.

## Credit

- **[Wox](https://github.com/wox-launcher/wox)** — the launcher concept is inspired by this project.
- **[Flow.translate-youdao](https://github.com/Prslc/Flow.translate-youdao)** — the Youdao translation plugin is adapted from this project.
- **[Quickshell](https://github.com/outfoxxed/quickshell)** — QtQuick shell toolkit that powers the Wayland overlay.
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — icon theme providing high-quality SVG icons.
- **[tokio](https://tokio.rs)** — async runtime driving the backend.
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — Firefox profile and usage database access.
- **[walkdir](https://github.com/BurntSushi/walkdir)** — recursive directory traversal for file and path search.
- **[nucleo](https://github.com/helix-editor/nucleo)** — fuzzy matching for application search.
- **[rustc-hash](https://github.com/rust-lang/rustc-hash)** — fast non-cryptographic hashing for plugin maps and icon cache.
