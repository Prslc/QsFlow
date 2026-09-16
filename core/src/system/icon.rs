//! Icon spec resolution to the absolute path the UI renders.

use rustc_hash::FxHashMap as HashMap;
use std::sync::{Mutex, OnceLock};

use crate::system::fs::get_resource_path;
use std::path::Path;
/// Papirus category dirs. Not every size ships every category: `panel` and
/// friends only exist in the small sizes, so lookup must scan sizes × categories.
const PAPIRUS_CATEGORIES: &[&str] = &[
    "actions",
    "apps",
    "categories",
    "devices",
    "emblems",
    "emotes",
    "mimetypes",
    "panel",
    "places",
    "status",
];
/// Preferred size order: rows render at 22–30px, so 48x48 is crisp without the
/// load cost of the 128px+ variants.
const PAPIRUS_SIZES: &[&str] = &[
    "48x48", "32x32", "64x64", "128x128", "96x96", "84x84", "42x42", "24x24", "22x22", "18x18",
    "16x16", "8x8",
];

/// Generic theme search space, shared by the icon-root and flatpak scans.
const THEME_CATEGORIES: &[&str] = &["places", "apps", "mimetypes", "devices", "panel", "actions"];
const ICON_SIZES: &[&str] = &[
    "scalable", "48x48", "32x32", "256x256", "128x128", "64x64", "24x24", "16x16",
];
const ICON_EXTS: &[&str] = &["svg", "png"];

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

    // a real category hint first (some names live under several categories)
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

    // base × size × category, first existing file wins
    for base in &bases {
        for size in PAPIRUS_SIZES {
            for category in &categories {
                let dir = format!("{base}/Papirus/{size}/{category}");
                if !dir_exists(&dir) {
                    continue;
                }
                let path = format!("{dir}/{name}.svg");
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
    let fallback = || get_resource_path("images/application_default.png");

    if name.is_empty() {
        return fallback();
    }
    if name.starts_with('/') {
        return Some(name.to_string());
    }
    // papirus:<name> / papirus:<category>/<name>: the UI renders absolute paths,
    // so resolve here rather than passing the scheme through to the wire
    if let Some(spec) = name.strip_prefix("papirus:") {
        return find_papirus(spec).or_else(fallback);
    }

    let themes = ["Papirus", "breeze", "Adwaita", "hicolor"];

    if let Some(p) = search_theme_tree("/usr/share/icons", &themes, name) {
        return Some(p);
    }

    // pixmaps — legacy path, many apps drop icons here
    for ext in ICON_EXTS {
        let path = format!("/usr/share/pixmaps/{name}.{ext}");
        if Path::new(&path).exists() {
            return Some(path);
        }
    }

    // flatpak exports (hicolor inside each export root)
    let mut flatpak_bases = vec!["/var/lib/flatpak/exports/share".to_string()];
    if let Ok(home) = std::env::var("HOME") {
        flatpak_bases.push(format!("{home}/.local/share/flatpak/exports/share"));
    }
    for base in &flatpak_bases {
        if let Some(p) = search_theme_tree(&format!("{base}/icons"), &themes, name) {
            return Some(p);
        }
    }

    // project images (plugin identity icons, e.g. application_default)
    for ext in ICON_EXTS {
        if let Some(p) = get_resource_path(&format!("images/{name}.{ext}")) {
            return Some(p);
        }
    }

    fallback()
}

/// theme × category × size × ext under one icon root, first existing file wins.
/// The directory level is memoized: nearly every combination is absent, so a
/// miss would otherwise stat hundreds of paths.
fn search_theme_tree(root: &str, themes: &[&str], name: &str) -> Option<String> {
    for theme in themes {
        for category in THEME_CATEGORIES {
            for size in ICON_SIZES {
                let dir = format!("{root}/{theme}/{size}/{category}");
                if !dir_exists(&dir) {
                    continue;
                }
                for ext in ICON_EXTS {
                    let path = format!("{dir}/{name}.{ext}");
                    if Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

/// `dir` existence, remembered per process.
fn dir_exists(dir: &str) -> bool {
    static DIRS: OnceLock<Mutex<rustc_hash::FxHashMap<std::path::PathBuf, bool>>> = OnceLock::new();
    let dirs = DIRS.get_or_init(|| Mutex::new(rustc_hash::FxHashMap::default()));

    let mut dirs = dirs.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(&known) = dirs.get(Path::new(dir)) {
        return known;
    }
    let exists = Path::new(dir).is_dir();
    dirs.insert(std::path::PathBuf::from(dir), exists);
    exists
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
    fn absolute_path_passes_through_unchanged() {
        // an already-resolved path must not re-enter the theme search
        let p = "/usr/share/icons/Papirus/48x48/apps/github.svg";
        assert_eq!(find_icon_path(p), Some(p.to_string()));
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
