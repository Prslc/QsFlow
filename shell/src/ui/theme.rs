use crate::config::ColorOverrides;
use wayrun_core::wire::ThemeConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub primary: [u8; 3],
    pub fg: [u8; 3],
    pub container: [u8; 3],
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            primary: [0x7a, 0xa2, 0xf7],
            fg: [0xc0, 0xca, 0xf5],
            container: [0x24, 0x28, 0x3b],
        }
    }
}

impl Theme {
    /// Field-by-field: a missing or unparseable entry keeps the fallback.
    pub fn from_config(config: &ThemeConfig) -> Self {
        let fallback = Self::default();

        Self {
            primary: config
                .primary
                .as_deref()
                .and_then(parse_hex)
                .unwrap_or(fallback.primary),
            fg: config
                .fg
                .as_deref()
                .and_then(parse_hex)
                .unwrap_or(fallback.fg),
            container: config
                .container
                .as_deref()
                .and_then(parse_hex)
                .unwrap_or(fallback.container),
        }
    }

    /// The system theme with `theme.toml`'s per-field overrides on top.
    pub fn overlay(system: Self, colors: &ColorOverrides) -> Self {
        Self {
            primary: colors.primary.unwrap_or(system.primary),
            fg: colors.fg.unwrap_or(system.fg),
            container: colors.container.unwrap_or(system.container),
        }
    }
}

/// `#rrggbb` (or `#rgb`), nothing else.
pub fn parse_hex(spec: &str) -> Option<[u8; 3]> {
    let hex = spec.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }

    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    match hex.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?]),
        3 => {
            let one = |at: usize| {
                u8::from_str_radix(&hex[at..at + 1], 16)
                    .ok()
                    .map(|v| v * 17)
            };
            Some([one(0)?, one(1)?, one(2)?])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wayrun_core::wire::ThemeConfig;

    #[test]
    fn parses_hex_and_falls_back() {
        let config = ThemeConfig {
            primary: Some("#7aa2f7".into()),
            on_primary: None,
            bg: None,
            fg: Some("nonsense".into()),
            container: None,
        };
        let theme = Theme::from_config(&config);
        assert_eq!(theme.primary, [0x7a, 0xa2, 0xf7]);
        assert_eq!(theme.fg, Theme::default().fg);
        assert_eq!(theme.container, Theme::default().container);
    }

    #[test]
    fn non_ascii_values_do_not_panic() {
        assert!(parse_hex("主色").is_none());
        assert!(parse_hex("#主色").is_none());
        assert!(parse_hex("🦀🦀🦀").is_none());
        assert!(parse_hex("#abc").is_some());
    }

    #[test]
    fn overrides_win_field_by_field() {
        let system = Theme {
            primary: [1, 2, 3],
            fg: [4, 5, 6],
            container: [7, 8, 9],
        };
        let colors = ColorOverrides {
            primary: Some([0xaa, 0xbb, 0xcc]),
            fg: None,
            container: Some([0x11, 0x22, 0x33]),
        };
        let theme = Theme::overlay(system, &colors);
        assert_eq!(theme.primary, [0xaa, 0xbb, 0xcc]);
        assert_eq!(theme.fg, [4, 5, 6]);
        assert_eq!(theme.container, [0x11, 0x22, 0x33]);
    }
}
