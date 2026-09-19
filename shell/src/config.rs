use std::path::PathBuf;
use std::time::{Duration, Instant};

use wayrun_core::watch_targets;

use crate::ui::geom::{Align, Layout};
use crate::ui::theme;

/// `~/.config/wayrun/theme.toml`. A missing file (or any missing key) keeps the
/// default below, so an absent config is the shipped look.
pub fn theme_path() -> Option<PathBuf> {
    Some(wayrun_core::config::dir()?.join("theme.toml"))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorOverrides {
    pub primary: Option<[u8; 3]>,
    pub fg: Option<[u8; 3]>,
    pub container: Option<[u8; 3]>,
    /// Ignore every override in this section and follow the system palette.
    pub follow_system: bool,
}

/// The UI's text and icon sizes, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontConfig {
    pub query_size: f32,
    pub title_size: f32,
    pub summary_size: f32,
    pub suggestion_size: f32,
    pub icon_size: f32,
    pub badge_size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            query_size: 18.0,
            title_size: 14.0,
            summary_size: 12.0,
            suggestion_size: 11.0,
            icon_size: 30.0,
            badge_size: 15.0,
        }
    }
}

/// The shell's appearance, loaded from `theme.toml`. Every default equals the
/// renderer's constant, so an unset field changes nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceConfig {
    pub colors: ColorOverrides,
    pub blur: bool,
    pub dim_color: [u8; 3],
    pub dim_alpha: f32,
    pub card_alpha: f32,
    pub field_alpha: f32,
    pub selection_alpha: f32,
    pub hover_alpha: f32,
    pub hairline_alpha: f32,
    pub muted_alpha: f32,
    pub summary_alpha: f32,
    pub footer_alpha: f32,
    pub accent_alpha: f32,
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
            dim_color: [0, 0, 0],
            dim_alpha: 0.30,
            card_alpha: 0.72,
            field_alpha: 0.08,
            selection_alpha: 0.15,
            hover_alpha: 0.08,
            hairline_alpha: 0.35,
            muted_alpha: 0.55,
            summary_alpha: 0.7,
            footer_alpha: 0.5,
            accent_alpha: 1.0,
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
    appearance: AppearanceFile,
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
    follow_system: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
