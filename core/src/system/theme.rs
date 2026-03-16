use anyhow::Result;
use std::{fs, env, path::PathBuf};
use crate::models::ThemeConfig;

pub fn load_theme() -> ThemeConfig {
    // default color
    let mut theme = ThemeConfig {
        primary: "#ffb59f".into(),
        on_primary: "#561f0f".into(),
        bg: "#1a110f".into(),
        fg: "#f1dfda".into(),
        container: "#271d1b".into(),
    };

    let _ = (|| -> Result<()> {
        let home = env::var("HOME").map(PathBuf::from)?;
        let path = home.join(".config/gtk-4.0/dank-colors.css");
        let content = fs::read_to_string(path)?;

        let find_color = |name: &str| -> Option<String> {
            content.lines()
                .find(|l| l.contains(name))
                .and_then(|l| l.split_whitespace().last())
                .map(|s| s.trim_end_matches(';').to_string())
        };

        if let Some(c) = find_color("accent_bg_color") { theme.primary = c; }
        if let Some(c) = find_color("accent_fg_color") { theme.on_primary = c; }
        if let Some(c) = find_color("window_bg_color") { theme.bg = c; }
        if let Some(c) = find_color("window_fg_color") { theme.fg = c; }
        if let Some(c) = find_color("popover_bg_color") { theme.container = c; }

        Ok(())
    })();

    theme
}