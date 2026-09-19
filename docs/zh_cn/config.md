# 配置

`~/.config/wayrun/config.toml` 保存 core 的行为设置。首次运行时自动生成（即
`core/default-config.toml` 的副本），并且会被监听，修改后无需重启 core 即可生效。
所有键都是可选的：文件或键缺失即使用此处列出的内置默认值。

## `[web_search]`

| 键 | 默认 | 含义 |
| --- | --- | --- |
| `engine` | `google` | 建议来源：`google` 或 `duckduckgo`。未知值回退到 `google`。 |

## `[font]`

| 键 | 默认 | 含义 |
| --- | --- | --- |
| `family` | `Source Han Sans CN` | 主排版字体族，或通用名（`serif`、`sans-serif`、`monospace`）。 |

壳在启动时读取 `family`，因此修改后需下次启动生效
（`systemctl --user restart wayrun-launcher`）。缺字会逐字回退到系统字体，因此只覆盖
你常用文字的字体族，能让从不绘制的字体不占用内存。