struct BlurFile {
    enabled: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
struct AppearanceFile {
    dim_color: Option<String>,
    dim_alpha: Option<f32>,
    card_alpha: Option<f32>,
    field_alpha: Option<f32>,
    selection_alpha: Option<f32>,
    hover_alpha: Option<f32>,
    hairline_alpha: Option<f32>,
    muted_alpha: Option<f32>,
    summary_alpha: Option<f32>,
    footer_alpha: Option<f32>,
    accent_alpha: Option<f32>,
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
    query_size: Option<f32>,
    title_size: Option<f32>,
    summary_size: Option<f32>,
    suggestion_size: Option<f32>,
    icon_size: Option<f32>,
    badge_size: Option<f32>,
}

#[derive(serde::Deserialize, Default)]
struct MotionFile {
    entrance_ms: Option<u64>,
    reflow_ms: Option<u64>,
    reduced: Option<bool>,
}

impl AppearanceConfig {
    pub fn load() -> Self {
        let mut config = Self::default();
        let Some(path) = theme_path() else {
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
        if let Some(c) = file.colors.primary.as_deref().and_then(theme::parse_hex) {
            self.colors.primary = Some(c);
        }
        if let Some(c) = file.colors.fg.as_deref().and_then(theme::parse_hex) {
            self.colors.fg = Some(c);
        }
        if let Some(c) = file.colors.container.as_deref().and_then(theme::parse_hex) {
            self.colors.container = Some(c);
        }
        if let Some(v) = file.colors.follow_system {
            self.colors.follow_system = v;
        }
        if let Some(v) = file.blur.enabled {
            self.blur = v;
        }
        if let Some(c) = file
            .appearance
            .dim_color
            .as_deref()
            .and_then(theme::parse_hex)
        {
            self.dim_color = c;
        }
        if let Some(v) = file.appearance.dim_alpha.and_then(clamp01) {
            self.dim_alpha = v;
        }
        if let Some(v) = file.appearance.card_alpha.and_then(clamp01) {
            self.card_alpha = v;
        }
        if let Some(v) = file.appearance.field_alpha.and_then(clamp01) {
            self.field_alpha = v;
        }
        if let Some(v) = file.appearance.selection_alpha.and_then(clamp01) {
            self.selection_alpha = v;
        }
        if let Some(v) = file.appearance.hover_alpha.and_then(clamp01) {
            self.hover_alpha = v;
        }
        if let Some(v) = file.appearance.hairline_alpha.and_then(clamp01) {
            self.hairline_alpha = v;
        }
        if let Some(v) = file.appearance.muted_alpha.and_then(clamp01) {
            self.muted_alpha = v;
        }
        if let Some(v) = file.appearance.summary_alpha.and_then(clamp01) {
            self.summary_alpha = v;
        }
        if let Some(v) = file.appearance.footer_alpha.and_then(clamp01) {
            self.footer_alpha = v;
        }
        if let Some(v) = file.appearance.accent_alpha.and_then(clamp01) {
            self.accent_alpha = v;
        }
        if let Some(v) = file.font.query_size.and_then(|v| font_size(v, 96.0)) {
            self.font.query_size = v;
        }
        if let Some(v) = file.font.title_size.and_then(|v| font_size(v, 96.0)) {
            self.font.title_size = v;
        }
        if let Some(v) = file.font.summary_size.and_then(|v| font_size(v, 96.0)) {
            self.font.summary_size = v;
        }
        if let Some(v) = file.font.suggestion_size.and_then(|v| font_size(v, 96.0)) {
            self.font.suggestion_size = v;
        }
        if let Some(v) = file.font.icon_size.and_then(|v| font_size(v, 256.0)) {
            self.font.icon_size = v;
        }
        if let Some(v) = file.font.badge_size.and_then(|v| font_size(v, 256.0)) {
            self.font.badge_size = v;
        }
        if let Some(v) = file.layout.radius.filter(|v| v.is_finite()) {
            self.layout.radius = v.max(0.0);
        }
        self.layout.field_radius = non_negative(file.layout.field_radius);
        self.layout.row_radius = non_negative(file.layout.row_radius);
        self.layout.chip_radius = non_negative(file.layout.chip_radius);
        if let Some(v) = file
            .layout
            .width_ratio
            .filter(|v| v.is_finite() && *v > 0.0)
        {
            self.layout.width_ratio = v;
        }
        if let Some(v) = file.layout.width_min.filter(|v| v.is_finite() && *v > 0.0) {
            self.layout.width_min = v;
        }
        if let Some(v) = file.layout.width_max.filter(|v| v.is_finite() && *v > 0.0) {
            self.layout.width_max = v;
        }
        if let Some(v) = file.layout.top_ratio.and_then(clamp01) {
            self.layout.top_ratio = v;
        }
        if let Some(a) = file.layout.align.as_deref() {
            match a {
                "left" => self.layout.align = Align::Left,
                "right" => self.layout.align = Align::Right,
                "center" => self.layout.align = Align::Center,
                _ => {}
            }
        }
        if let Some(v) = file.layout.offset_x.filter(|v| v.is_finite()) {
            self.layout.offset_x = v;
        }
        if let Some(v) = file.layout.offset_y.filter(|v| v.is_finite()) {
            self.layout.offset_y = v;
        }
        if let Some(v) = file.layout.hairline_width.filter(|v| v.is_finite()) {
            self.layout.hairline_width = v.clamp(0.0, 8.0);
        }
        if let Some(v) = file.layout.accent_width.filter(|v| v.is_finite()) {
            self.layout.accent_width = v.clamp(0.0, 40.0);
        }
        if let Some(v) = file.layout.accent_height.filter(|v| v.is_finite()) {
            self.layout.accent_height = v.clamp(0.0, 200.0);
        }
        if let Some(v) = file.layout.max_rows {
            self.layout.max_rows = v.clamp(1, 8);
        }
        if let Some(v) = file.motion.entrance_ms.filter(|v| *v > 0) {
            self.entrance_ms = v;
        }
        if let Some(v) = file.motion.reflow_ms.filter(|v| *v > 0) {
            self.reflow_ms = v;
        }
        if let Some(v) = file.motion.reduced {
            self.reduced = v;
        }
        if self.layout.width_min > self.layout.width_max {
            self.layout.width_min = self.layout.width_max;
        }
    }
}

/// A finite, non-negative geometry value, else `None` so the base is kept.
fn non_negative(v: Option<f32>) -> Option<f32> {
    v.filter(|v| v.is_finite()).map(|v| v.max(0.0))
}

/// A finite, positive size, clamped into `(0, max]` so an absurd value cannot
/// allocate a huge glyph atlas.
fn font_size(v: f32, max: f32) -> Option<f32> {
    (v.is_finite() && v > 0.0).then(|| v.clamp(1.0, max))
}

fn clamp01(v: f32) -> Option<f32> {
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

/// A config reload is cheap, so it is debounced shorter than the core's plugin
/// reload.
const CONFIG_DEBOUNCE: Duration = Duration::from_millis(300);

/// Watch `theme.toml` and hand each parsed config to the shell's event loop.
/// Debounced: editors typically fire several events per save.
pub fn watch(tx: calloop::channel::Sender<AppearanceConfig>) -> Option<notify::RecommendedWatcher> {
    let path = theme_path()?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let watch_path = path.clone();
    let now = Instant::now();
    let mut last = now.checked_sub(Duration::from_secs(1)).unwrap_or(now);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // React to writes, not reads: `load` reads this file, so an unfiltered
        // Access event would re-trigger the watcher forever.
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        if !ev.paths.iter().any(|p| p == &watch_path) {
            return;
        }
        if last.elapsed() < CONFIG_DEBOUNCE {
            return;
        }
        last = Instant::now();
        let _ = tx.send(AppearanceConfig::load());
    })
    .ok()?;

