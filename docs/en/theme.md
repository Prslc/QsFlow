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

The first shell run writes `~/.config/wayrun/theme.toml` as a commented template.
Every key in it is commented out, so it lists the options inline without pinning
their current defaults; uncomment only what you want to change.

### `[colors]`

Per-field override of the system theme. Values are `#rrggbb` or `#rgb`; an
unparseable value is skipped and the system colour stays.

| Key | Default |
| --- | --- |
| `primary` | system theme |
| `fg` | system theme |
| `container` | system theme |
| `follow_system` | `false` |

Override and dynamic theming interact as follows. The overlay is per field: a
role you set stops following the system palette, while an unset role keeps
tracking it, so a matugen/DMS change still recolours the rest live. Setting
`follow_system = true` ignores the three colours above and follows the system
palette entirely, which is a convenient way to suspend overrides without
deleting them. Omitting the section is the same as a pure dynamic theme.

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
| `dim_color` | colour | `#000000` | `#rrggbb` | Backdrop dim colour. |
| `dim_alpha` | float | `0.30` | 0–1 | Backdrop dim under the card. |
| `card_alpha` | float | `0.72` | 0–1 | Card fill alpha. Lower is more see-through, so the frost reads stronger. |
| `field_alpha` | float | `0.08` | 0–1 | Search field fill over the card. |
| `selection_alpha` | float | `0.15` | 0–1 | Selected row's tint. |
| `hover_alpha` | float | `0.08` | 0–1 | Hovered row's tint. |
| `hairline_alpha` | float | `0.35` | 0–1 | Card border opacity. |
| `accent_alpha` | float | `1.0` | 0–1 | Selected row's accent bar. |
| `muted_alpha` | float | `0.55` | 0–1 | Placeholder, magnifier, ✕ and key hints. |
| `summary_alpha` | float | `0.7` | 0–1 | A row's summary line. |
| `footer_alpha` | float | `0.5` | 0–1 | Footer hint text. |

### `[layout]`

| Key | Type | Default | Range | Meaning |
| --- | --- | --- | --- | --- |
| `radius` | float | `16.0` | ≥ 0 | Card corner radius. The inner radii follow it down so nothing pokes outside. |
| `field_radius` | float | derived (9.0) | ≥ 0 | Field corner radius; defaults to the `radius`-derived 9 and is still capped by the card. |
| `row_radius` | float | derived (8.0) | ≥ 0 | Row tint radius. |
| `chip_radius` | float | derived (6.0) | ≥ 0 | Keyword chip radius. |
| `hairline_width` | float | `1.0` | 0–8 | Card border stroke width. |
| `accent_width` | float | `3.0` | 0–40 | Selected row's accent bar width. |
| `accent_height` | float | `28.0` | 0–200 | Selected row's accent bar height. |
| `width_ratio` | float | `0.38` | > 0 | Card width as a fraction of the output width. |
| `width_min` | float | `560.0` | > 0 | Lower clamp for the width. |
| `width_max` | float | `760.0` | > 0 | Upper clamp for the width. |
| `top_ratio` | float | `0.28` | 0–1 | Card top as a fraction of the output height. |
| `align` | string | `center` | `left`/`center`/`right` | Horizontal anchor on the output. |
| `offset_x` | float | `0.0` | any | Nudge after the width and anchor are resolved. |
| `offset_y` | float | `0.0` | any | Nudge after `top_ratio` is resolved. |
| `max_rows` | int | `5` | 1–8 | Visible result rows. |

If `width_min` ends up greater than `width_max`, it is pulled down to
`width_max`. An unknown `align` keeps the default.

### `[font]`

Text and icon sizes, in logical pixels. The shaping family is **not** here; it
lives in `config.toml`'s `[font].family` because the core owns text shaping.

| Key | Type | Default | Range | Meaning |
| --- | --- | --- | --- | --- |
| `query_size` | float | `18.0` | 1–96 | The query / placeholder. |
| `title_size` | float | `14.0` | 1–96 | A result row's title. |
| `summary_size` | float | `12.0` | 1–96 | A result row's summary. |
| `suggestion_size` | float | `11.0` | 1–96 | Footer, panel header and keyword chip. |
| `icon_size` | float | `30.0` | 1–256 | A result row's icon box. |
| `badge_size` | float | `15.0` | 1–256 | A row's status badge. |

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
follow_system = false

[blur]
enabled = true

[appearance]
dim_color = "#000000"
dim_alpha = 0.30
card_alpha = 0.72
field_alpha = 0.08
selection_alpha = 0.15
hover_alpha = 0.08
hairline_alpha = 0.35
accent_alpha = 1.0
muted_alpha = 0.55
summary_alpha = 0.7
footer_alpha = 0.5

[layout]
radius = 16.0
field_radius = 9.0
row_radius = 8.0
chip_radius = 6.0
hairline_width = 1.0
accent_width = 3.0
accent_height = 28.0
width_ratio = 0.38
width_min = 560.0
width_max = 760.0
top_ratio = 0.28
align = "center"
offset_x = 0.0
offset_y = 0.0
max_rows = 5

[font]
query_size = 18.0
title_size = 14.0
summary_size = 12.0
suggestion_size = 11.0
icon_size = 30.0
badge_size = 15.0

[motion]
entrance_ms = 240
reflow_ms = 150
reduced = false
```
