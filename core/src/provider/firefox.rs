use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::SystemTime;

use anyhow::Result;
use rusqlite::Connection;
use tempfile::NamedTempFile;
use tokio::task;

use crate::models::{ActionItem, ResultItem};
use crate::plugin::{Meta, Plugin};
use crate::system::fs::get_home;
use crate::system::icon::find_icon_path;

enum Mode {
    Bookmarks,
    History,
}

fn find_db() -> Result<PathBuf> {
    let home = get_home()?;
    let bases = [
        home.join(".mozilla/firefox"),
        home.join(".config/mozilla/firefox"),
    ];

    // `read_dir` order is arbitrary and a machine can hold several profiles
    // (a stale ESR beside the live release), so take the most recently written
    // one instead of the first the filesystem happens to list.
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for base in &bases {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let db = entry.path().join("places.sqlite");
                let Ok(modified) = fs::metadata(&db).and_then(|meta| meta.modified()) else {
                    continue;
                };
                if newest.as_ref().is_none_or(|(time, _)| modified > *time) {
                    newest = Some((modified, db));
                }
            }
        }
    }

    newest
        .map(|(_, path)| path)
        .ok_or_else(|| anyhow::anyhow!("No Firefox profile with places.sqlite found"))
}

async fn do_search(mode: Mode, query: &str) -> Result<Vec<ResultItem>> {
    if query.is_empty() {
        return Ok(vec![]);
    }

    let query = query.to_string();
    task::spawn_blocking(move || {
        let db_path = find_db()?;
        let tmp = NamedTempFile::new()?;
        fs::copy(&db_path, tmp.path())?;

        let conn = Connection::open(tmp.path())?;

        let sql = match mode {
            Mode::Bookmarks => {
                // The bookmark's own title (`moz_bookmarks.title`) is what the
                // user saved and may have edited; `moz_places.title` is only the
                // page's last-visited title, which Firefox overwrites on every
                // visit. Prefer the former and fall back to the latter.
                "
                SELECT COALESCE(NULLIF(moz_bookmarks.title, ''), moz_places.title) AS title,
                       moz_places.url
                FROM moz_bookmarks
                JOIN moz_places ON moz_bookmarks.fk = moz_places.id
                WHERE moz_places.url <> ''
                  AND (?1 = ''
                       OR COALESCE(NULLIF(moz_bookmarks.title, ''), moz_places.title) LIKE ?2
                       OR moz_places.url LIKE ?2)
                ORDER BY moz_bookmarks.dateAdded DESC
                LIMIT 50
            "
            }
            Mode::History => {
                "
                SELECT moz_places.title, moz_places.url
                FROM moz_places
                JOIN moz_historyvisits ON moz_places.id = moz_historyvisits.place_id
                WHERE moz_places.url <> ''
                  AND (?1 = '' OR moz_places.title LIKE ?2 OR moz_places.url LIKE ?2)
                ORDER BY moz_historyvisits.visit_date DESC
                LIMIT 50
            "
            }
        };

        let pattern = format!("%{query}%");
        let firefox_icon = find_icon_path("firefox");
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([query.as_str(), &pattern], move |row| {
            let title: Option<String> = row.get(0)?;
            let url: String = row.get(1)?;
            Ok(ResultItem {
                title: title.unwrap_or_else(|| "[no title]".to_string()),
                summary: Some(url.clone()),
                on_click: Some(url),
                icon: firefox_icon.clone(),
                ephemeral: false,
                actions: Vec::new(),
                badge: None,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<ResultItem>>>()?)
    })
    .await?
}

macro_rules! firefox_plugin {
    ($name:ident, $mode:ident, $id:literal, $display:literal, $ready:literal) => {
        pub struct $name;

        impl Plugin for $name {
            fn meta(&self) -> &Meta {
                &Meta {
                    id: $id,
                    name: $display,
                    icon: "firefox",
                    ready: $ready,
                }
            }

            fn search(
                &self,
                query: &str,
                _full: &str,
            ) -> Pin<Box<dyn Future<Output = Result<Vec<ResultItem>>> + Send + '_>> {
                let query = query.to_string();
                Box::pin(async move { do_search(Mode::$mode, &query).await })
            }

            fn actions(&self, item: &ResultItem) -> Vec<ActionItem> {
                copy_url_action(item)
            }
        }
    };
}

/// A bookmark/history row's extra command: copy the link instead of opening it.
fn copy_url_action(item: &ResultItem) -> Vec<ActionItem> {
    let Some(url) = item
        .on_click
        .as_deref()
        .filter(|on_click| on_click.starts_with("http"))
    else {
        return Vec::new();
    };
    vec![ActionItem {
        title: "Copy URL".to_string(),
        on_click: format!("copy:{}", serde_json::json!({ "text": url })),
        icon: Some("edit-copy".to_string()),
    }]
}

firefox_plugin!(
    FirefoxBookmarks,
    Bookmarks,
    "firefox-bookmarks",
    "Firefox Bookmarks",
    "Search Firefox bookmarks"
);
firefox_plugin!(
    FirefoxHistory,
    History,
    "firefox-history",
    "Firefox History",
    "Search Firefox history"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_query_matches_nothing() {
        assert!(do_search(Mode::Bookmarks, "").await.unwrap().is_empty());
        assert!(do_search(Mode::History, "").await.unwrap().is_empty());
    }

    #[test]
    fn a_bookmark_row_offers_a_copy_link() {
        let row = ResultItem {
            title: "Example".to_string(),
            summary: None,
            on_click: Some("https://example.com".to_string()),
            icon: None,
            ephemeral: false,
            actions: Vec::new(),
            badge: None,
        };
        let actions = copy_url_action(&row);
        assert_eq!(actions[0].title, "Copy URL");
        assert_eq!(
            actions[0].on_click,
            r#"copy:{"text":"https://example.com"}"#
        );
    }
}
