use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::PathBuf;

use crate::system::fs::get_home;

fn db_path() -> Result<PathBuf> {
    let dir = get_home()?.join(".local/share/qsflow");
    std::fs::create_dir_all(&dir).context("Failed to create qsflow data directory")?;
    Ok(dir.join("usage.db"))
}

fn conn() -> Result<Connection> {
    let conn = Connection::open(db_path()?)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage (
            key TEXT PRIMARY KEY,
            count INTEGER NOT NULL DEFAULT 1,
            last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
            item_json TEXT NOT NULL
        );"
    )?;
    Ok(conn)
}

pub fn record(item_json: &str) -> Result<()> {
    let item: serde_json::Value = serde_json::from_str(item_json)?;
    let key = item["on_click"].as_str()
        .context("item missing on_click")?;

    let conn = conn()?;
    conn.execute(
        "INSERT INTO usage (key, count, last_used_at, item_json)
         VALUES (?1, 1, datetime('now'), ?2)
         ON CONFLICT(key) DO UPDATE SET
             count = count + 1,
             last_used_at = datetime('now'),
             item_json = ?2",
        rusqlite::params![key, item_json],
    )?;
    Ok(())
}

pub fn get_top(limit: i32) -> Result<Vec<serde_json::Value>> {
    let conn = conn()?;
    let mut stmt = conn.prepare(
        "SELECT item_json FROM usage ORDER BY count DESC, last_used_at DESC LIMIT ?1"
    )?;
    let rows = stmt.query_map([limit], |row| {
        let json: String = row.get(0)?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}
