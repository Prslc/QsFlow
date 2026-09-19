use crate::ui::geom::{Align, Layout};
use crate::ui::theme;

/// Assign the parsed key when present; an absent key keeps the current value.
macro_rules! assign {
    ($slot:expr, $value:expr) => {
        if let Some(value) = $value {
            $slot = value;
        }
    };
}

/// Assign a surface colour; unlike `assign!` an absent key clears the slot, so
/// the surface falls back to the role it derives from.
macro_rules! assign_color {
    ($slot:expr, $value:expr) => {
        $slot = $value.as_deref().and_then(theme::parse_color);
    };
}

/// The base roles are RGB; every surface takes a colour with inline alpha, so
/// opacity travels with the colour instead of its own key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorOverrides {
    pub primary: Option<[u8; 3]>,
    pub fg: Option<[u8; 3]>,
    pub container: Option<[u8; 3]>,
    pub card: Option<[u8; 4]>,
    pub field: Option<[u8; 4]>,
    pub selection: Option<[u8; 4]>,
    pub hover: Option<[u8; 4]>,
    pub hairline: Option<[u8; 4]>,
    pub muted: Option<[u8; 4]>,
    pub summary: Option<[u8; 4]>,
    pub footer: Option<[u8; 4]>,
    pub accent: Option<[u8; 4]>,
    pub dim: Option<[u8; 4]>,
    /// Ignore every override in this section and follow the system palette.
    pub follow_system: bool,
}

/// One interface size; every text and icon role keeps its shipped ratio to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontConfig {
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self { size: 14.0 }
    }
}

impl FontConfig {
    pub fn query(&self) -> f32 {
        self.size * 18.0 / 14.0
    }

    pub fn title(&self) -> f32 {
        self.size
    }

    pub fn summary(&self) -> f32 {
        self.size * 12.0 / 14.0
    }

    pub fn suggestion(&self) -> f32 {
        self.size * 11.0 / 14.0
    }

    pub fn icon(&self) -> f32 {
        self.size * 30.0 / 14.0
    }

    pub fn badge(&self) -> f32 {
        self.size * 15.0 / 14.0
    }
}

/// The shell's appearance, loaded from `theme.toml`. Every default equals the
/// renderer's constant, so an unset field changes nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceConfig {
    pub colors: ColorOverrides,
    pub blur: bool,
    pub font: FontConfig,
    pub layout: Layout,
    pub entrance_ms: u64,
    pub reflow_ms: u64,
    pub reduced: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            colors: ColorOverrides::default(),
            blur: true,
            font: FontConfig::default(),
            layout: Layout::default(),
            entrance_ms: 240,
            reflow_ms: 150,
            reduced: false,
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct ThemeFile {
    #[serde(default)]
    colors: ColorsFile,
    #[serde(default)]
    blur: BlurFile,
    #[serde(default)]
    layout: LayoutFile,
    #[serde(default)]
    font: FontFile,
    #[serde(default)]
    motion: MotionFile,
}

#[derive(serde::Deserialize, Default)]
struct ColorsFile {
    primary: Option<String>,
    fg: Option<String>,
    container: Option<String>,
    card: Option<String>,
    field: Option<String>,
    selection: Option<String>,
    hover: Option<String>,
    hairline: Option<String>,
    muted: Option<String>,
    summary: Option<String>,
    footer: Option<String>,
    accent: Option<String>,
    dim: Option<String>,
    follow_system: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
struct BlurFile {
    enabled: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
struct LayoutFile {
    radius: Option<f32>,
    field_radius: Option<f32>,
    row_radius: Option<f32>,
    chip_radius: Option<f32>,
    width_ratio: Option<f32>,
    width_min: Option<f32>,
    width_max: Option<f32>,
    top_ratio: Option<f32>,
    align: Option<String>,
    offset_x: Option<f32>,
    offset_y: Option<f32>,
    hairline_width: Option<f32>,
    accent_width: Option<f32>,
    accent_height: Option<f32>,
    max_rows: Option<usize>,
}

#[derive(serde::Deserialize, Default)]
struct FontFile {
    size: Option<f32>,
}

#[derive(serde::Deserialize, Default)]
struct MotionFile {
    entrance_ms: Option<u64>,
    reflow_ms: Option<u64>,
    reduced: Option<bool>,
}

impl AppearanceConfig {
    pub fn load() -> Self {
        super::write_template();
        let mut config = Self::default();
        let Some(path) = super::theme_path() else {
            return config;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return config;
        };
        let Ok(file) = toml::from_str::<ThemeFile>(&text) else {
            return config;
        };
        config.apply(file);
        config
    }

