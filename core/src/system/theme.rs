use crate::models::ThemeConfig;
use std::path::PathBuf;

/// The Material palette DankMaterialShell generated with matugen for the
/// current theme. It holds both modes at once and a `mode` selecting the one in
/// use, so a light/dark switch is a plain file change.
pub fn dms_colors_path() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("DankMaterialShell/dms-colors.json"))
}

#[derive(serde::Deserialize)]
struct DmsColors {
    mode: String,
    colors: DmsModes,
}

#[derive(serde::Deserialize)]
struct DmsModes {
    light: DmsPalette,
    dark: DmsPalette,
}

/// Only the roles the shell draws with; unknown keys are ignored.
#[derive(serde::Deserialize)]
struct DmsPalette {
    primary: String,
    on_primary: String,
    background: String,
    on_surface: String,
    surface_container_high: String,
}

/// The built-in dark palette, used when no system theme is available.
pub fn default_theme() -> ThemeConfig {
    ThemeConfig {
        primary: "#ffb59f".into(),
        on_primary: "#561f0f".into(),
        bg: "#1a110f".into(),
        fg: "#f1dfda".into(),
        container: "#271d1b".into(),
    }
}

pub fn load_theme() -> ThemeConfig {
    load_dms().unwrap_or_else(default_theme)
}

/// The shell draws the container from `surface_container_high` (what the GTK
/// template fed `popover_bg_color`), so the card keeps its depth.
fn load_dms() -> Option<ThemeConfig> {
    let text = std::fs::read_to_string(dms_colors_path()?).ok()?;
    from_json(&text)
}

fn from_json(text: &str) -> Option<ThemeConfig> {
    let file: DmsColors = serde_json::from_str(text).ok()?;
    let palette = match file.mode.as_str() {
        "dark" => file.colors.dark,
        _ => file.colors.light,
    };
    Some(ThemeConfig {
        primary: palette.primary,
        on_primary: palette.on_primary,
        bg: palette.background,
        fg: palette.on_surface,
        container: palette.surface_container_high,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(mode: &str) -> String {
        format!(
            r##"{{
                "mode": "{mode}",
                "colors": {{
                    "light": {{
                        "primary": "#35618e",
                        "on_primary": "#ffffff",
                        "background": "#f8f9ff",
                        "on_surface": "#191c20",
                        "surface_container_high": "#eceef4"
                    }},
                    "dark": {{
                        "primary": "#a0cafd",
                        "on_primary": "#003257",
                        "background": "#101418",
                        "on_surface": "#e1e2e8",
                        "surface_container_high": "#1d2024"
                    }}
                }}
            }}"##
        )
    }

    #[test]
    fn the_mode_key_selects_the_palette() {
        let light = from_json(&sample("light")).unwrap();
        assert_eq!(light.primary, "#35618e");
        assert_eq!(light.bg, "#f8f9ff");
        assert_eq!(light.fg, "#191c20");
        assert_eq!(light.container, "#eceef4");

        let dark = from_json(&sample("dark")).unwrap();
        assert_eq!(dark.primary, "#a0cafd");
        assert_eq!(dark.bg, "#101418");
        assert_eq!(dark.container, "#1d2024");
    }

    #[test]
    fn an_unknown_mode_falls_back_to_light() {
        assert_eq!(from_json(&sample("sepia")).unwrap().bg, "#f8f9ff");
    }

    #[test]
    fn a_missing_role_or_bad_json_is_none() {
        assert!(from_json("not json").is_none());
        assert!(from_json(r#"{"mode":"light","colors":{"light":{},"dark":{}}}"#).is_none());
    }
}
