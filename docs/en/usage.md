# Usage

The overlay is one text field. Typing searches; the first word routes to a
plugin's keyword when one owns it, otherwise the whole input is an app/command
query (see [plugins.md](plugins.md)). Results are rows, and Enter acts on the
highlighted one.

## Prefixes

| Input | Action |
| --- | --- |
| `firefox` | fuzzy-search installed applications |
| `b <query>` | search Firefox bookmarks |
| `h <query>` | search Firefox history |
| `f <query>` | search files by name |
| `d <query>` | search files by path (multi-token fuzzy) |
| `r <query>` | fuzzy-search `$PATH` executables and run one |
| `w <query>` | switch focus to a matching open niri window |
| `c <query>` | search clipboard history (cliphist) |
| `s <query>` | Google suggestions |
| `?` | show keyword modes, default functions, and hints |
| `lock` / `reboot` / `shutdown` | system commands |
| `2 + 3` | inline calculator |
| _(empty)_ | show most-used items |

## Keys

| Key | Action |
| --- | --- |
| `Enter` | launch the selected result |
| `↑` / `↓` | move the selection |
| `PageUp` / `PageDown` | move by a page |
| `Home` / `End` | move the caret to the start/end of the query |
| `Ctrl+A` | select the whole query |
| `Shift+←/→` | extend the selection |
| `Ctrl+C` / `Ctrl+X` | copy the selection to the clipboard |
| `Ctrl+V` | paste |
| `Delete` | forget the selected history item |
| `Esc` | dismiss |
