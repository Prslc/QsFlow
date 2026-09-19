# Config

`~/.config/wayrun/config.toml` holds core behaviour. It is generated on the
first run as a copy of `core/default-config.toml` and is watched, so an edit is
picked up without restarting the core. Every key is optional: a missing file or
key keeps the built-in default shown here.

## `[web_search]`

| Key | Default | Meaning |
| --- | --- | --- |
| `engine` | `google` | Suggestion backend: `google` or `duckduckgo`. An unknown value falls back to `google`. |

## `[font]`

| Key | Default | Meaning |
| --- | --- | --- |
| `family` | `Source Han Sans CN` | Primary shaping family, or a generic name (`serif`, `sans-serif`, `monospace`). |

The shell reads `family` at startup, so a change takes effect on the next launch
(`systemctl --user restart wayrun-launcher`). A non-CJK family avoids mapping a
CJK font when you never draw CJK text.
