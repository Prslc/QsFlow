use crate::system::fs::{get_home, get_resource_path};
use std::path::Path;

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

    // search local images
    if let Some(internal) = get_resource_path(&format!("images/{}.png", name)) {
        return Some(internal);
    }

    // default icon
    get_resource_path(default_icon)
}
