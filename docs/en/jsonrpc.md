# JSON-RPC 2.0

The `qsflow-core` backend also speaks [JSON-RPC 2.0](https://www.jsonrpc.org/specification)
over the same stdin/stdout. Lines that parse to an object with
`"jsonrpc":"2.0"` and a `method` are handled as RPC requests and can be mixed
with the launcher's text protocol. Responses are newline-delimited JSON on
stdout.

```sh
printf '%s\n' '{"jsonrpc":"2.0","method":"search","params":{"text":"firefox"},"id":1}' | qsflow-core
# -> {"jsonrpc":"2.0","result":[...],"id":1}
```

| Method | Params | Result |
|--------|--------|--------|
| `search` | `{"text"}` | array of result items |
| `top` | — | most-used items |
| `select` | item object | `null` (records usage) |
| `forget` | `{"on_click"}` | `null` |
| `run` | `{"cmd"}` | `null` |
| `list_plugins` | — | plugin metadata |
| `theme` | — | theme colors |
| `ping` | — | `"pong"` |

`search` takes an object with a `text` key (a non-empty string). An absent
`params`, an empty `text`, a bare string, `{"query": …}`, or a non-string `text`
returns `-32602`. Use `top` for the most-used items — `search` does not serve a
default view.

A request without an `id` is a notification (side effect only, no response).
Unknown methods return `-32601`; malformed requests `-32600`; bad params
`-32602`.

## Result items

`search` and `top` return an array of items. Every item is an object with these
keys — all four are always present (`null` for an absent optional field):

| Key | Type | Meaning |
|-----|------|---------|
| `title` | string | primary label (app name, command, file name, …) |
| `summary` | string \| null | secondary line (command, path, description, …) |
| `on_click` | string \| null | action bound to Enter; see the schemes below |
| `icon` | string \| null | absolute path to an icon image |

`on_click` schemes:

| Scheme | Effect |
|--------|--------|
| `run:<shell cmd>` | execute a shell command (system commands, clipboard) |
| `launch:<desktop-id>` | launch an app by desktop id (app-search) |
| `copy:{"text":"…"}` | write the text to the Wayland clipboard (translate copy) |
| bare URL / `file:` / `mailto:` URI | opened by the UI via `Qt.openUrlExternally` |

An item without `on_click` is non-interactive (display only).
