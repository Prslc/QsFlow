use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use anyhow::Result;
use gio::prelude::*;
use rusqlite::{Connection, OpenFlags};
use tempfile::NamedTempFile;
use tokio::task;

use crate::models::ResultItem;
use crate::plugin::{Meta, Plugin};
use crate::system::fs::get_home;
use crate::system::icon::find_icon_path;

#[derive(Clone, Copy)]
enum Mode {
    Bookmarks,
    History,
}

/// The profile Firefox used most recently. `read_dir` order is arbitrary, and
/// the profiles differ by 6× in size (31MB/30k rows vs 5MB/250).
fn find_db() -> Result<PathBuf> {
    let home = get_home()?;
    let bases = [
        home.join(".mozilla/firefox"),
        home.join(".config/mozilla/firefox"),
    ];

    let mut candidates = Vec::new();
    for base in &bases {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let db = entry.path().join("places.sqlite");
                if db.is_file() {
                    candidates.push(db);
                }
            }
        }
    }

    candidates.sort_by_key(|db| std::fs::metadata(db).and_then(|meta| meta.modified()).ok());
    candidates
        .pop()
        .ok_or_else(|| anyhow::anyhow!("No Firefox profile with places.sqlite found"))
}

/// Search the active profile in place, instead of copying `places.sqlite` per
/// keystroke. A plain read-only open is not enough — with a `-wal`/`-shm`
/// present it fails or waits out SQLite's busy timeout — so the URI carries
/// `immutable=1` (main file only, which is what the copy saw too). The copy
/// stays as the fallback.
async fn do_search(mode: Mode, query: &str) -> Result<Vec<ResultItem>> {
    let query = query.to_string();
    task::spawn_blocking(move || {
        let db_path = find_db()?;
        if let Ok(items) = query_in_place(&db_path, mode, &query) {
            return Ok(items);
        }

        let tmp = NamedTempFile::new()?;
        fs::copy(&db_path, tmp.path())?;
        let conn = Connection::open(tmp.path())?;
        run_query(&conn, mode, &query)
    })
    .await?
}

/// Query the live database through a GLib-built `file:` URI (paths with spaces
/// or non-ASCII characters stay valid) with `immutable=1`.
fn query_in_place(db_path: &Path, mode: Mode, query: &str) -> Result<Vec<ResultItem>> {
    let uri = format!("{}?immutable=1", gio::File::for_path(db_path).uri());
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    run_query(&conn, mode, query)
}

/// Run one search against an open database.
fn run_query(conn: &Connection, mode: Mode, query: &str) -> Result<Vec<ResultItem>> {
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
    let rows = stmt.query_map([query, &pattern], move |row| {
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
