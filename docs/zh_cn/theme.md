# 主题

WayRun 的配色方式，以及 `~/.config/wayrun/theme.toml` 控制的内容。

分两层，按顺序应用：

1. **系统主题** —— 由 core 读取后发给壳。
2. **`theme.toml`** —— 壳在其上叠加本地覆盖。

两者都会被监听，修改后实时生效，无需重启。

## 系统主题

在 DankMaterialShell 桌面上，core 读取 DMS 用 matugen 生成的 Material You 调色板：

```
~/.cache/DankMaterialShell/dms-colors.json
```

该文件同时保存亮/暗两套调色板与当前 `mode`，因此切换亮暗或更换壁纸只是一次文件写入，会被实时捕获。

角色映射：

| WayRun | DMS，取自 `colors[mode]` |
| --- | --- |
| `primary` | `primary` |
| `on_primary` | `on_primary` |
| `bg` | `background` |
| `fg` | `on_surface` |
| `container` | `surface_container_high` |

壳目前实际使用 `primary`、`fg`、`container`；`bg` 与 `on_primary` 一并透传以备后用。

没有 DMS 时使用内置深色调色板。面向 GNOME / KDE 的 freedesktop appearance portal
（`org.freedesktop.appearance` 的 `color-scheme` 与 `accent-color`，即 GTK/libadwaita 的方案）支持已在计划中。

## `~/.config/wayrun/theme.toml`

所有 section 与键均为可选。缺省键保留默认值，无法解析的文件被忽略，越界值会被 clamp。
文件被监听，修改后无需重启即可生效。

### `[colors]`

对系统主题做逐字段覆盖。值为 `#rrggbb` 或 `#rgb`；无法解析时跳过，保留系统颜色。

| 键 | 默认值 |
| --- | --- |
| `primary` | 系统主题 |
| `fg` | 系统主题 |
| `container` | 系统主题 |

### `[blur]`

壳本身不模糊任何像素。它只绘制一张半透明卡片；当合成器支持
`ext-background-effect-v1` 时，壳把卡片的圆角矩形作为区域交给合成器，由合成器模糊其背后的内容，
这就是磨砂观感的来源。合成器若没有该协议，会忽略该区域，此设置也就没有可见效果。

在 niri 上还需要额外一条规则。对于客户端通过 `ext-background-effect` 发起的请求，niri 会**默认自动开启 xray**，
即只模糊壁纸、忽略下方窗口。若要让卡片模糊其真正背后的内容，需要加一条关闭 xray 的 layer rule：

```kdl
layer-rule {
    match namespace="WayRun"
    background-effect {
        xray false
    }
}
```

其中 namespace 为 `WayRun`。非 xray 模糊会在其下方内容变化时重新计算，因此开销高于默认。

| 键 | 类型 | 默认值 | 含义 |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | 是否向合成器声明背景模糊区域。`false` 时不再声明，背景保持清晰；卡片的半透明填充不变。 |

### `[appearance]`

| 键 | 类型 | 默认值 | 范围 | 含义 |
| --- | --- | --- | --- | --- |
| `dim_alpha` | float | `0.30` | 0–1 | 卡片下方背景的暗化程度。 |
| `card_alpha` | float | `0.72` | 0–1 | 卡片填充透明度。越低越透，磨砂感越强。 |

### `[layout]`

| 键 | 类型 | 默认值 | 范围 | 含义 |
| --- | --- | --- | --- | --- |
| `radius` | float | `16.0` | ≥ 0 | 卡片圆角。内层圆角会随之收缩，避免越界。 |
| `width_ratio` | float | `0.38` | > 0 | 卡片宽度占输出宽度的比例。 |
| `width_min` | float | `560.0` | > 0 | 宽度下限。 |
| `width_max` | float | `760.0` | > 0 | 宽度上限。 |
| `top_ratio` | float | `0.28` | 0–1 | 卡片顶部占输出高度的比例。 |
| `max_rows` | int | `5` | 1–8 | 可见结果行数。 |

若解析后 `width_min` 大于 `width_max`，会把 `width_min` 拉到 `width_max`。

### `[motion]`

| 键 | 类型 | 默认值 | 含义 |
| --- | --- | --- | --- |
| `entrance_ms` | int | `240` | 显示时的透明度淡入时长。 |
| `reflow_ms` | int | `150` | 卡片高度动画时长。 |
| `reduced` | bool | `false` | 等价于 `WAYRUN_REDUCED_MOTION=1`；任一生效都会让两段动画直接停在终态且不请求帧。 |

## 示例

```toml
# ~/.config/wayrun/theme.toml —— 所有键均为可选
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
