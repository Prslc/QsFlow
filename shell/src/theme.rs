//! Theme colors, parsed from the core's hex payload with the QML's dark
//! fallbacks: primary #7aa2f7, fg #c0caf5, container #24283b.

use iced::Color;

use crate::model::ThemeConfig;

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub primary: Color,
    pub fg: Color,
    pub container: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            primary: Color::from_rgb8(0x7a, 0xa2, 0xf7),
            fg: Color::from_rgb8(0xc0, 0xca, 0xf5),
            container: Color::from_rgb8(0x24, 0x28, 0x3b),
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

fn parse(spec: &Option<String>) -> Option<Color> {
    let hex = spec.as_deref()?.trim();
    // iced's `Color::FromStr` slices the value by byte index, so a multi-byte
    // character panics inside the parse (which `.ok()` cannot catch). The value
    // is arbitrary user data from dank-colors.css.
    if !hex.is_ascii() {
        return None;
    }

    hex.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_falls_back() {
        let config = ThemeConfig {
            primary: Some("#7aa2f7".into()),
            fg: Some("nonsense".into()),
            container: None,
        };
        let theme = Theme::from_config(&config);
        assert_eq!(theme.primary, "#7aa2f7".parse::<Color>().unwrap());
        assert_eq!(theme.fg, Theme::default().fg);
        assert_eq!(theme.container, Theme::default().container);
    }

    /// `Color`'s `FromStr` slices the input by byte index, so a multi-byte value
    /// panics inside the parse — `.ok()` cannot catch that.
    #[test]
    fn non_ascii_values_do_not_panic() {
        assert!(parse(&Some("主色".into())).is_none());
        assert!(parse(&Some("#主色".into())).is_none());
        assert!(parse(&Some("🦀🦀🦀".into())).is_none());
    }
}
