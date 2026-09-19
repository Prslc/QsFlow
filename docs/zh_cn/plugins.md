# 插件

`~/.config/wayrun/plugins.toml` 是插件注册表。首次运行时自动生成（即 `core/default-plugins.toml` 的副本），
并且会被监听，修改后无需重启 core 即可生效。

## 条目字段

```toml
[[plugins]]
id = "runner"
keyword = "r"
enable = true
```

| 字段 | 必填 | 含义 |
| --- | --- | --- |
| `id` | 是 | 该条目配置的插件。可以是内置 id，也可以是外部主机上报的 id。 |
| `keyword` | 是 | 路由到该插件的前缀。`""` 表示它是**默认**提供者。 |
| `enable` | 否 | 默认 `true`。设为 `false` 可禁用而不删除条目。 |
| `command` | 否 | 外部 JSON-RPC 2.0 主机，见下文。 |

调整条目顺序即可改变优先级。未识别或已删除的 id 会被忽略。内置 id 且未写 `command` 时使用编译进的插件；
未知 id 且未写 `command` 时跳过。

## 路由

只有某个插件拥有输入的第一个词作为 keyword 时，才会路由到该插件；否则整段输入（含第一个词）都作为
默认提供者的查询：`foo bar` 会以 `foo bar` 到达默认提供者，而不是 `bar`。

默认提供者（keyword 为 `""`）按条目顺序尝试，第一个返回非空结果者胜出，因此裸查询是应用/命令搜索，而非所有插件的并集。

## 内置插件

| Id | Keyword | 搜索内容 |
| --- | --- | --- |
| `calculator` | `""` | 行内算术。 |
| `system-commands` | `""` | `lock`、`reboot`、`shutdown`、`suspend`、`logout`。 |
| `app-search` | `""` | 已安装应用（desktop 条目）。 |
| `runner` | `r` | 模糊匹配 `$PATH` 可执行文件；可带参数。 |
| `firefox-bookmarks` | `b` | Firefox 书签。 |
| `firefox-history` | `h` | Firefox 历史。 |
| `web-search` | `s` | 网页搜索建议。 |
| `file-search` | `f` | 主目录下的文件。 |
| `path-search` | `d` | 目录与路径。 |
| `clipboard` | `c` | 剪贴板历史。 |
| `window` | `w` | niri 上的打开窗口。 |

## 外部主机

`command` 指向一个 JSON-RPC 2.0 主机。该值必须是单个可执行文件 token，按 `PATH` 解析或写绝对路径，
不含参数、无 shell 语法（脚本需 shebang + 执行位）。

core 每次查询都会重新拉起主机（一次请求后关闭其 stdin），并受 5s 超时约束，因此主机卡死只会消耗本次截止时间，
不会拖垮会话。主机经 `list_plugins` 上报的身份会按文件的 `(mtime, size)` 缓存，因此后续启动不会为未变的主机再次 fork。

身份 `icon` 与每条结果的 `icon` 都接受绝对路径、`papirus:` 规范或主题图标名；core 会在结果行到达壳之前把所有规范解析为绝对路径。

主机可以手写。[WayRun-Plugins](https://github.com/Prslc/WayRun-Plugins) 工作区提供了一套 Python 框架、
示例插件与 `template/` 模板，可复制起步。主机协议是 [jsonrpc.md](jsonrpc.md) 中记录的 JSON-RPC 子集：
`search`、`top`、`select`、`forget`、`list_plugins`。
