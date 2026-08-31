# JSON-RPC 2.0

`qsflow-core` 后端还支持 [JSON-RPC 2.0](https://www.jsonrpc.org/specification)，
经由同一条 stdin/stdout。凡解析为含 `"jsonrpc":"2.0"` 与 `method` 的对象行，都会
作为 RPC 请求处理，并与启动器的文本协议混用。响应为 stdout 上以换行分隔的 JSON。

```sh
printf '%s\n' '{"jsonrpc":"2.0","method":"search","params":{"text":"firefox"},"id":1}' | qsflow-core
# -> {"jsonrpc":"2.0","result":[...],"id":1}
```

| 方法 | 参数 | 结果 |
|------|------|------|
| `search` | `{"text"}` | 结果项数组 |
| `top` | — | 最常用项 |
| `select` | 结果项对象 | `null`（记录使用） |
| `forget` | `{"on_click"}` | `null` |
| `run` | `{"cmd"}` | `null` |
| `list_plugins` | — | 插件元数据 |
| `theme` | — | 主题颜色 |
| `ping` | — | `"pong"` |

`search` 的 `params` 是带 `text` 键的对象（非空字符串）。缺省、空 `text`、裸字符串、
`{"query": …}`、非字符串 `text` 都会返回 `-32602`。最常用项请用 `top`——`search`
不承担默认视图。

无 `id` 的请求是通知（仅副作用，不返回响应）。未知方法返回 `-32601`；畸形请求
`-32600`；参数错误 `-32602`。

## 结果项

`search` 和 `top` 返回结果项数组。每个结果项是含以下键的对象——四个键**始终都在**，
缺省的可选字段为 `null`（而非省略）：

| 键 | 类型 | 含义 |
|-----|------|------|
| `title` | string | 主标签（应用名、命令、文件名……） |
| `summary` | string \| null | 副行（命令、路径、描述……） |
| `on_click` | string \| null | Enter 绑定的动作；见下方 scheme |
| `icon` | string \| null | 图标图像的绝对路径 |

`on_click` 的 scheme：

| Scheme | 效果 |
|--------|------|
| `run:<shell cmd>` | 执行 shell 命令（系统命令、剪贴板、复制） |
| `launch:<desktop-id>` | 按 desktop id 启动应用（app-search） |
| 裸 URL / `file:` / `mailto:` URI | 由 UI 经 `Qt.openUrlExternally` 打开 |

无 `on_click` 的结果项不可交互（仅展示）。
