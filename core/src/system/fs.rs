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

fn find_up_from_bin(sub_path: &str) -> Option<String> {
    let mut dir = env::current_exe().ok()?.parent()?.to_path_buf();
    loop {
        let candidate = dir.join(sub_path);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn find_in_xdg_data(sub_path: &str) -> Option<String> {
    let mut dirs: Vec<PathBuf> = env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string())
        .split(':')
        .map(|s| PathBuf::from(s).join("wayrun"))
        .collect();

    if let Ok(home) = env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/wayrun"));
    }

    for dir in &dirs {
        let candidate = dir.join(sub_path);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

pub fn get_resource_path(sub_path: &str) -> Option<String> {
    if let Ok(dir) = env::var("QFLOW_RESOURCE_DIR") {
        let p = PathBuf::from(dir).join(sub_path);
        if p.exists() {
            return Some(p.to_string_lossy().into_owned());
        }
    }

    // search upward from the executable (dev / portable installs)
    if let Some(p) = find_up_from_bin(sub_path) {
        return Some(p);
    }

    // search XDG data directories (system-wide installs)
    if let Some(p) = find_in_xdg_data(sub_path) {
        return Some(p);
    }

    // relative to Cargo workspace root, debug builds only
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .join(sub_path);
        if dev.exists() {
            return Some(dev.to_string_lossy().into_owned());
        }
    }

    None
}
