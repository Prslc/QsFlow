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
    spec.as_deref()?.trim().parse().ok()
}
