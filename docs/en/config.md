# Config

`~/.config/wayrun/config.toml` holds core behaviour. It is generated on the
first run as a copy of `core/default-config.toml` and is watched, so an edit is
picked up without restarting the core. Every key is optional: a missing file or
key keeps the built-in default shown here. Out-of-range values are clamped.

## `[web_search]`

| Key | Default | Meaning |
| --- | --- | --- |
| `engine` | `google` | Suggestion backend: `google` or `duckduckgo`. An unknown value falls back to `google`. |
| `timeout_ms` | `5000` | Suggestion request deadline, rounded up to whole seconds (100–60000). |

## `[hosts]`

| Key | Default | Meaning |
| --- | --- | --- |
| `timeout_ms` | `5000` | Per-call deadline for an external JSON-RPC host (100–60000). |
| `discovery_concurrency` | `2` | Hosts forked at once while discovering their identities (1–8). |

## `[results]`

Rows kept per provider.

| Key | Default | Meaning |
| --- | --- | --- |
| `apps` | `50` | Application search. |
| `runner` | `20` | `$PATH` executables. |
| `windows` | `50` | Open windows. |

Each is clamped to 1–200.

## `[files]`

| Key | Default | Meaning |
| --- | --- | --- |
| `depth` | `3` | Walk depth under the home directory (1–6). |

## `[history]`

| Key | Default | Meaning |
| --- | --- | --- |
| `top` | `20` | Rows returned by the `top` RPC method (1–200). |

The empty-query history the launcher shows is never capped.

## `[font]`

| Key | Default | Meaning |
| --- | --- | --- |
| `family` | `Source Han Sans CN` | Primary shaping family, or a generic name (`serif`, `sans-serif`, `monospace`). |

The shell reads this at startup, so a change takes effect on the next launch
(`systemctl --user restart wayrun-launcher`). A non-CJK family avoids mapping a
CJK font when you never draw CJK text.