    fn apply(&mut self, file: ThemeFile) {
        let ThemeFile {
            colors,
            blur,
            layout,
            font,
            motion,
        } = file;

        self.colors.primary = colors.primary.as_deref().and_then(theme::parse_hex);
        self.colors.fg = colors.fg.as_deref().and_then(theme::parse_hex);
        self.colors.container = colors.container.as_deref().and_then(theme::parse_hex);
        assign_color!(self.colors.card, colors.card);
        assign_color!(self.colors.field, colors.field);
        assign_color!(self.colors.selection, colors.selection);
        assign_color!(self.colors.hover, colors.hover);
        assign_color!(self.colors.hairline, colors.hairline);
        assign_color!(self.colors.muted, colors.muted);
        assign_color!(self.colors.summary, colors.summary);
        assign_color!(self.colors.footer, colors.footer);
        assign_color!(self.colors.accent, colors.accent);
        assign_color!(self.colors.dim, colors.dim);
        assign!(self.colors.follow_system, colors.follow_system);

        assign!(self.blur, blur.enabled);
        assign!(self.font.size, font.size.and_then(|v| positive(v, 96.0)));

        assign!(
            self.layout.radius,
            layout.radius.filter(|v| v.is_finite()).map(|v| v.max(0.0))
        );
        self.layout.field_radius = non_negative(layout.field_radius);
        self.layout.row_radius = non_negative(layout.row_radius);
        self.layout.chip_radius = non_negative(layout.chip_radius);
        assign!(
            self.layout.width_ratio,
            layout.width_ratio.filter(|v| v.is_finite() && *v > 0.0)
        );
        assign!(
            self.layout.width_min,
            layout.width_min.filter(|v| v.is_finite() && *v > 0.0)
        );
        assign!(
            self.layout.width_max,
            layout.width_max.filter(|v| v.is_finite() && *v > 0.0)
        );
        assign!(self.layout.top_ratio, layout.top_ratio.and_then(clamp01));
        assign!(
            self.layout.align,
            layout.align.as_deref().and_then(parse_align)
        );
        assign!(self.layout.offset_x, finite(layout.offset_x));
        assign!(self.layout.offset_y, finite(layout.offset_y));
        assign!(
            self.layout.hairline_width,
            finite(layout.hairline_width).map(|v| v.clamp(0.0, 8.0))
        );
        assign!(
            self.layout.accent_width,
            finite(layout.accent_width).map(|v| v.clamp(0.0, 40.0))
        );
        assign!(
            self.layout.accent_height,
            finite(layout.accent_height).map(|v| v.clamp(0.0, 200.0))
        );
        assign!(self.layout.max_rows, layout.max_rows.map(|v| v.clamp(1, 8)));

        assign!(self.entrance_ms, motion.entrance_ms.filter(|v| *v > 0));
        assign!(self.reflow_ms, motion.reflow_ms.filter(|v| *v > 0));
        assign!(self.reduced, motion.reduced);

        if self.layout.width_min > self.layout.width_max {
            self.layout.width_min = self.layout.width_max;
        }
    }
}

fn parse_align(value: &str) -> Option<Align> {
    match value {
        "left" => Some(Align::Left),
        "center" => Some(Align::Center),
        "right" => Some(Align::Right),
        _ => None,
    }
}

fn finite(v: Option<f32>) -> Option<f32> {
    v.filter(|v| v.is_finite())
}

/// A finite, non-negative value, else `None` so the base is kept.
fn non_negative(v: Option<f32>) -> Option<f32> {
    v.filter(|v| v.is_finite()).map(|v| v.max(0.0))
}

/// A finite, positive value clamped into `(0, max]`, else `None`.
fn positive(v: f32, max: f32) -> Option<f32> {
    (v.is_finite() && v > 0.0).then(|| v.clamp(1.0, max))
}

fn clamp01(v: f32) -> Option<f32> {
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> AppearanceConfig {
        let mut config = AppearanceConfig::default();
        config.apply(toml::from_str(src).unwrap());
        config
    }

    #[test]
    fn absent_keys_keep_the_defaults() {
        assert_eq!(parse(""), AppearanceConfig::default());
    }

    #[test]
    fn present_keys_override_field_by_field() {
        let config = parse(
            r##"
            [colors]
            primary = "#7aa2f7"
            fg = "nonsense"

            [blur]
            enabled = false

            [layout]
            radius = 20.0
            max_rows = 3

            [motion]
            reduced = true
            "##,
        );
        assert_eq!(config.colors.primary, Some([0x7a, 0xa2, 0xf7]));
        assert_eq!(config.colors.fg, None);
        assert!(!config.blur);
        assert_eq!(config.layout.radius, 20.0);
        assert_eq!(config.layout.max_rows, 3);
        assert!(config.reduced);
        // untouched defaults survive
        assert_eq!(config.font.size, 14.0);
        assert_eq!(config.layout.width_ratio, 0.38);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let config = parse(
            r#"
            [font]
            size = 500.0

            [layout]
            max_rows = 99

            [motion]
            entrance_ms = 0
            "#,
        );
        assert_eq!(config.font.size, 96.0);
        assert_eq!(config.layout.max_rows, 8);
        assert_eq!(config.entrance_ms, 240);
    }

    #[test]
    fn a_surface_colour_carries_its_own_alpha() {
        let config = parse(
            r##"
            [colors]
            card = "#11223380"
            muted = "#445566"
            dim = "#000000"
            accent = "nonsense"
            follow_system = false
            "##,
        );
        assert_eq!(config.colors.card, Some([0x11, 0x22, 0x33, 0x80]));
        assert_eq!(config.colors.muted, Some([0x44, 0x55, 0x66, 255]));
        assert_eq!(config.colors.dim, Some([0, 0, 0, 255]));
        assert_eq!(config.colors.accent, None);
    }

    #[test]
    fn follow_system_is_a_plain_flag() {
        assert!(parse("[colors]\nfollow_system = true").colors.follow_system);
    }

    #[test]
    fn one_size_scales_every_role_by_its_shipped_ratio() {
        let config = parse("[font]\nsize = 28.0");
        assert_eq!(config.font.query(), 36.0);
        assert_eq!(config.font.title(), 28.0);
        assert_eq!(config.font.summary(), 24.0);
        assert_eq!(config.font.suggestion(), 22.0);
        assert_eq!(config.font.icon(), 60.0);
        assert_eq!(config.font.badge(), 30.0);

        // zero and negative keep the default
        assert_eq!(parse("[font]\nsize = -3.0").font.size, 14.0);
        assert_eq!(parse("[font]\nsize = 0.0").font.size, 14.0);
    }

    #[test]
    fn align_offsets_and_stroke_sizes_apply() {
        let config = parse(
            r#"
            [layout]
            align = "left"
            offset_x = 12.0
            offset_y = -8.0
            field_radius = 4.0
            row_radius = 3.0
            chip_radius = 2.0
            hairline_width = 2.0
            accent_width = 5.0
            accent_height = 20.0
            "#,
        );
        assert_eq!(config.layout.align, Align::Left);
        assert_eq!(config.layout.offset_x, 12.0);
        assert_eq!(config.layout.offset_y, -8.0);
        assert_eq!(config.layout.field_radius, Some(4.0));
        assert_eq!(config.layout.row_radius, Some(3.0));
        assert_eq!(config.layout.chip_radius, Some(2.0));
        assert_eq!(config.layout.hairline_width, 2.0);
        assert_eq!(config.layout.accent_width, 5.0);
        assert_eq!(config.layout.accent_height, 20.0);

        let unknown = parse("[layout]\nalign = \"diagonal\"");
        assert_eq!(unknown.layout.align, Align::Center);
    }

    #[test]
    fn the_shipped_template_comments_out_every_key() {
        for line in super::super::DEFAULT_TEMPLATE.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            assert!(line.starts_with('['), "uncommented key: {line}");
        }

        // commented keys are absent, so the whole file is the built-in defaults
        let mut config = AppearanceConfig::default();
        config.apply(toml::from_str(super::super::DEFAULT_TEMPLATE).unwrap());
        assert_eq!(config, AppearanceConfig::default());
    }
}
