use crate::session::model::ThemeConfig;

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
            primary: parse(&config.primary).unwrap_or(fallback.primary),
            fg: parse(&config.fg).unwrap_or(fallback.fg),
            container: parse(&config.container).unwrap_or(fallback.container),
        }
    }
}

/// `#rrggbb` (or `#rgb`), nothing else.
fn parse(spec: &Option<String>) -> Option<[u8; 3]> {
    let hex = spec.as_deref()?.trim().strip_prefix('#')?;
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
    use crate::session::model::ThemeConfig;

    #[test]
    fn parses_hex_and_falls_back() {
        let config = ThemeConfig {
            primary: Some("#7aa2f7".into()),
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
        assert!(parse(&Some("主色".into())).is_none());
        assert!(parse(&Some("#主色".into())).is_none());
        assert!(parse(&Some("🦀🦀🦀".into())).is_none());
        assert!(parse(&Some("#abc".into())).is_some());
    }
}
