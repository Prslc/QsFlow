use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::system::fs::get_resource_path;
use std::path::Path;

static CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn find_icon_path(name: &str) -> Option<String> {
    if let Ok(cache) = CACHE.lock() {
        if let Some(cached) = cache.get(name) {
            return cached.clone();
        }
    }

    let result = do_find(name);

    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(name.to_string(), result.clone());
    }

    result
}

fn do_find(name: &str) -> Option<String> {
    let default_icon = "images/application_default.png";

    if name.is_empty() {
        return get_resource_path(default_icon);
    }
    if name.starts_with('/') {
        return Some(name.to_string());
    }

    let themes = ["Papirus", "breeze", "Adwaita", "hicolor"];
    let categories = ["places", "apps", "mimetypes", "devices", "panel", "actions"];
    let sizes = ["scalable", "48x48", "32x32", "256x256", "128x128", "64x64", "24x24", "16x16"];
    let exts = ["svg", "png"];

    for theme in themes {
        for category in categories {
            for size in sizes {
                for ext in exts {
                    let path = format!("/usr/share/icons/{}/{}/{}/{}.{}", theme, size, category, name, ext);
                    if Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    // pixmaps — legacy path, many apps drop icons here
    for ext in exts {
        let path = format!("/usr/share/pixmaps/{}.{}", name, ext);
        if Path::new(&path).exists() {
            return Some(path);
        }
    }

    // flatpak exports
    let mut flatpak_bases = vec!["/var/lib/flatpak/exports/share".to_string()];
    if let Ok(home) = std::env::var("HOME") {
        flatpak_bases.push(format!("{}/.local/share/flatpak/exports/share", home));
    }
    for base in &flatpak_bases {
        for category in categories {
            for size in sizes {
                for ext in exts {
                    let path = format!("{}/icons/hicolor/{}/{}/{}.{}", base, size, category, name, ext);
                    if Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    // project images
    for ext in exts {
        if let Some(p) = get_resource_path(&format!("images/{}.{}", name, ext)) {
            return Some(p);
        }
    }

    get_resource_path(default_icon)
}
