use std::future::Future;
use std::pin::Pin;

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

/// Enumerate installed applications from GLib's `GAppInfo` registry — no XDG
/// directory scanning or `.desktop` parsing. `should_show()` honours
/// `Hidden`/`NoDisplay`/`OnlyShowIn`/`NotShowIn`; name/comment are localised
/// by GLib. Ranking stays fuzzy via `nucleo`. `on_click` carries the desktop
/// id so the backend can re-fetch the `GAppInfo` and `launch()` it.
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

            name_score.max(comment_score)
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
