<div align="center">

# WayRun

<img src="images/application_default.png" alt="App Icon" width="150" height="150"><br>

English | [Chinese](docs/zh_cn/README_CN.md)

</div>

## Overview

WayRun is a Wayland-native application launcher and quick-search tool for Linux.
Type to search installed apps, Firefox bookmarks, web suggestions, and
inline math — all from a single floating overlay. It ships as one Rust binary,
`wayrun`, which runs as the overlay shell or, with `--core`, as the backend
service. The shell owns a `wlr-layer-shell` surface and rasterises its own card,
list and animations with
[tiny-skia](https://github.com/RazrFalcon/tiny-skia) into a `wl_shm` buffer, so
the frontend needs no GPU stack and no GUI toolkit; the core owns the plugin
registry, the JSON-RPC protocol and the usage database.

## Screenshots

| Light theme — most-used items | Dark theme — fuzzy app search (`android`) |
|-----------------------------------|-------------------------------------|
| ![WayRun — light theme](images/launcher.png) | ![WayRun — dark theme](images/launcher-search.png) |

## Features

- **Fuzzy app launcher** — search `.desktop` entries across XDG data dirs
  (name, GenericName, Keywords — plus each desktop action as its own row).
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
- Rust toolchain (`cargo build --release` builds the whole workspace; the shell is plain tiny-skia/cosmic-text crates)
- A font with CJK coverage for Chinese/Japanese queries (e.g. Source Han Sans)
- Firefox (optional, for bookmarks / history)
- [cliphist](https://github.com/sentriz/cliphist) (optional, for clipboard history)

## Quick Start

```bash
git clone https://github.com/Prslc/WayRun.git
cd WayRun
cargo build --release
ln -s "$(pwd)/target/release/wayrun" ~/.local/bin/wayrun
```

Bind a hotkey (e.g. Alt+Space) to launch the shell:

```bash
wayrun
```

The launcher is a full-screen overlay with a dimmed backdrop and a centered card.
In the default spawn-per-hotkey flow, `Esc` / clicking outside quits it; in
resident mode (below) the hotkey toggles the surface and dismiss hides it.

## Resident mode (optional — zero cold-start)

By default each hotkey press re-spawns the shell and its Rust core. To pop the
launcher up instantly, keep one resident process alive and toggle the surface
over a unix socket (`$XDG_RUNTIME_DIR/wayrun.sock`):

```ini
# ~/.config/systemd/user/wayrun-launcher.service
[Unit]
Description=WayRun launcher (resident)
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/home/you/.local/bin/wayrun
# the shell exits 0 on a core crash, which is a *clean* exit — never "on-failure"
Restart=always
RestartSec=2
# so commands the launcher runs see ~/.local/bin too
Environment=PATH=/home/you/.local/bin:/usr/local/bin:/usr/bin:/bin
# WAYLAND_DISPLAY/DISPLAY come from the graphical session (imported by the
# compositor); only XDG_RUNTIME_DIR is set, via the uid-proof `%t` specifier —
# no hardcoded display number or uid.
Environment=XDG_RUNTIME_DIR=%t
# resident mode: start hidden, toggle via `wayrun toggle`
Environment=WAYRUN_RESIDENT=1

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now wayrun-launcher
# niri hotkey — toggle instead of spawn:
#   Alt+Space { spawn-sh "wayrun toggle"; }
```

The verbs are `open` / `close` / `toggle` / `status`, spoken to the socket the
resident instance owns; `status` prints `visible` or `hidden`.
`WAYRUN_RESIDENT=1` selects resident mode (start hidden, dismiss hides); without
it a plain `wayrun` shows on launch and quits on dismiss, so the manual/dev
path is independent of the systemd service. To revert,
`systemctl --user disable --now wayrun-launcher` and restore the
spawn-per-hotkey binding.


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

On first run, `~/.config/wayrun/plugins.toml` is generated automatically:

```toml
# ~/.config/wayrun/plugins.toml
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
Hosts can be written by hand; the
[WayRun-Plugins](https://github.com/Prslc/WayRun-Plugins) workspace ships a
Python framework, example plugins, and a `template/` to copy from.

Theme colors are read from `~/.config/gtk-4.0/dank-colors.css` (falling back to
built-in defaults).

## JSON-RPC 2.0

`wayrun --core` speaks [JSON-RPC 2.0](https://www.jsonrpc.org/specification) over
stdin/stdout, alongside the launcher's text protocol: methods `search`, `top`,
`select`, `forget`, `run`, `resolve_icon`, `list_plugins`, `theme`, `ping`. The
full protocol spec and the result-item (schema) contract are in
[docs/en/jsonrpc.md](docs/en/jsonrpc.md).

## Credit

- **[Wox](https://github.com/wox-launcher/wox)** — the launcher concept is inspired by this project.
- **[tiny-skia](https://github.com/RazrFalcon/tiny-skia)** — the software rasteriser the shell draws its card, list and animations into a `wl_shm` buffer with.
- **[cosmic-text](https://github.com/pop-os/cosmic-text)** — text shaping and glyph rasterisation for the query and the result rows.
- **[resvg](https://github.com/RazrFalcon/resvg)** — SVG icon rasterisation.
- **[smithay-client-toolkit](https://github.com/Smithay/client-toolkit)** — Wayland client plumbing for the layer-shell overlay.
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — icon theme providing high-quality SVG icons.
- **[tokio](https://tokio.rs)** — async runtime driving the backend.
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — Firefox profile and usage database access.
- **[walkdir](https://github.com/BurntSushi/walkdir)** — recursive directory traversal for file and path search.
- **[nucleo](https://github.com/helix-editor/nucleo)** — fuzzy matching for `r` (commands) and `w` (windows).
- **[rustc-hash](https://github.com/rust-lang/rustc-hash)** — fast non-cryptographic hashing for plugin maps and icon cache.
- **[gio (gtk-rs)](https://gtk-rs.org/)** — GLib `GAppInfo` registry for application discovery and launching.
- **[fasteval](https://github.com/likebike/fasteval)** — calculator expression evaluation.
