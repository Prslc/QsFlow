<div align="center">

# WayRun

<img src="../../images/application_default.png" alt="appicon" width="150" height="150"><br>

中文 | [English](../../README.md)

[![CI](https://github.com/Prslc/WayRun/actions/workflows/ci.yml/badge.svg)](https://github.com/Prslc/WayRun/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/Prslc/WayRun?color=yellow)](../../LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)](https://www.rust-lang.org/)
[![Wayland](https://img.shields.io/badge/Wayland-native-4a90d9?logo=wayland&logoColor=white)](https://wayland.freedesktop.org/)

</div>

## 概述

WayRun 是一款 Wayland 原生的 Linux 应用启动器和快速搜索工具。在悬浮窗口中输入关键词，即可搜索已安装应用、Firefox 书签、网页建议，并进行即时数学计算。整个项目只产出一个 Rust 可执行文件 `wayrun`：默认跑覆盖层壳，带 `--core` 时跑后端服务。壳自己持有 `wlr-layer-shell` 表面，用
[tiny-skia](https://github.com/RazrFalcon/tiny-skia) 把卡片、列表与动画直接光栅化进 `wl_shm` 缓冲，因此不需要 GPU 栈，也不依赖任何 GUI 工具包；内核负责插件注册表、JSON-RPC 协议与使用历史库。

## 截图

| 亮色主题 — 高频使用项 | 暗色主题 — 模糊应用搜索（`android`） |
|------------------------|-------------------------------------|
| ![WayRun — 亮色主题](../../images/launcher.png) | ![WayRun — 暗色主题](../../images/launcher-search.png) |

## 功能特性

- **模糊应用启动器** — 搜索 XDG 数据目录中的 `.desktop` 条目
  （Name、GenericName、Keywords，以及每条 desktop action 作为独立结果行）。
- **规范的应用启动** — 桌面应用经 GLib `GAppInfo` 注册表启动
  （`g_app_info_launch`，正确处理 Exec 引号、字段码、环境变量与
  `DBusActivatable` 单例），而非把 `Exec=` 交给 shell 重新解析。
- **文件与路径搜索** — 遍历 `~/Desktop`、`~/Documents`、`~/Downloads` 与主目录，直接打开结果。
- **剪贴板历史** — 通过 `c` 前缀搜索并粘贴 `cliphist` 记录。
- **系统命令** — `lock`、`reboot`、`shutdown`、`suspend`、`logout`。
- **命令运行** — 通过 `r` 前缀模糊搜索 `$PATH` 可执行文件并运行（可带参数）。
- **窗口切换** — 通过 `w` 前缀切换到任一打开的 niri 窗口。
- **Firefox 书签与历史** — 读取最新配置目录 `places.sqlite` 的副本（避免锁定正在使用的数据库）。
- **网页搜索** — Google 搜索建议（`s`）。
- **内联工具** — 即时计算。
- **使用历史** — 留空时展示高频项，按 `Delete` 删除。
- **动态主题** — 跟随 DankMaterialShell 的 Material You 调色板，并可在 `theme.toml` 中逐字段覆盖配色，以及调整布局/磨砂/动效。
- **插件系统** — TOML 注册表，可启用、禁用、调整顺序或修改前缀。
- **图标解析** — Papirus、Breeze、Adwaita、hicolor 及 Flatpak；会话内缓存。

## 环境要求

- 支持 `wlr-layer-shell` 协议的 **Wayland** 混成器
- Rust 工具链（前后端都是 Rust，壳只依赖 tiny-skia/cosmic-text 等普通 crate）
- 覆盖中日文输入的字体（如 Source Han Sans）
- Firefox（可选，用于书签和历史搜索）
- [cliphist](https://github.com/sentriz/cliphist)（可选，用于剪贴板历史）

## 快速开始

```bash
git clone https://github.com/Prslc/WayRun.git
cd WayRun
cargo build --release
ln -s "$(pwd)/target/release/wayrun" ~/.local/bin/wayrun
```

然后在混成器配置中绑定快捷键（如 `Alt+Space`）来启动：

```bash
wayrun
```

启动器以全屏覆盖方式打开，带调暗背景与居中卡片。默认的按热键拉起流程下，
`Esc`/点击卡片外会退出；常驻模式（见下）下热键切换窗口，关闭改为隐藏。

## 常驻模式（可选 —— 零冷启动）

默认每次按热键都会重新拉起壳与 Rust 内核。要让启动器即刻弹出，让一个常驻进程
保持存活，通过 unix socket（`$XDG_RUNTIME_DIR/wayrun.sock`）切换表面：

```ini
# ~/.config/systemd/user/wayrun-launcher.service
[Unit]
Description=WayRun launcher (resident)
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/home/you/.local/bin/wayrun
Restart=always
RestartSec=2
# 让启动器执行的命令也能看到 ~/.local/bin
Environment=PATH=/home/you/.local/bin:/usr/local/bin:/usr/bin:/bin
# WAYLAND_DISPLAY/DISPLAY 由图形会话导入；这里只设 XDG_RUNTIME_DIR（uid 无关的 %t）
Environment=XDG_RUNTIME_DIR=%t
# 常驻模式：隐藏启动，用 `wayrun toggle` 切换
Environment=WAYRUN_RESIDENT=1

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now wayrun-launcher
# niri 热键 —— 切换而非重新拉起：
#   Alt+Space { spawn-sh "wayrun toggle"; }
```

动词为 `open` / `close` / `toggle` / `status`，都发给常驻实例持有的这个 socket；
`status` 打印 `visible` 或 `hidden`。`WAYRUN_RESIDENT=1` 选中常驻模式（隐藏启动、
关闭即隐藏）；不带该变量时，直接 `wayrun` 保持旧行为——启动即弹出、关闭即退出，
因此手动/开发路径与 systemd 服务相互独立。恢复：`systemctl --user disable --now
wayrun-launcher` 并还原绑定的启动方式。


## 使用说明

| 输入 | 操作 |
|------|------|
| `firefox` | 模糊搜索已安装应用 |
| `b <关键词>` | 搜索 Firefox 书签 |
| `h <关键词>` | 搜索 Firefox 历史 |
| `f <关键词>` | 按文件名搜索 |
| `d <关键词>` | 按路径搜索（多词模糊） |
| `r <关键词>` | 模糊搜索 `$PATH` 可执行文件并运行 |
| `w <关键词>` | 切换到匹配的打开窗口 |
| `c <关键词>` | 搜索剪贴板历史 |
| `s <关键词>` | Google 搜索建议 |
| `?` | 显示关键词模式、默认功能与提示 |
| `lock` / `reboot` / `shutdown` | 系统命令 |
| `2 + 3` | 即时计算 |
| *(留空)* | 展示高频使用项 |
| `Enter` | 打开选中结果 |
| `Delete` | 删除历史条目 |

## 配置

`~/.config/wayrun/` 下有两个可选文件，都会被监听，修改后无需重启即可生效：

- **`plugins.toml`** —— 插件注册表，首次运行时生成。可启用/禁用、调整顺序或重映射前缀，
  以及声明外部 JSON-RPC 主机。见 **[plugins.md](plugins.md)**。
- **`theme.toml`** —— 配色与外观。跟随 DankMaterialShell 的 Material You 调色板，
  并可覆盖配色、背景模糊、布局与动效。见 **[theme.md](theme.md)**。

## JSON-RPC 2.0

`wayrun --core` 在 stdin/stdout 上支持 [JSON-RPC 2.0](https://www.jsonrpc.org/specification)，
与启动器文本协议混用：方法 `search`、`top`、`select`、`forget`、`run`、`action`、
`launch`、`open`、`copy`、`resolve_icon`、`list_plugins`、`theme`、`ping`。完整协议与
结果项 schema 见 [zh_cn/jsonrpc.md](jsonrpc.md)。

## 致谢

- **[Wox](https://github.com/wox-launcher/wox)** — 本启动器的设计灵感来源。
- **[tiny-skia](https://github.com/RazrFalcon/tiny-skia)** — 软件光栅化器，壳用它把卡片、列表与动画画进 `wl_shm` 缓冲。
- **[cosmic-text](https://github.com/pop-os/cosmic-text)** — 文本整形与字形栅格化，用于输入框与结果行。
- **[resvg](https://github.com/RazrFalcon/resvg)** — SVG 图标栅格化。
- **[smithay-client-toolkit](https://github.com/Smithay/client-toolkit)** — 覆盖层所需的 Wayland 客户端管线。
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — 高质量 SVG 图标主题。
- **[tokio](https://tokio.rs)** — Rust 异步运行时。
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — SQLite 绑定，用于读取 Firefox 数据库和使用历史。
- **[walkdir](https://github.com/BurntSushi/walkdir)** — 递归目录遍历，支撑文件搜索。
- **[nucleo](https://github.com/helix-editor/nucleo)** — 模糊匹配引擎，用于 `r`（命令）与 `w`（窗口）搜索。
- **[rustc-hash](https://github.com/rust-lang/rustc-hash)** — 快速非加密哈希，用于插件表和图标注销缓存。
- **[gio (gtk-rs)](https://gtk-rs.org/)** — GLib `GAppInfo` 应用注册表，用于应用发现与启动。
- **[fasteval](https://github.com/likebike/fasteval)** — 计算器表达式求值。
