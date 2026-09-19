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
- **动态主题** — 跟随 DankMaterialShell 的 Material You 调色板，并可在 `theme.toml` 中逐字段覆盖配色，以及调整布局/模糊/动效。
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
`Esc`/点击卡片外会退出；常驻模式下热键切换窗口，关闭改为隐藏。

## 文档

完整的用户文档位于 [`docs/en/`](../en/)（英文）与 [`docs/zh_cn/`](.)（中文）。

- [使用说明](usage.md) —— 前缀、按键与剪贴板。
- [常驻模式](resident.md) —— systemd 单元与 IPC 动词。
- [主题](theme.md) —— 系统主题与 `theme.toml`（配色、模糊、布局、动效）。
- [插件](plugins.md) —— `plugins.toml`、内置插件与外部 JSON-RPC 主机。
- [JSON-RPC 2.0](jsonrpc.md) —— 通信协议与结果项 schema。

## 致谢

WayRun 的设计灵感来自 [Wox](https://github.com/wox-launcher/wox)。它建立在 Rust Wayland 生态之上——
[smithay-client-toolkit](https://github.com/Smithay/client-toolkit) 负责 layer-shell 管线，
[tiny-skia](https://github.com/RazrFalcon/tiny-skia) 负责光栅化，
[cosmic-text](https://github.com/pop-os/cosmic-text) 负责文本整形。
图标来自 [Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme) 主题。
