use anyhow::{Context, Result};
use dirs;
use std::env;
use glob::glob;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct ResultItem {
    pub title: String,
    pub summary: Option<String>,
    pub on_click: Option<String>,
    pub icon: Option<String>,
}

pub fn get_project_root() -> Option<PathBuf> {
    env::current_exe()
        .ok()?
        .parent()? // bin
        .parent()  // project root
        .map(|p| p.to_path_buf())
}

/// get resource path
pub fn get_resource_path(sub_path: &str) -> Option<String> {
    get_project_root()
        .map(|root| root.join(sub_path))
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().into_owned())
}

/// exec command
pub fn execute_command(cmd: &str) {
    let clean_cmd = cmd.replace("%u", "").replace("%U", "").replace("%f", "").replace("%F", "");

    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("setsid {} >/dev/null 2>&1 &", clean_cmd))
        .spawn()
        .ok();
}

/// Returns the user's HOME directory.
pub fn get_home() -> Result<PathBuf> {
    dirs::home_dir().context("Failed to get user HOME directory")
}

/// Locates the Firefox `places.sqlite` database file.
pub fn get_firefox_db_path() -> Result<PathBuf> {
    let home = get_home()?;
    let pattern = home.join(".config/mozilla/firefox/*.default-release");
    let pattern_str = pattern.to_str().context("Invalid UTF-8 path string")?;

    for entry in glob(pattern_str).context("Failed to read glob pattern")? {
        if let Ok(path) = entry {
            return Ok(path.join("places.sqlite"));
        }
    }

    anyhow::bail!("No Firefox profile found")
}

/// Searches for an application icon or returns the project's default icon.
pub fn find_icon_path(name: &str) -> Option<String> {
    let default_icon: &str = "images/application_default.png";
    // 1. Check if the name is an absolute path already
    if name.is_empty() {
        return get_resource_path(default_icon);
    }
    if name.starts_with('/') {
        return Some(name.to_string());
    }

    // 2. Search in standard Linux icon paths
    let mut base_dirs = vec![
        // System-wide SVG icons (highest quality)
        "/usr/share/icons/hicolor/scalable/apps".to_string(),
        // System-wide fixed-size PNG icons (fallback)
        "/usr/share/icons/hicolor/48x48/apps".to_string(),
        // Legacy path for miscellaneous application icons
        "/usr/share/pixmaps".to_string(),
        // Flatpak system-wide exported icons
        "/var/lib/flatpak/exports/share/icons/hicolor/scalable/apps".to_string(),
    ];

    if let Ok(home) = get_home() {
        // Flatpak user-specific exported icons
        let flatpak_user_icon_path = home
            .join(".local/share/flatpak/exports/share/icons/hicolor/scalable/apps")
            .to_string_lossy()
            .into_owned();
        base_dirs.push(flatpak_user_icon_path);
    }

    // 3. Fallback to your project's default icon
    for dir in base_dirs {
        for ext in ["svg", "png"] {
            let path = format!("{}/{}.{}", dir, name, ext);
            if Path::new(&path).exists() {
                return Some(path);
            }
        }
    }

    // default icon
    get_resource_path(default_icon)
}
