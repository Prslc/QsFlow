<div align="center">

# WayRun

<img src="images/application_default.png" alt="App Icon" width="150" height="150"><br>

English | [Chinese](docs/zh_cn/README_CN.md)

[![CI](https://github.com/Prslc/WayRun/actions/workflows/ci.yml/badge.svg)](https://github.com/Prslc/WayRun/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/Prslc/WayRun?color=yellow)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)](https://www.rust-lang.org/)
[![Wayland](https://img.shields.io/badge/Wayland-native-4a90d9?logo=wayland&logoColor=white)](https://wayland.freedesktop.org/)

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
- **Proper app launching** — desktop apps open via the GLib `GAppInfo` registry
  (`g_app_info_launch`, honouring Exec quoting, field codes, env and
  `DBusActivatable` single-instance) rather than `Exec=` re-parsed through a
  shell.
- **Files & paths** — walk `~/Desktop`, `~/Documents`, `~/Downloads` and home; open in the default app.
- **Clipboard history** — search and paste from `cliphist` via `c`.
- **System commands** — `lock`, `reboot`, `shutdown`, `suspend`, `logout`.
- **Run commands** — fuzzy-search `$PATH` executables via `r` and run them (with args).
- **Window switcher** — switch to any open niri window via `w`.
- **Firefox bookmarks & history** — reads a copy of the newest profile's
  `places.sqlite` (avoiding a lock on the live database).
- **Web search** — Google suggestions (`s`).
- **Inline tools** — calculator.
- **Usage history** — frequent items on empty input; `Delete` removes an entry.
- **Dynamic theming** — follows DankMaterialShell's Material You palette, with
  optional per-field overrides and layout/blur/motion knobs in `theme.toml`.
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
resident mode the hotkey toggles the surface and dismiss hides it.

## Documentation

Full user docs live in [`docs/en/`](docs/en/); the Chinese editions are in
[`docs/zh_cn/`](docs/zh_cn/).

- [Usage](docs/en/usage.md) — prefixes, keybindings, and the clipboard.
- [Resident mode](docs/en/resident.md) — the systemd unit and the IPC verbs.
- [Theme](docs/en/theme.md) — the system theme and `theme.toml` (colors, blur,
  layout, motion).
- [Plugins](docs/en/plugins.md) — `plugins.toml`, the built-ins, and external
  JSON-RPC hosts.
- [JSON-RPC 2.0](docs/en/jsonrpc.md) — the wire protocol and the result schema.

## Credit

WayRun is inspired by [Wox](https://github.com/wox-launcher/wox). It is built on
the Rust Wayland ecosystem — in particular
[smithay-client-toolkit](https://github.com/Smithay/client-toolkit) for the
layer-shell plumbing, [tiny-skia](https://github.com/RazrFalcon/tiny-skia) for
rasterisation, and [cosmic-text](https://github.com/pop-os/cosmic-text) for text
shaping. Icons come from the
[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme) theme.
