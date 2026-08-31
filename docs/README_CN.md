<div align="center">

# QsFlow (QuickShell)

<img src="../images/application_default.png" alt="appicon" width="150" height="150"><br>

中文 | [English](../README.md)

</div>

## 概述

QsFlow 是一款 Wayland 原生的 Linux 应用启动器和快速搜索工具。在悬浮窗口中输入关键词，即可搜索已安装应用、Firefox 书签、网页建议、GitHub 以及进行即时数学计算。后端基于 Rust 异步实现，前端使用 [Quickshell](https://github.com/outfoxxed/quickshell) 的 QML 构建。

## 截图

| 亮色主题 — 高频使用项 | 暗色主题 — 模糊应用搜索（`android`） |
|------------------------|-------------------------------------|
| ![QsFlow — 亮色主题](../images/launcher.png) | ![QsFlow — 暗色主题](../images/launcher-search.png) |

## 功能特性

- **模糊应用启动器** — 搜索 XDG 数据目录中的 `.desktop` 条目。
- **文件与路径搜索** — 遍历 `~/Desktop`、`~/Documents`、`~/Downloads` 与主目录，直接打开结果。
- **剪贴板历史** — 通过 `c` 前缀搜索并粘贴 `cliphist` 记录。
- **系统命令** — `lock`、`reboot`、`shutdown`、`suspend`、`logout`。
- **命令运行** — 通过 `r` 前缀模糊搜索 `$PATH` 可执行文件并运行（可带参数）。
- **窗口切换** — 通过 `w` 前缀切换到任一打开的 niri 窗口。
- **Firefox 书签与历史** — 直接读取 `places.sqlite`。
- **网页与 GitHub** — Google 搜索建议（`s`）与 GitHub 链接（`g`）。
- **内联工具** — 即时计算；有道翻译（`tr`，回车复制）。
- **使用历史** — 留空时展示高频项，按 `Delete` 删除。
- **GTK 主题集成** — 从 GTK4 主题 CSS 读取主题色。
- **插件系统** — TOML 注册表，可启用、禁用、调整顺序或修改前缀。
- **图标解析** — Papirus、Breeze、Adwaita、hicolor 及 Flatpak；会话内缓存。

## 环境要求

- 支持 `wlr-layer-shell` 协议的 **Wayland** 混成器
- **[Quickshell](https://github.com/outfoxxed/quickshell)**
- Rust 工具链（用于编译后端）
- Firefox（可选，用于书签和历史搜索）
- [cliphist](https://github.com/sentriz/cliphist)（可选，用于剪贴板历史）

## 快速开始

```bash
git clone https://github.com/Prslc/QsFlow.git
cd QsFlow/core
cargo build --release
ln -s "$(pwd)/target/release/qsflow-core" ~/.local/bin/qsflow-core
```

然后在混成器配置中绑定快捷键（如 `Alt+Space`）来启动：

```bash
quickshell -p /path/to/QsFlow/ui/MainShell.qml
```

启动器以全屏覆盖方式打开，带调暗背景与居中卡片；点击卡片外或按 `Esc` 关闭。

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
| `g <关键词>` | GitHub 搜索 |
| `tr <关键词>` | 有道翻译（回车复制） |
| `?` | 显示关键词模式、默认功能与提示 |
| `lock` / `reboot` / `shutdown` | 系统命令 |
| `2 + 3` | 即时计算 |
| *(留空)* | 展示高频使用项 |
| `Enter` | 打开选中结果 |
| `Delete` | 删除历史条目 |

## 配置

首次运行时，`~/.config/qsflow/plugins.toml` 会自动生成：

```toml
# ~/.config/qsflow/plugins.toml
[[plugins]]
id = "app-search"
keyword = ""       # 留空 = 无前缀
enable = true

[[plugins]]
id = "firefox-bookmarks"
keyword = "b"

[[plugins]]
id = "translate"
keyword = "tr"
```

调整条目顺序可改变优先级，修改 `keyword` 可重映射触发前缀，设置 `enable = false` 可禁用插件。未识别或已删除的插件 ID 会被自动跳过。

有道翻译插件从 `~/.config/qsflow/translate.toml` 读取凭据（首次运行自动生成模板）：

```toml
app_token = "..."
app_secret = "..."
lang_from = "Auto"    # 可选，默认 Auto
lang_to = "English"   # 可选，默认 English
```

语言值使用显示名，如 `Auto`、`English`、`Chinese (Simplified)`。复制译文需要系统安装 `wl-copy`。

主题色默认从 `~/.config/gtk-4.0/dank-colors.css` 读取，读取失败则使用内置默认值。

## 致谢

- **[Wox](https://github.com/wox-launcher/wox)** — 本启动器的设计灵感来源。
- **[Flow.translate-youdao](https://github.com/Prslc/Flow.translate-youdao)** — 有道翻译插件改编自本项目。
- **[Quickshell](https://github.com/outfoxxed/quickshell)** — QtQuick Shell 框架，负责 Wayland 悬浮面板渲染。
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — 高质量 SVG 图标主题。
- **[tokio](https://tokio.rs)** — Rust 异步运行时。
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — SQLite 绑定，用于读取 Firefox 数据库和使用历史。
- **[walkdir](https://github.com/BurntSushi/walkdir)** — 递归目录遍历，支撑文件搜索。
- **[nucleo](https://github.com/helix-editor/nucleo)** — 模糊匹配引擎，用于应用搜索。
- **[rustc-hash](https://github.com/rust-lang/rustc-hash)** — 快速非加密哈希，用于插件表和图标注销缓存。
- **[gio (gtk-rs)](https://gtk-rs.org/)** — GLib `GAppInfo` 应用注册表，用于应用发现与启动。
- **[fasteval](https://github.com/likebike/fasteval)** — 计算器表达式求值。
