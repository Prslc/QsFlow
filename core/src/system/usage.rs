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
    record_with(&conn()?, item_json)
}

pub fn forget(key: &str) -> Result<()> {
    forget_with(&conn()?, key)
}

pub fn get_top(limit: i32) -> Result<Vec<serde_json::Value>> {
    get_top_with(&conn()?, limit)
}

#[allow(dead_code)]
fn init_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage (
            key TEXT PRIMARY KEY,
            count INTEGER NOT NULL DEFAULT 1,
            last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
            item_json TEXT NOT NULL
        );"
    ).ok();
}

fn record_with(conn: &Connection, item_json: &str) -> Result<()> {
    let item: serde_json::Value = serde_json::from_str(item_json)?;
    let key = item["on_click"].as_str()
        .context("item missing on_click")?;

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

fn forget_with(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM usage WHERE key = ?1", rusqlite::params![key])?;
    Ok(())
}

fn get_top_with(conn: &Connection, limit: i32) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT item_json FROM usage ORDER BY count DESC, last_used_at DESC LIMIT ?1"
    )?;
    let rows = stmt.query_map([limit], |row| {
        let json: String = row.get(0)?;
        Ok(serde_json::from_str(&json).unwrap_or_default())
    })?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn);
        conn
    }

    #[test]
    fn record_and_get_top() {
        let conn = test_conn();
        let json = r#"{"title":"Firefox","on_click":"run:firefox"}"#;
        record_with(&conn, json).unwrap();
        record_with(&conn, json).unwrap();

        let items = get_top_with(&conn, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Firefox");
    }

    #[test]
    fn forget_removes_entry() {
        let conn = test_conn();
        record_with(&conn, r#"{"title":"A","on_click":"run:a"}"#).unwrap();
        assert_eq!(get_top_with(&conn, 10).unwrap().len(), 1);

        forget_with(&conn, "run:a").unwrap();
        assert!(get_top_with(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn top_is_sorted_by_count() {
        let conn = test_conn();
        record_with(&conn, r#"{"title":"A","on_click":"run:a"}"#).unwrap();
        record_with(&conn, r#"{"title":"B","on_click":"run:b"}"#).unwrap();
        record_with(&conn, r#"{"title":"B","on_click":"run:b"}"#).unwrap();

        let items = get_top_with(&conn, 10).unwrap();
        assert_eq!(items[0]["title"], "B");
        assert_eq!(items[1]["title"], "A");
    }

    #[test]
    fn record_missing_on_click() {
        let conn = test_conn();
        assert!(record_with(&conn, r#"{"title":"no key"}"#).is_err());
    }
}
