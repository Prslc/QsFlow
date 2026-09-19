# JSON-RPC 2.0

`wayrun-core` 后端还支持 [JSON-RPC 2.0](https://www.jsonrpc.org/specification)，
经由同一条 stdin/stdout。凡解析为含 `"jsonrpc":"2.0"` 与 `method` 的对象行，都会
作为 RPC 请求处理，并与启动器的文本协议混用。响应为 stdout 上以换行分隔的 JSON。

```sh
printf '%s\n' '{"jsonrpc":"2.0","method":"search","params":{"text":"firefox"},"id":1}' | wayrun-core
# -> {"jsonrpc":"2.0","result":[...],"id":1}
```

| 方法 | 参数 | 结果 |
|------|------|------|
| `search` | `{"text"}` | 结果项数组 |
| `top` | — | 最常用项 |
| `select` | 结果项对象 | `null`（记录使用；`ephemeral` 与 `copy:` 行不记录） |
| `forget` | `{"on_click"}` | `{"forgotten": bool}` |
| `run` | `{"cmd"}` | `null` |
| `action` | `{"desktop_id","action_id"}` | `null`（运行 desktop 文件里的一个 `[Desktop Action …]` 组） |
| `launch` | `{"desktop_id"}` | `null`（经 GLib 的 `GAppInfo` 启动） |
| `open` | `{"uri"}` | `null`（用默认处理器打开） |
| `copy` | `{"text"}` | `null`（写入 Wayland 剪贴板） |
| `resolve_icon` | `{"name"}` | 图标规范对应的绝对路径 |
| `list_plugins` | — | 插件元数据；见 [schema](#插件元数据list_plugins) |
| `theme` | — | 主题颜色 |
| `ping` | — | `"pong"` |

`search` 的 `params` 是带 `text` 键的对象（非空字符串）。缺省、空 `text`、裸字符串、
`{"query": …}`、非字符串 `text` 都会返回 `-32602`。最常用项请用 `top`——`search`
不承担默认视图。

`forget` 把一行从使用历史中移除。当 `on_click` 是 `run:` shell 命令、且其首个
token 是某个已注册外部主机的 `command`（绝对路径；配置用裸名时按 PATH 解析），
core 还会向该主机转发一条 `forget` 请求，让主机删除自己的数据——例如 todo 插件
据此删除对应待办。返回值说明是否真的删掉了东西：删除了一条历史行、**或**拥有该行
的主机无错误地应答时为 `true`，否则为 `false`。内置提供者没有可删的数据；未实现
`forget` 方法的主机会回 `-32601`，这算「不是我的行」——UI 会把这类行留在列表里，
而不是声称完成了一次没人做过的删除。主机遍历在自己的任务里跑，慢的或卡住的主机
不会占住 stdin 循环，它的回复只是晚一点到，仍带着自己的 `id`。

无 `id` 的请求是通知（仅副作用，不返回响应）。未知方法返回 `-32601`；畸形请求
`-32600`；参数错误 `-32602`。

`resolve_icon` 把任意图标规范——绝对路径、主题图标名，或下文所述的 `papirus:`
scheme——解析为启动器 UI 渲染所用的绝对路径。它面向需要动态构造图标的外部
插件主机（如 `list_plugins` 身份图标），无需硬编码主题路径。`name` 缺失或非
字符串返回 `-32602`。

## 插件元数据（`list_plugins`）

`list_plugins` 返回插件对象数组，存在两种形态：

### core → 客户端

在 core 的 stdin 上调用，返回当前注册表：

| 键 | 类型 | 含义 |
|-----|------|------|
| `id` | string | 插件 id，与 `plugins.toml` 条目对应 |
| `name` | string | 显示名 |
| `icon` | string | 图标路径或主题名（外部主机的 `papirus:` 规范已解析为绝对路径） |
| `keyword` | string | 触发前缀（空 = 默认） |
| `enabled` | bool | 插件是否启用 |

### 外部主机 → core（身份发现）

当 `plugins.toml` 条目声明 `command` 时，core 拉起主机并调用一次 `list_plugins`
以发现身份。遵循此契约的 Python 框架与示例主机见
[WayRun-Plugins](https://github.com/Prslc/WayRun-Plugins) 工作区；响应 `result`
为对象数组：

| 键 | 类型 | 含义 |
|-----|------|------|
| `id` | string（必填） | 插件 id——必须与 `plugins.toml` 条目的 id 一致，否则身份被忽略 |
| `name` | string | 显示名（空 → 回落为配置的 id） |
| `icon` | string | 绝对路径或 `papirus:` 规范（见 [图标规范](#图标规范)） |
| `description` | string | ready 提示，显示在 `?` 列表与关键词+空格提示中 |

未实现 `list_plugins`（或返回中没有匹配的 `id`）的主机仍可用——搜索照常转发、
结果照常解析——但身份退化为配置的 id 且无图标，因此 `?` 列表与关键词+空格提示
显示默认占位符。声明身份（带 `papirus:` 图标）正是让这两处渲染真实图标的关键。

## 关键词插件的默认视图

`plugins.toml` 中带非空 `keyword` 的条目，由「关键词 + 空格」（如 `todo `）的查询
打开——文本协议与 `search` RPC 皆然。打开时 core 会向外部主机请求**默认视图**：

```sh
printf '%s\n' '{"jsonrpc":"2.0","method":"top","params":{"plugin":"todo"},"id":1}' | /path/to/todo/main.py
# -> {"jsonrpc":"2.0","result":[{"title":…,"summary":…,"on_click":…,"icon":…}],"id":1}
```

注意这是 core → 主机的请求，与 core 自身的 `top` RPC（最常用使用历史，服务启动器
空查询视图）不同。主机通过响应 `top` 声明默认视图（用插件框架的
`@plugin.method("top")` 注册）；响应 `result` 为结果项数组，与 `search`
同 [schema](#结果项)，含图标解析。

主机返回非空默认视图时，它取代关键词+空格的身份提示；否则维持原行为：

- 主机未实现 `top`（未知方法 `-32601`）或处理失败（`-32603`）→ 显示
  `list_plugins` 的身份卡片（`name` + `description`）
- 返回空结果列表 → 同一身份卡片，作为插件的空状态

## 结果项

`search` 和 `top` 返回结果项数组。每个结果项是含以下键的对象——五个键**始终都在**，
缺省的可选字段为 `null`（而非省略）：

| 键 | 类型 | 含义 |
|-----|------|------|
| `title` | string | 主标签（应用名、命令、文件名……） |
| `summary` | string \| null | 副行（命令、路径、描述……） |
| `on_click` | string \| null | Enter 绑定的动作；见下方 scheme |
| `icon` | string \| null | 图标图像的绝对路径；见 [图标规范](#图标规范) |
| `ephemeral` | bool | 为 true 时，选中该项不记入使用历史 |

`on_click` 的 scheme：

| Scheme | 效果 |
|--------|------|
| `run:<shell cmd>` | 执行 shell 命令（系统命令、剪贴板） |
| `launch:<desktop-id>` | 按 desktop id 启动应用（app-search） |
| `copy:{"text":"…"}` | 把文本写入 Wayland 剪贴板（翻译复制） |
| `action:<desktop-id>:<action-id>` | 运行 desktop action；由 UI 以 `action` 动词转交内核执行（app-search 按 DMS 的方式为每个 action 输出一行） |
| 裸 URL / `file:` / `mailto:` URI | 由 core 经 GLib `g_app_info_launch_default_for_uri` 打开 |

`file:` URI 会做百分号编码，路径中的空格与非 ASCII 字符都能保留；`Terminal=true`
的处理器会在终端中启动。

无 `on_click` 的结果项不可交互（仅展示）。

选择一项会把它记入使用历史（空查询时的 `top` 列表）。两类行会被豁免：主机标记
`ephemeral: true` 的行（比如一次性的搜索结果），以及 `on_click` 为 `copy:` 的行
（它的值是被复制的文本，而不是可再次打开的目标）。语义由字段或 scheme 声明，所以
内置 provider 与外部主机都适用。

### 图标规范

启动器 UI 以 `file://` + 路径渲染图标，因此 core 输出的每个 `icon` 都是绝对路径。
**外部插件主机**（`plugins.toml` 中带 `command` 的条目）可以返回主题图标名、
`papirus:` 规范或绝对路径——用在任何结果项 `icon` 字段（`search` 结果与 `top`
默认视图皆然）及 `list_plugins` 身份 `icon` 上——core 会在结果到达 UI 前解析：

| 规范 | 解析方式 |
|------|----------|
| 绝对路径 | 原样透传 |
| 主题图标名 | 依次在 `$XDG_DATA_HOME/icons`、各 `$XDG_DATA_DIRS/icons`、`~/.icons`、`/usr/share/pixmaps` 中查找，用户目录优先 |
| `papirus:<name>` | 在 Papirus 的类别与尺寸中查找 `<name>` 的首个匹配 |
| `papirus:<category>/<name>` | 同上，但优先搜索 `<category>` |

示例：`papirus:folder-open` →
`/usr/share/icons/Papirus/48x48/places/folder-open.svg`；
`papirus:apps/firefox` → `/usr/share/icons/Papirus/48x48/apps/firefox.svg`。