    watch_targets(&mut watcher, &path);
    Some(watcher)
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

            [appearance]
            dim_alpha = 0.5

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
        assert_eq!(config.dim_alpha, 0.5);
        assert_eq!(config.layout.radius, 20.0);
        assert_eq!(config.layout.max_rows, 3);
        assert!(config.reduced);
        // untouched defaults survive
        assert_eq!(config.card_alpha, 0.72);
        assert_eq!(config.layout.width_ratio, 0.38);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let config = parse(
            r#"
            [appearance]
            dim_alpha = 9.0

            [layout]
            max_rows = 99

            [motion]
            entrance_ms = 0
            "#,
        );
        assert_eq!(config.dim_alpha, 1.0);
        assert_eq!(config.layout.max_rows, 8);
        assert_eq!(config.entrance_ms, 240);
    }

    #[test]
    fn an_inverted_width_range_collapses_to_the_max() {
        let config = parse(
            r#"
            [layout]
            width_min = 900.0
            width_max = 700.0
            "#,
        );
        assert_eq!(config.layout.width_min, 700.0);
    }

    #[test]
    fn dim_color_and_the_alpha_roles_apply() {
        let config = parse(
            r##"
            [appearance]
            dim_color = "#102030"
            field_alpha = 0.2
            selection_alpha = 0.3
            hover_alpha = 0.1
            hairline_alpha = 0.4
            muted_alpha = 0.6
            summary_alpha = 0.8
            footer_alpha = 0.7
            accent_alpha = 0.9

            [colors]
            follow_system = true
            "##,
        );
        assert_eq!(config.dim_color, [0x10, 0x20, 0x30]);
        assert_eq!(config.field_alpha, 0.2);
        assert_eq!(config.selection_alpha, 0.3);
        assert_eq!(config.hover_alpha, 0.1);
        assert_eq!(config.hairline_alpha, 0.4);
        assert_eq!(config.muted_alpha, 0.6);
        assert_eq!(config.summary_alpha, 0.8);
        assert_eq!(config.footer_alpha, 0.7);
        assert_eq!(config.accent_alpha, 0.9);
        assert!(config.colors.follow_system);
        // an unparseable dim color keeps the default
        assert_eq!(
            parse("[appearance]\ndim_color = \"nope\"").dim_color,
            [0; 3]
        );
    }

    #[test]
    fn font_sizes_stay_positive_and_bounded() {
        let config = parse(
            r#"
            [font]
            query_size = 0.0
            title_size = -3.0
            summary_size = 500.0
            suggestion_size = 10.5
            icon_size = 48.0
            badge_size = 1.0
            "#,
        );
        // zero and negative keep the default; 500 clamps to the cap
        assert_eq!(config.font.query_size, 18.0);
        assert_eq!(config.font.title_size, 14.0);
        assert_eq!(config.font.summary_size, 96.0);
        assert_eq!(config.font.suggestion_size, 10.5);
        assert_eq!(config.font.icon_size, 48.0);
        assert_eq!(config.font.badge_size, 1.0);
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
}
