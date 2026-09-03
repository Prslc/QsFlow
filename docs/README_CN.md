<div align="center">

# QsFlow (QuickShell)

<img src="../images/application_default.png" alt="appicon" width="150" height="150"><br>

中文 | [English](../README.md)

</div>

## 概述

QsFlow 是一款 Wayland 原生的 Linux 应用启动器和快速搜索工具。在悬浮窗口中输入关键词，即可搜索已安装应用、Firefox 书签、网页建议，并进行即时数学计算。后端基于 Rust 异步实现，前端使用 [Quickshell](https://github.com/outfoxxed/quickshell) 的 QML 构建。

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
- **网页搜索** — Google 搜索建议（`s`）。
- **内联工具** — 即时计算。
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
# qsflow 是 quickshell 的软链：进程在 ps/top 里显示自己的名字
# （纯表面 —— IPC 按 -p 配置路径路由，与二进制名无关）
ln -s "$(command -v quickshell)" ~/.local/bin/qsflow
```

然后在混成器配置中绑定快捷键（如 `Alt+Space`）来启动：

```bash
qsflow -p /path/to/QsFlow/ui/MainShell.qml
```

启动器以全屏覆盖方式打开，带调暗背景与居中卡片。默认的按热键拉起流程下，
`Esc`/点击卡片外会退出；常驻模式（见下）下热键切换窗口，关闭改为隐藏。

## 常驻模式（可选 —— 零冷启动）

默认每次按热键都会重新拉起 QML 壳与 Rust 内核，首次按键需付 ~300ms 冷启动
（主要是 QML/Qt 初始化，不是核心）。要让启动器即刻弹出，让一个常驻的壳+内核
保持存活，通过 Quickshell 的 IPC 切换窗口：

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
# qsflow-core 与 qsflow 软链都在 ~/.local/bin；由下面的 PATH（该目录须保持第一）解析
Environment=PATH=/home/you/.local/bin:/usr/local/bin:/usr/bin:/bin
# WAYLAND_DISPLAY/DISPLAY 由图形会话导入；这里只设 XDG_RUNTIME_DIR（uid 无关的 %t）
Environment=XDG_RUNTIME_DIR=%t
# 常驻模式：隐藏启动，用 `ipc call launcher toggle` 切换
Environment=QSFLOW_RESIDENT=1

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now qsflow-launcher
# niri 热键 —— 切换而非重新拉起：
#   Alt+Space { spawn-sh "quickshell ipc --path $HOME/Project/QsFlow/ui/MainShell.qml call launcher toggle"; }
```

`MainShell.qml` 暴露了一个 `IpcHandler`（`target: "launcher"`），带
`open` / `close` / `toggle`。`QSFLOW_RESIDENT=1` 选中常驻模式（隐藏启动、关闭即隐藏）；
不带该变量时，直接 `qsflow -p ui/MainShell.qml` 保持旧行为——启动即弹出、关闭即退出，
因此手动/开发路径与 systemd 服务相互独立。恢复：`systemctl --user disable --now
qsflow-launcher` 并还原绑定的启动方式。


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
id = "web-search"
keyword = "s"
```

调整条目顺序可改变优先级，修改 `keyword` 可重映射触发前缀，设置 `enable = false` 可禁用插件。未识别或已删除的插件 ID 会被自动跳过。
条目还可以声明可选的 `command` 字段，指向外部 JSON-RPC 2.0 主机。该值必须是单个可执行文件 token——按 `PATH` 解析或写绝对路径，不含参数、无 shell 语法（脚本需 shebang + 执行位）。core 每次查询时拉起它、转发 `search`，并经主机的 `list_plugins` 响应发现插件身份；身份 `icon` 与结果 `icon` 字段都支持 `papirus:` 规范（core 解析为 Papirus 绝对路径）。

主题色默认从 `~/.config/gtk-4.0/dank-colors.css` 读取，读取失败则使用内置默认值。

## JSON-RPC 2.0

`qsflow-core` 在同一条 stdin/stdout 上支持 [JSON-RPC 2.0](https://www.jsonrpc.org/specification)，
与启动器文本协议混用：方法 `search`、`top`、`select`、`forget`、`run`、`resolve_icon`、
`list_plugins`、`theme`、`ping`。完整协议与结果项 schema 见
[zh_cn/jsonrpc.md](zh_cn/jsonrpc.md)。

## 致谢

- **[Wox](https://github.com/wox-launcher/wox)** — 本启动器的设计灵感来源。
- **[Quickshell](https://github.com/outfoxxed/quickshell)** — QtQuick Shell 框架，负责 Wayland 悬浮面板渲染。
- **[Papirus](https://github.com/PapirusDevelopmentTeam/papirus-icon-theme)** — 高质量 SVG 图标主题。
- **[tokio](https://tokio.rs)** — Rust 异步运行时。
- **[rusqlite](https://github.com/rusqlite/rusqlite)** — SQLite 绑定，用于读取 Firefox 数据库和使用历史。
- **[walkdir](https://github.com/BurntSushi/walkdir)** — 递归目录遍历，支撑文件搜索。
- **[nucleo](https://github.com/helix-editor/nucleo)** — 模糊匹配引擎，用于应用搜索。
- **[rustc-hash](https://github.com/rust-lang/rustc-hash)** — 快速非加密哈希，用于插件表和图标注销缓存。
- **[gio (gtk-rs)](https://gtk-rs.org/)** — GLib `GAppInfo` 应用注册表，用于应用发现与启动。
- **[fasteval](https://github.com/likebike/fasteval)** — 计算器表达式求值。
