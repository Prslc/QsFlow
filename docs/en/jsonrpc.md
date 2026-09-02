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
| `resolve_icon` | `{"name"}` | absolute path for an icon spec |
| `list_plugins` | — | plugin metadata; see [schema](#plugin-metadata-list_plugins) |
| `theme` | — | theme colors |
| `ping` | — | `"pong"` |

`search` takes an object with a `text` key (a non-empty string). An absent
`params`, an empty `text`, a bare string, `{"query": …}`, or a non-string `text`
returns `-32602`. Use `top` for the most-used items — `search` does not serve a
default view.

A request without an `id` is a notification (side effect only, no response).
Unknown methods return `-32601`; malformed requests `-32600`; bad params
`-32602`.

`resolve_icon` resolves any icon spec — an absolute path, a theme icon name, or
the `papirus:` scheme below — to the absolute path the launcher UI renders. It
is meant for external plugin hosts that build icons dynamically (e.g. a
`list_plugins` identity) without hard-coding theme paths. An absent or
non-string `name` returns `-32602`.

## Plugin metadata (`list_plugins`)

`list_plugins` returns an array of plugin objects. There are two shapes:

### Core → client

Called on the core's stdin, it answers with the current registry:

| Key | Type | Meaning |
|-----|------|---------|
| `id` | string | plugin id, matches the `plugins.toml` entry |
| `name` | string | display name |
| `icon` | string | icon path or theme name (external hosts' `papirus:` specs are already resolved to absolute paths) |
| `keyword` | string | trigger prefix (empty = default) |
| `enabled` | bool | whether the plugin is active |

### External host → core (identity discovery)

When a `plugins.toml` entry declares `command`, the core spawns the host and
calls `list_plugins` once to discover identity. The response `result` is an
array of objects:

| Key | Type | Meaning |
|-----|------|---------|
| `id` | string (required) | plugin id — must match the `plugins.toml` entry id, or the identity is ignored |
| `name` | string | display name (empty → falls back to the configured id) |
| `icon` | string | absolute path or `papirus:` spec (see [Icon specs](#icon-specs)) |
| `description` | string | ready hint shown in the `?` list and the keyword+space hint |

## Default views for keyword plugins

A `plugins.toml` entry with a non-empty `keyword` is opened by a query that is
just the keyword followed by a space (e.g. `todo `) — through the text protocol
and the `search` RPC alike. Opening asks the external host for its **default
view**:

```sh
printf '%s\n' '{"jsonrpc":"2.0","method":"top","params":{"plugin":"todo"},"id":1}' | /path/to/todo/main.py
# -> {"jsonrpc":"2.0","result":[{"title":…,"summary":…,"on_click":…,"icon":…}],"id":1}
```

This `top` call is a core → host request — distinct from the core's own `top`
RPC (most-used usage history), which serves the empty-launcher view. A host
declares a default view by serving `top` (registering it via the plugin
framework's `@plugin.method("top")`); the response `result` is an array of
result items with the same [schema](#result-items) as `search`, icons
included.

When the host returns a non-empty default view it is shown instead of the
keyword+space identity hint. The hint stays otherwise:

- host without `top` (unknown method `-32601`) or a failing handler
  (`-32603`) → identity card from `list_plugins` (`name` + `description`)
- empty result list → same identity card, as the plugin's empty state

## Result items

`search` and `top` return an array of items. Every item is an object with these
keys — all four are always present (`null` for an absent optional field):

| Key | Type | Meaning |
|-----|------|---------|
| `title` | string | primary label (app name, command, file name, …) |
| `summary` | string \| null | secondary line (command, path, description, …) |
| `on_click` | string \| null | action bound to Enter; see the schemes below |
| `icon` | string \| null | absolute path to an icon image; see [Icon specs](#icon-specs) |

`on_click` schemes:

| Scheme | Effect |
|--------|--------|
| `run:<shell cmd>` | execute a shell command (system commands, clipboard) |
| `launch:<desktop-id>` | launch an app by desktop id (app-search) |
| `copy:{"text":"…"}` | write the text to the Wayland clipboard (translate copy) |
| bare URL / `file:` / `mailto:` URI | opened by the UI via `Qt.openUrlExternally` |

An item without `on_click` is non-interactive (display only).

### Icon specs

The launcher UI renders icons as `file://` + path, so every `icon` the core
emits is an absolute path. **External plugin hosts** (a `plugins.toml` entry
with `command`) may instead return a `papirus:` spec — in any result-item
`icon` field (`search` results and `top` default views alike) and in the
`list_plugins` identity `icon` — and the core resolves it before the item
reaches the UI:

| Spec | Resolution |
|------|------------|
| `papirus:<name>` | first match for `<name>` across Papirus categories and sizes |
| `papirus:<category>/<name>` | same, but searches `<category>` first |

Examples: `papirus:folder-open` →
`/usr/share/icons/Papirus/48x48/places/folder-open.svg`;
`papirus:apps/firefox` → `/usr/share/icons/Papirus/48x48/apps/firefox.svg`.
Installations rooted under `~/.local/share/icons` are searched as well.
