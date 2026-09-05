use std::future::Future;
use std::pin::Pin;
use std::{env, fs, path::Path};

use anyhow::Result;
use gio::prelude::*;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::icon::find_icon_path;

pub struct AppSearch;

impl Plugin for AppSearch {
    fn meta(&self) -> &Meta {
        &Meta {
            id: "app-search",
            name: "Applications",
            icon: "application_default",
            ready: "Search installed applications",
            keyword: "",
        }
    }

    fn search(
        &self,
        _query: &str,
        full: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
        let input = full.to_string();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || do_search(&input))
                .await
                .unwrap_or_else(|_| Ok(vec![]))
        })
    }
}

/// Fallback match surface: `GenericName` + `Keywords` read from the app's
/// `.desktop` file, located by its id through the XDG data dirs. Some apps
/// (flatpak GIMP is the classic case) carry their common name only in
/// `Keywords` while `Name`/`Comment` are prose ("GNU Image Manipulation
/// Program"), so fuzzy name/comment matching misses them entirely. Parsed
/// lazily — only when the primary fields scored zero.
fn desktop_meta_haystack(id: &str) -> Option<String> {
    let mut bases: Vec<String> = env::var("XDG_DATA_DIRS")
        .map(|s| s.split(':').map(String::from).collect())
        .unwrap_or_else(|_| vec!["/usr/local/share/".into(), "/usr/share/".into()]);
    if let Ok(home) = env::var("HOME") {
        bases.push(format!("{home}/.local/share"));
    }

    for base in bases {
        let file = Path::new(&base).join("applications").join(id);
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };

        let mut generic = String::new();
        let mut keywords = String::new();
        let mut in_entry = false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
            } else if in_entry {
                // plain (unlocalised) keys only — the [xx] variants follow
                // later in the file and would otherwise overwrite these
                if let Some(v) = line.strip_prefix("GenericName=") {
                    if generic.is_empty() {
                        generic = v.trim().to_string();
                    }
                } else if let Some(v) = line.strip_prefix("Keywords=") {
                    if keywords.is_empty() {
                        keywords = v.replace(';', " ");
                    }
                }
            }
        }

        let hay = format!("{} {keywords}", generic.trim());
        if !hay.trim().is_empty() {
            return Some(hay);
        }
    }
    None
}

/// Enumerate installed applications from GLib's `GAppInfo` registry — no XDG
/// directory scanning or `.desktop` parsing (a lazy `GenericName`/`Keywords`
/// lookup in `desktop_meta_haystack` only kicks in when name/comment miss).
/// `should_show()` honours `Hidden`/`NoDisplay`/`OnlyShowIn`/`NotShowIn`;
/// name/comment are localised by GLib. Ranking stays fuzzy via `nucleo`.
/// `on_click` carries the desktop id so the backend can re-fetch the
/// `GAppInfo` and `launch()` it.
fn do_search(query: &str) -> Result<Vec<ResultItem>> {
    let mut matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
    let pattern = nucleo::Utf32String::from(query.to_lowercase());

    let mut results: Vec<(u16, ResultItem)> = Vec::new();

    for app in gio::AppInfo::all() {
        if !app.should_show() {
            continue;
        }
        let Some(id) = app.id().map(|s| s.to_string()) else {
            continue;
        };

        let title = app.name().to_string();
        let comment = app.description().map(|s| s.to_string());

        let score = if query.is_empty() {
            1
        } else {
            let title_utf32 = nucleo::Utf32String::from(title.to_lowercase());
            let name_score = matcher
                .fuzzy_match(title_utf32.slice(..), pattern.slice(..))
                .unwrap_or(0);

            let comment_score = comment
                .as_ref()
                .and_then(|c| {
                    let c_utf32 = nucleo::Utf32String::from(c.to_lowercase());
                    matcher.fuzzy_match(c_utf32.slice(..), pattern.slice(..))
                })
                .unwrap_or(0);

            let meta_score = if name_score == 0 && comment_score == 0 {
                desktop_meta_haystack(&id)
                    .and_then(|hay| {
                        let h_utf32 = nucleo::Utf32String::from(hay.to_lowercase());
                        matcher.fuzzy_match(h_utf32.slice(..), pattern.slice(..))
                    })
                    .unwrap_or(0)
            } else {
                0
            };

            name_score.max(comment_score).max(meta_score)
        };

        if score > 0 {
            let icon_path = app
                .icon()
                .and_then(|i| i.to_string())
                .map(|s| s.to_string())
                .and_then(|name| {
                    // `g_icon_to_string` yields `!!/path` for file icons.
                    if let Some(path) = name.strip_prefix("!!") {
                        (!path.is_empty()).then(|| path.to_string())
                    } else {
                        find_icon_path(&name)
                    }
                });

            results.push((
                score,
                ResultItem {
                    title,
                    summary: comment,
                    on_click: Some(format!("launch:{}", id)),
                    icon: icon_path,
                },
            ));
        }
    }

    results.sort_by(|a, b| b.0.cmp(&a.0));
    results.dedup_by(|a, b| a.1.title == b.1.title);
    results.truncate(50);

    Ok(results.into_iter().map(|(_, item)| item).collect())
}
