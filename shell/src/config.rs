use std::path::PathBuf;
use std::time::{Duration, Instant};

use notify::Watcher;

use crate::ui::geom::Layout;
use crate::ui::theme;

/// `~/.config/wayrun/theme.toml`. A missing file (or any missing key) keeps the
/// default below, so an absent config is the shipped look.
pub fn theme_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".config/wayrun/theme.toml"))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorOverrides {
    pub primary: Option<[u8; 3]>,
    pub fg: Option<[u8; 3]>,
    pub container: Option<[u8; 3]>,
}

/// The shell's appearance, loaded from `theme.toml`. Every default equals the
/// renderer's constant, so an unset field changes nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceConfig {
    pub colors: ColorOverrides,
    pub blur: bool,
    pub dim_alpha: f32,
    pub card_alpha: f32,
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
            dim_alpha: 0.30,
            card_alpha: 0.72,
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
    motion: MotionFile,
}

#[derive(serde::Deserialize, Default)]
struct ColorsFile {
    primary: Option<String>,
    fg: Option<String>,
    container: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct BlurFile {
    enabled: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
struct AppearanceFile {
    dim_alpha: Option<f32>,
    card_alpha: Option<f32>,
}

#[derive(serde::Deserialize, Default)]
struct LayoutFile {
    radius: Option<f32>,
    width_ratio: Option<f32>,
    width_min: Option<f32>,
    width_max: Option<f32>,
    top_ratio: Option<f32>,
    max_rows: Option<usize>,
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
        if let Some(v) = file.blur.enabled {
            self.blur = v;
        }
        if let Some(v) = file.appearance.dim_alpha.and_then(clamp01) {
            self.dim_alpha = v;
        }
        if let Some(v) = file.appearance.card_alpha.and_then(clamp01) {
            self.card_alpha = v;
        }
        if let Some(v) = file.layout.radius.filter(|v| v.is_finite()) {
            self.layout.radius = v.max(0.0);
        }
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

fn clamp01(v: f32) -> Option<f32> {
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

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
        if last.elapsed() < Duration::from_millis(300) {
            return;
        }
        last = Instant::now();
        let _ = tx.send(AppearanceConfig::load());
    })
    .ok()?;

    watch_targets(&mut watcher, &path);
    Some(watcher)
}

/// Watch the parent dir (for atomic rename saves) AND the file itself (for
/// in-place writes, which a directory watch never reports).
fn watch_targets(watcher: &mut notify::RecommendedWatcher, path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        let _ = watcher.watch(dir, notify::RecursiveMode::NonRecursive);
    }
    if path.exists() {
        let _ = watcher.watch(path, notify::RecursiveMode::NonRecursive);
    }
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
}
