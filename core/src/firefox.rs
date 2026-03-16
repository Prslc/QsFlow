use anyhow::Result;
use rusqlite::Connection;
use std::fs;
use tempfile::NamedTempFile;
use crate::utils;
use crate::utils::ResultItem;
use tokio::task;

pub enum Mode {
    Bookmarks,
    History,
}

pub async fn firefox_search(mode: Mode, query: &str) -> Result<Vec<ResultItem>> {
    let mode = mode;
    let query = query.to_string();

    let results = task::spawn_blocking(move || -> Result<Vec<ResultItem>> {
        let db_path = utils::get_firefox_db_path()?;

        // copy database to tmpfs
        let tmp_file = NamedTempFile::new()?;
        fs::copy(&db_path, tmp_file.path())?;

        let conn = Connection::open(tmp_file.path())?;

        let sql = match mode {
            Mode::Bookmarks => "
                SELECT moz_places.title, moz_places.url
                FROM moz_bookmarks
                JOIN moz_places ON moz_bookmarks.fk = moz_places.id
                WHERE moz_places.url <> ''
                  AND (?1 = '' OR moz_places.title LIKE ?2 OR moz_places.url LIKE ?2)
                ORDER BY moz_bookmarks.dateAdded DESC
                LIMIT 50
            ",
            Mode::History => "
                SELECT moz_places.title, moz_places.url
                FROM moz_places
                JOIN moz_historyvisits ON moz_places.id = moz_historyvisits.place_id
                WHERE moz_places.url <> ''
                  AND (?1 = '' OR moz_places.title LIKE ?2 OR moz_places.url LIKE ?2)
                ORDER BY moz_historyvisits.visit_date DESC
                LIMIT 50
            ",
        };

        let search_pattern = format!("%{}%", query);

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([query.as_str(), &search_pattern], |row| {
            let title: Option<String> = row.get(0)?;
            let url: String = row.get(1)?;
            Ok(ResultItem {
                title: title.unwrap_or_else(|| "[no title]".to_string()),
                summary: Some(url.clone()),
                on_click: Some(url),
                icon: Some("".to_string()),
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    })
    .await??;

    Ok(results)
}