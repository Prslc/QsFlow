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

## 演示

<p align="center">
  <img src="../../images/demo.webp" alt="WayRun —— 应用搜索、文件搜索、动态主题与书签">
</p>

## 功能特性

- **应用启动** — 跨 XDG 数据目录的 `.desktop` 模糊搜索，经 GLib `GAppInfo`
  启动（正确处理 Exec 引号、字段码与 `DBusActivatable`），并逐行解析 Flatpak 与主题图标。
- **快速搜索** — 文件与路径、Firefox 书签与历史、剪贴板历史、网页建议、`$PATH`
  命令、打开的 niri 窗口、系统命令，以及即时计算。
- **使用历史** — 留空时展示高频项，按 `Delete` 删除。
- **可主题化** — 跟随 DankMaterialShell 的 Material You 调色板，并可用
  `theme.toml` 覆盖配色、模糊、布局与动效。
- **可扩展** — TOML 插件注册表，以及外部 JSON-RPC 主机。

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

- [使用说明](usage.md) —— 前缀、按键与剪贴板。
- [常驻模式](resident.md) —— systemd 单元与 IPC 动词。
- [主题](theme.md) —— 系统主题与 `theme.toml`（配色、模糊、布局、动效）。
- [插件](plugins.md) —— `plugins.toml`、内置插件与外部 JSON-RPC 主机。
- [JSON-RPC 2.0](jsonrpc.md) —— 通信协议与结果项 schema。

## 致谢

- **[Wox](https://github.com/wox-launcher/wox)** —— 本启动器的灵感来源。
- **[tiny-skia](https://github.com/RazrFalcon/tiny-skia)** 与 **[cosmic-text](https://github.com/pop-os/cosmic-text)** —— 覆盖层的软件光栅化与文本整形。
- **[smithay-client-toolkit](https://github.com/Smithay/client-toolkit)** —— Wayland layer-shell 客户端管线。
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** —— 图标主题。
