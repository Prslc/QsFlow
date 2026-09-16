//! Theme colours, parsed from GTK4's `dank-colors.css`.
use crate::models::ThemeConfig;
use crate::system::fs::get_home;
use anyhow::Result;
use std::fs;

pub fn load_theme() -> ThemeConfig {
    let mut theme = ThemeConfig {
        primary: "#ffb59f".into(),
        on_primary: "#561f0f".into(),
        bg: "#1a110f".into(),
        fg: "#f1dfda".into(),
        container: "#271d1b".into(),
    };

    let _ = (|| -> Result<()> {
        let home = get_home()?;
        let path = home.join(".config/gtk-4.0/dank-colors.css");
        let content = fs::read_to_string(path)?;

        // Match the declaration itself: a substring hit could land on a
        // comment, an alias (`@window_bg_color`) or a longer name.
        let find_color = |name: &str| -> Option<String> {
            let prefix = format!("@define-color {name}");
            content.lines().map(str::trim).find_map(|line| {
                let rest = line.strip_prefix(&prefix)?;
                // the name has to end here: `accent_bg_color` is not
                // `accent_bg_color_more`
                if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
                    return None;
                }
                let value = rest.trim();
                let value = value.strip_suffix(';').unwrap_or(value).trim();
                (!value.is_empty()).then(|| value.to_string())
            })
        };

        if let Some(c) = find_color("accent_bg_color") {
            theme.primary = c;
        }
        if let Some(c) = find_color("accent_fg_color") {
            theme.on_primary = c;
        }
        if let Some(c) = find_color("window_bg_color") {
            theme.bg = c;
        }
        if let Some(c) = find_color("window_fg_color") {
            theme.fg = c;
        }
        if let Some(c) = find_color("popover_bg_color") {
            theme.container = c;
        }

        Ok(())
    })();

    theme
}
