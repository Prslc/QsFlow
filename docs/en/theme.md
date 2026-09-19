# Theme

How WayRun is coloured and what `~/.config/wayrun/theme.toml` controls.

Two layers, applied in order:

1. **System theme** — the core reads it and sends it to the shell.
2. **`theme.toml`** — the shell applies its overrides on top.

Both are watched. A change applies live; nothing restarts.

## System theme

On a DankMaterialShell desktop the core reads the Material You palette DMS
generated with matugen:

```
~/.cache/DankMaterialShell/dms-colors.json
```

The file holds both palettes at once plus the active `mode`, so switching
light/dark or changing the wallpaper is a plain file edit and is picked up live.

Roles are mapped like this:

| WayRun | DMS, from `colors[mode]` |
| --- | --- |
| `primary` | `primary` |
| `on_primary` | `on_primary` |
| `bg` | `background` |
| `fg` | `on_surface` |
| `container` | `surface_container_high` |

The shell currently draws with `primary`, `fg` and `container`; `bg` and
`on_primary` are carried for completeness.

Without DMS the built-in dark palette is used. Support for the freedesktop
appearance portal (`org.freedesktop.appearance`: `color-scheme` and
`accent-color`), which is what GTK/libadwaita reads on GNOME and KDE, is
planned.

## `~/.config/wayrun/theme.toml`

Every section and every key is optional. An absent key keeps the default, an
unparseable file is ignored, and out-of-range values are clamped. The file is
watched, so an edit applies without restarting.

### `[colors]`

Per-field override of the system theme. Values are `#rrggbb` or `#rgb`; an
unparseable value is skipped and the system colour stays.

| Key | Default |
| --- | --- |
| `primary` | system theme |
| `fg` | system theme |
| `container` | system theme |

### `[blur]`

The shell never blurs pixels itself. It draws a translucent card, and when a
compositor offers `ext-background-effect-v1` it hands that compositor the
card's rounded rectangle as a region to blur behind. The compositor does the
work; that is the frosted look. A compositor without the protocol ignores the
region, so this setting then has no visible effect.

On niri the effect needs one more thing. niri auto-enables **xray** for a
client's `ext-background-effect` request, which blurs the wallpaper and ignores
the windows below. For the card to blur the content actually behind it, add a
layer rule that turns xray off:

```kdl
layer-rule {
    match namespace="WayRun"
    background-effect {
        xray false
    }
}
```

The namespace is `WayRun`. niri recomputes non-xray blur whenever the content
underneath changes, so it is costlier than the default.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Declare the blur region. `false` declares nothing, so the backdrop stays sharp; the card's translucent fill is unchanged. |

### `[appearance]`

| Key | Type | Default | Range | Meaning |
| --- | --- | --- | --- | --- |
| `dim_alpha` | float | `0.30` | 0–1 | Backdrop dim under the card. |
| `card_alpha` | float | `0.72` | 0–1 | Card fill alpha. Lower is more see-through, so the frost reads stronger. |

### `[layout]`

| Key | Type | Default | Range | Meaning |
| --- | --- | --- | --- | --- |
| `radius` | float | `16.0` | ≥ 0 | Card corner radius. The inner radii follow it down so nothing pokes outside. |
| `width_ratio` | float | `0.38` | > 0 | Card width as a fraction of the output width. |
| `width_min` | float | `560.0` | > 0 | Lower clamp for the width. |
| `width_max` | float | `760.0` | > 0 | Upper clamp for the width. |
| `top_ratio` | float | `0.28` | 0–1 | Card top as a fraction of the output height. |
| `max_rows` | int | `5` | 1–8 | Visible result rows. |

If `width_min` ends up greater than `width_max`, it is pulled down to
`width_max`.

### `[motion]`

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `entrance_ms` | int | `240` | Opacity fade when the launcher is shown. |
| `reflow_ms` | int | `150` | Card height animation. |
| `reduced` | bool | `false` | Same as `WAYRUN_REDUCED_MOTION=1`; either one starts both animations at their end state and asks for no frames. |

## Example

```toml
# ~/.config/wayrun/theme.toml — every key is optional
[colors]
primary = "#7aa2f7"
fg = "#c0caf5"
container = "#24283b"

[blur]
enabled = true

[appearance]
dim_alpha = 0.30
card_alpha = 0.72

[layout]
radius = 16.0
width_ratio = 0.38
width_min = 560.0
width_max = 760.0
top_ratio = 0.28
max_rows = 5

[motion]
entrance_ms = 240
reflow_ms = 150
reduced = false
```
