use std::sync::{Mutex, OnceLock};
use rustc_hash::FxHashMap as HashMap;

use crate::system::fs::get_resource_path;
use std::path::Path;
/// Papirus category dirs. Not every size ships every category: `panel` and
/// friends only exist in the small sizes, so lookup must scan sizes × categories.
const PAPIRUS_CATEGORIES: &[&str] = &[
    "actions", "apps", "categories", "devices", "emblems", "emotes",
    "mimetypes", "panel", "places", "status",
];
/// Preferred size order — the UI renders rows at 22–30px, so 48x48 gives crisp
/// downscale headroom without wasting load time on the 128px+ variants.
const PAPIRUS_SIZES: &[&str] = &[
    "48x48", "32x32", "64x64", "128x128", "96x96", "84x84", "42x42",
    "24x24", "22x22", "18x18", "16x16", "8x8",
];

/// `papirus:name` -> `(None, "name")`; `papirus:category/name` ->
/// `(Some("category"), "name")`.
fn parse_papirus_spec(spec: &str) -> (Option<&str>, &str) {
    match spec.split_once('/') {
        Some((cat, name)) => (Some(cat), name),
        None => (None, spec),
    }
}

fn find_papirus(spec: &str) -> Option<String> {
    let (hint, name) = parse_papirus_spec(spec);

    // category hint first when it names a real Papirus category, then the rest
    // (some names live under multiple categories)
    let mut categories: Vec<&str> = Vec::with_capacity(PAPIRUS_CATEGORIES.len() + 1);
    if let Some(h) = hint.filter(|h| PAPIRUS_CATEGORIES.contains(h)) {
        categories.push(h);
    }
    categories.extend(
        PAPIRUS_CATEGORIES
            .iter()
            .copied()
            .filter(|c| Some(*c) != hint),
    );

    let mut bases = vec!["/usr/share/icons".to_string()];
    if let Ok(home) = std::env::var("HOME") {
        bases.push(format!("{}/.local/share/icons", home));
    }

    for base in &bases {
        for size in PAPIRUS_SIZES {
            for category in &categories {
                let path = format!("{base}/Papirus/{size}/{category}/{name}.svg");
                if Path::new(&path).exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::default()))
}

pub fn find_icon_path(name: &str) -> Option<String> {
    if let Ok(cache) = cache().lock()
        && let Some(cached) = cache.get(name)
    {
        return cached.clone();
    }

    let result = do_find(name);

    if let Ok(mut cache) = cache().lock() {
        cache.insert(name.to_string(), result.clone());
    }

    result
}

fn do_find(name: &str) -> Option<String> {
    let default_icon = "images/application_default.png";

    if name.is_empty() {
        return get_resource_path(default_icon);
    }
    // `papirus:<name>` — explicit Papirus theme reference for external hosts;
    // `papirus:<category>/<name>` scopes the lookup to one category first.
    // The UI only renders absolute paths, so resolve here rather than passing
    // the scheme through to the wire.
    if let Some(spec) = name.strip_prefix("papirus:") {
        return find_papirus(spec).or_else(|| get_resource_path(default_icon));
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn papirus_spec_splits_category_hint() {
        assert_eq!(parse_papirus_spec("folder-open"), (None, "folder-open"));
        assert_eq!(
            parse_papirus_spec("panel/system-shutdown"),
            (Some("panel"), "system-shutdown")
        );
    }

    #[test]
    fn papirus_resolves_to_installed_path() {
        if !Path::new("/usr/share/icons/Papirus").exists() {
            return; // theme not installed on this machine
        }
        let path = find_icon_path("papirus:folder-open").unwrap();
        assert!(path.contains("/Papirus/"));
        assert!(path.ends_with(".svg"));
    }

    #[test]
    fn papirus_category_hint_resolves() {
        if !Path::new("/usr/share/icons/Papirus").exists() {
            return;
        }
        // `system-shutdown` also lives under `apps`; the hint scopes to `panel`
        // first, then falls back to the other categories.
        let path = find_icon_path("papirus:apps/system-shutdown").unwrap();
        assert!(path.contains("/apps/system-shutdown.svg"));
    }

    #[test]
    fn papirus_unknown_name_falls_back_to_default() {
        let path = find_icon_path("papirus:definitely-not-an-icon-xyz");
        assert!(path.is_some()); // default icon, same semantics as any miss
    }
}
