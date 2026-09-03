<div align="center">

# QsFlow (QuickShell)

<img src="images/application_default.png" alt="App Icon" width="150" height="150"><br>

English | [Chinese](docs/README_CN.md)

</div>

## Overview

QsFlow is a Wayland-native application launcher and quick-search tool for Linux.
Type to search installed apps, Firefox bookmarks, web suggestions, and
inline math — all from a single floating overlay. Built with a Rust backend and a
QML frontend powered by [Quickshell](https://github.com/outfoxxed/quickshell).

## Screenshots

| Light theme — most-used items | Dark theme — fuzzy app search (`android`) |
|-----------------------------------|-------------------------------------|
| ![QsFlow — light theme](images/launcher.png) | ![QsFlow — dark theme](images/launcher-search.png) |

## Features

- **Fuzzy app launcher** — search `.desktop` entries across XDG data dirs.
- **Proper app launching** — apps open via the GLib `GAppInfo` registry
  (`g_app_info_launch`, honouring Exec quoting, field codes, env and
  `DBusActivatable` single-instance), never a raw `sh -c`.
- **Files & paths** — walk `~/Desktop`, `~/Documents`, `~/Downloads` and home; open in the default app.
- **Clipboard history** — search and paste from `cliphist` via `c`.
- **System commands** — `lock`, `reboot`, `shutdown`, `suspend`, `logout`.
- **Run commands** — fuzzy-search `$PATH` executables via `r` and run them (with args).
- **Window switcher** — switch to any open niri window via `w`.
- **Firefox bookmarks & history** — reads `places.sqlite` directly.
- **Web search** — Google suggestions (`s`).
- **Inline tools** — calculator.
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
# qsflow is a symlink to quickshell: the process shows its own name in ps/top
# (cosmetic — IPC is keyed by the -p config path, not the binary name)
ln -s "$(command -v quickshell)" ~/.local/bin/qsflow
```

Bind a hotkey (e.g. Alt+Space) to launch the shell:

```bash
qsflow -p /path/to/QsFlow/ui/MainShell.qml
```

The launcher is a full-screen overlay with a dimmed backdrop and a centered card.
In the default spawn-per-hotkey flow, `Esc` / clicking outside quits it; in
resident mode (below) the hotkey toggles the window and dismiss hides it.

## Resident mode (optional — zero cold-start)

By default each hotkey press re-spawns the QML shell and its Rust core, so the
first keystroke pays a ~300ms cold start (mostly QML/Qt init, not the core). To
pop the launcher up instantly, keep one resident shell + core alive and toggle
the window via Quickshell's IPC:

```ini
# ~/.config/systemd/user/qsflow-launcher.service
[Unit]
Description=QsFlow launcher (resident quickshell + core)
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=qsflow -p /abs/path/to/QsFlow/ui/MainShell.qml
Restart=on-failure
RestartSec=2
# qsflow-core and the qsflow symlink both live in ~/.local/bin; the PATH below
# (that dir MUST stay first) resolves them
Environment=PATH=/home/you/.local/bin:/usr/local/bin:/usr/bin:/bin
# WAYLAND_DISPLAY/DISPLAY come from the graphical session (imported by the
# compositor); only XDG_RUNTIME_DIR is set, via the uid-proof `%t` specifier —
# no hardcoded display number or uid.
Environment=XDG_RUNTIME_DIR=%t
# resident mode: start hidden, toggle via `ipc call launcher toggle`
Environment=QSFLOW_RESIDENT=1

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now qsflow-launcher
# niri hotkey — toggle instead of spawn:
#   Alt+Space { spawn-sh "quickshell ipc --path $HOME/Project/QsFlow/ui/MainShell.qml call launcher toggle"; }
```

`MainShell.qml` exposes an `IpcHandler` (`target: "launcher"`) with
`open` / `close` / `toggle`. `QSFLOW_RESIDENT=1` selects resident mode (start
hidden, dismiss hides); without it a plain `qsflow -p ui/MainShell.qml` keeps
the old behaviour — shows on launch and quits on dismiss, so the manual/dev path
is independent of the systemd service. To revert, `systemctl --user disable
--now qsflow-launcher` and restore the spawn-per-hotkey binding.


## Usage

| Input | Action |
|-------|--------|
| `firefox` | fuzzy-search installed applications |
| `b <query>` | search Firefox bookmarks |
| `h <query>` | search Firefox history |
| `f <query>` | search files by name |
| `d <query>` | search files by path (multi-token fuzzy) |
| `r <query>` | fuzzy-search `$PATH` executables and run one |
| `w <query>` | switch focus to a matching open niri window |
| `c <query>` | search clipboard history (cliphist) |
| `s <query>` | Google suggestions |
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
id = "web-search"
keyword = "s"
```

Reorder entries to change priority, edit `keyword` to remap prefixes, or set
`enable = false` to disable a plugin. Removed or unknown plugin IDs are ignored.
An entry may also declare `command`, naming an external JSON-RPC 2.0 host. The
value is a single executable token — resolved on `PATH`, or an absolute path;
no arguments or shell syntax (scripts need a shebang and exec bit). The core
spawns it, relays `search`, and discovers the plugin's identity from the
host's `list_plugins` response; both the identity `icon` and result `icon`
fields accept the `papirus:` scheme (resolved to an absolute Papirus path).

Theme colors are read from `~/.config/gtk-4.0/dank-colors.css` (falling back to
built-in defaults).

## JSON-RPC 2.0

`qsflow-core` speaks [JSON-RPC 2.0](https://www.jsonrpc.org/specification) over the
same stdin/stdout, alongside the launcher's text protocol: methods `search`, `top`,
`select`, `forget`, `run`, `resolve_icon`, `list_plugins`, `theme`, `ping`. The
full protocol spec and the result-item (schema) contract are in
[docs/en/jsonrpc.md](docs/en/jsonrpc.md).

## Credit

- **[Wox](https://github.com/wox-launcher/wox)** — the launcher concept is inspired by this project.
- **[Quickshell](https://github.com/outfoxxed/quickshell)** — QtQuick shell toolkit that powers the Wayland overlay.
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — icon theme providing high-quality SVG icons.
- **[tokio](https://tokio.rs)** — async runtime driving the backend.
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — Firefox profile and usage database access.
- **[walkdir](https://github.com/BurntSushi/walkdir)** — recursive directory traversal for file and path search.
- **[nucleo](https://github.com/helix-editor/nucleo)** — fuzzy matching for application search.
- **[rustc-hash](https://github.com/rust-lang/rustc-hash)** — fast non-cryptographic hashing for plugin maps and icon cache.
- **[gio (gtk-rs)](https://gtk-rs.org/)** — GLib `GAppInfo` registry for application discovery and launching.
- **[fasteval](https://github.com/likebike/fasteval)** — calculator expression evaluation.
