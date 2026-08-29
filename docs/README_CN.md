<div align="center">

# QsFlow (QuickShell)

<img src="../images/application_default.png" alt="appicon" width="150" height="150"><br>

中文 | [English](../README.md)

</div>

## 概述

QsFlow 是一款 Wayland 原生的 Linux 应用启动器和快速搜索工具。在悬浮窗口中输入关键词，即可搜索已安装应用、Firefox 书签、网页建议、GitHub 以及进行即时数学计算。后端基于 Rust 异步实现，前端使用 [Quickshell](https://github.com/outfoxxed/quickshell) 的 QML 构建。

## 功能特性

- **应用启动器** — 模糊搜索 XDG 数据目录中的 `.desktop` 应用条目。
- **文件与路径搜索** — 遍历 `~/Desktop`、`~/Documents`、`~/Downloads` 和主目录，搜索结果可直接用默认应用打开。文件按扩展名显示对应图标。
- **剪贴板历史** — 通过 `c` 前缀搜索 `cliphist` 记录的剪贴板历史，选中即粘贴。
- **系统命令** — 输入 `lock`、`reboot`、`shutdown`、`suspend` 或 `logout`，直接从启动器执行操作。
- **Firefox 书签与历史** — 直接读取 `places.sqlite`，无需浏览器扩展。
- **网页搜索** — `s` 前缀获取 Google 搜索建议。
- **GitHub 搜索** — `g` 前缀直接跳转 GitHub。
- **即时计算器** — 自动识别并计算数学表达式，无需前缀。
- **有道翻译** — `tr` 前缀即时翻译，回车将译文复制到剪贴板。
- **使用历史** — 输入框留空时展示高频使用项（按次数排序），按 `Delete` 删除指定条目。
- **GTK 主题集成** — 自动从 GTK4 主题 CSS 读取主题色，读取失败时使用内置默认值。
- **插件系统** — TOML 驱动的插件注册表，无需改代码即可启用、禁用、调整顺序或修改触发前缀。新增插件启动时自动加载。
- **图标解析** — 从 Papirus、Breeze、Adwaita 和 hicolor 主题中解析应用图标，同时兼容 Flatpak 导出图标及旧版 pixmap 图标。解析结果会话内缓存。

## 环境要求

- 支持 `wlr-layer-shell` 协议的 **Wayland** 混成器
- **[Quickshell](https://github.com/outfoxxed/quickshell)**
- Rust 工具链（用于编译后端）
- Firefox（可选，用于书签和历史搜索）
- [cliphist](https://github.com/sentriz/cliphist)（可选，用于剪贴板历史）

## 安装

```bash
# 克隆并编译
git clone https://github.com/Prslc/QsFlow.git
cd QsFlow/core
cargo build --release

# 将二进制链接到 PATH
ln -s "$(pwd)/target/release/qsflow-core" ~/.local/bin/qsflow-core
```

然后在混成器配置中绑定快捷键（如 `Alt+Space`）来启动：

```bash
quickshell -p /path/to/QsFlow/ui/MainShell.qml
```

## 使用说明

| 输入 | 操作 |
|------|------|
| `firefox` | 模糊搜索已安装应用 |
| `b <关键词>` | 搜索 Firefox 书签 |
| `h <关键词>` | 搜索 Firefox 历史 |
| `f <关键词>` | 按文件名搜索 |
| `d <关键词>` | 按路径搜索（多词模糊） |
| `c <关键词>` | 搜索剪贴板历史 |
| `s <关键词>` | Google 搜索建议 |
| `g <关键词>` | GitHub 搜索 |
| `tr <关键词>` | 有道翻译（回车复制） |
| `?` | 显示可用关键词、默认功能及提示 |
| `lock` / `reboot` / `shutdown` | 系统命令 |
| `2 + 3` | 即时计算 |
| *(留空)* | 展示高频使用项 |
| `Enter` | 打开选中结果 |
| `Delete` | 删除历史条目 |

## 配置

首次运行时，`~/.config/qsflow/plugins.toml` 会自动生成：

```toml
[[plugins]]
id = "calculator"
keyword = ""
enable = true

[[plugins]]
id = "system-commands"
keyword = ""
enable = true

[[plugins]]
id = "app-search"
keyword = ""
enable = true

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

- **[Quickshell](https://github.com/outfoxxed/quickshell)** — QtQuick Shell 框架，负责 Wayland 悬浮面板渲染。
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — 高质量 SVG 图标主题。
- **[tokio](https://tokio.rs)** — Rust 异步运行时。
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — SQLite 绑定，用于读取 Firefox 数据库和使用历史。
- **[walkdir](https://github.com/BurntSushi/walkdir)** — 递归目录遍历，支撑文件搜索。
- **[nucleo](https://github.com/helix-editor/nucleo)** — 模糊匹配引擎，用于应用搜索。
- **[rustc-hash](https://github.com/rust-lang/rustc-hash)** — 快速非加密哈希，用于插件表和图标注销缓存。
