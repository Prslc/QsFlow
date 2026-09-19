use anyhow::{Context, Result};
use dirs;
use std::env;
use std::path::PathBuf;

pub fn get_home() -> Result<PathBuf> {
    dirs::home_dir().context("Failed to get user HOME directory")
}

/// Flatpak apps live in `<installation>/exports/share`, which only reaches
/// `XDG_DATA_DIRS` from a login shell's profile script. Runs before `GLib` caches
/// the dirs.
pub fn ensure_flatpak_data_dirs() {
    let home = env::var("HOME").unwrap_or_default();
    let dirs =
        env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    let missing: Vec<String> = [
        format!("{home}/.local/share/flatpak/exports/share"),
        "/var/lib/flatpak/exports/share".to_string(),
    ]
    .into_iter()
    .filter(|dir| PathBuf::from(dir).is_dir() && !dirs.split(':').any(|d| d == dir))
    .collect();

    if !missing.is_empty() {
        // SAFETY: `main` calls this before any other thread exists.
        unsafe { env::set_var("XDG_DATA_DIRS", format!("{}:{}", missing.join(":"), dirs)) };
    }
}
