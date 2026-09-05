use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use anyhow::Result;
use rusqlite::Connection;
use tempfile::NamedTempFile;
use tokio::task;

use crate::models::ResultItem;
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

    for base in &bases {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let db = entry.path().join("places.sqlite");
                if db.exists() {
                    return Ok(db);
                }
            }
        }
    }

    anyhow::bail!("No Firefox profile with places.sqlite found")
}

async fn do_search(mode: Mode, query: &str) -> Result<Vec<ResultItem>> {
    let query = query.to_string();
    task::spawn_blocking(move || {
        let db_path = find_db()?;
        let tmp = NamedTempFile::new()?;
        fs::copy(&db_path, tmp.path())?;

        let conn = Connection::open(tmp.path())?;

        let sql = match mode {
            Mode::Bookmarks => {
                "
                SELECT moz_places.title, moz_places.url
                FROM moz_bookmarks
                JOIN moz_places ON moz_bookmarks.fk = moz_places.id
                WHERE moz_places.url <> ''
                  AND (?1 = '' OR moz_places.title LIKE ?2 OR moz_places.url LIKE ?2)
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

        let pattern = format!("%{}%", query);
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
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<ResultItem>>>()?)
    })
    .await?
}

macro_rules! firefox_plugin {
    ($name:ident, $mode:ident, $id:literal, $display:literal, $kw:literal, $ready:literal) => {
        pub struct $name;

        impl Plugin for $name {
            fn meta(&self) -> &Meta {
                &Meta {
                    id: $id,
                    name: $display,
                    icon: "firefox",
                    ready: $ready,
                    keyword: $kw,
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
        }
    };
}

firefox_plugin!(
    FirefoxBookmarks,
    Bookmarks,
    "firefox-bookmarks",
    "Firefox Bookmarks",
    "b",
    "Search Firefox bookmarks"
);
firefox_plugin!(
    FirefoxHistory,
    History,
    "firefox-history",
    "Firefox History",
    "h",
    "Search Firefox history"
);
