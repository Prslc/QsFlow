use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, PoisonError};

use crate::system::fs::get_home;

/// The schema every open ensures. `usage` is the ranked history behind an empty
/// query, `pins` the per-query favorites.
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS usage (
        key TEXT PRIMARY KEY,
        on_click TEXT,
        count INTEGER NOT NULL DEFAULT 1,
        last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
        item_json TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS pins (
        id INTEGER PRIMARY KEY,
        scope TEXT NOT NULL,
        on_click TEXT NOT NULL,
        item_json TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        UNIQUE (scope, on_click)
    );";

fn db_path() -> Result<PathBuf> {
    let dir = get_home()?.join(".local/share/wayrun");
    std::fs::create_dir_all(&dir).context("creating the wayrun data directory")?;
    Ok(dir.join("usage.db"))
}

fn open_conn() -> Result<Connection> {
    let conn = Connection::open(db_path()?)?;
    conn.execute_batch(SCHEMA)?;
    migrate_usage(&conn)?;
    migrate_pins(&conn)?;
    Ok(conn)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    Ok(names.any(|name| name.is_ok_and(|name| name == column)))
}

/// One connection for the process lifetime, opened lazily and left unset on
/// failure (no `$HOME`, full disk) so history degrades to empty, not a panic.
static DB: LazyLock<Mutex<Option<Connection>>> = LazyLock::new(|| Mutex::new(None));

/// Run `f` on the shared connection, surviving lock poisoning.
pub fn with_db<T>(f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let mut guard = DB.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_none() {
        *guard = Some(open_conn()?);
    }
    f(guard.as_ref().expect("opened just above"))
}

/// Migrate a database without the `on_click` column, merging same-title rows
/// (counts sum, newest wins) so one app reached two ways is one entry.
fn migrate_usage(conn: &Connection) -> Result<()> {
    if column_exists(conn, "usage", "on_click")? {
        return Ok(());
    }

    let rows: Vec<(String, i64, String, String)> = {
        let mut stmt = conn.prepare("SELECT key, count, last_used_at, item_json FROM usage")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .filter_map(Result::ok)
            .collect()
    };

    // title -> (sum count, item_json of the most recent use, its last_used_at)
    let mut merged: BTreeMap<String, (i64, String, String)> = BTreeMap::new();
    for (key, count, last_used_at, item_json) in rows {
        let title = serde_json::from_str::<serde_json::Value>(&item_json)
            .ok()
            .and_then(|v| v["title"].as_str().map(String::from))
            .filter(|t| !t.is_empty())
            .unwrap_or(key);
        let entry = merged.entry(title).or_default();
        entry.0 += count;
        if last_used_at > entry.2 {
            entry.1 = item_json;
            entry.2 = last_used_at;
        }
    }

    conn.execute_batch(
        "DROP TABLE usage;
         CREATE TABLE usage (
            key TEXT PRIMARY KEY,
            on_click TEXT,
            count INTEGER NOT NULL DEFAULT 1,
            last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
            item_json TEXT NOT NULL
         );",
    )?;
    let mut stmt = conn.prepare(
        "INSERT INTO usage (key, on_click, count, last_used_at, item_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (title, (count, item_json, last_used_at)) in merged {
        let on_click = serde_json::from_str::<serde_json::Value>(&item_json)
            .ok()
            .and_then(|v| v["on_click"].as_str().map(String::from));
        stmt.execute(rusqlite::params![
            title,
            on_click,
            count,
            last_used_at,
            item_json
        ])?;
    }
    Ok(())
}

/// Give a pins table created before the explicit `id` the integer primary key
/// its ordering now uses, copying the rows across in insertion order.
fn migrate_pins(conn: &Connection) -> Result<()> {
    if column_exists(conn, "pins", "id")? {
        return Ok(());
    }

    conn.execute_batch(
        "ALTER TABLE pins RENAME TO pins_old;
         CREATE TABLE pins (
            id INTEGER PRIMARY KEY,
            scope TEXT NOT NULL,
            on_click TEXT NOT NULL,
            item_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE (scope, on_click)
         );
         INSERT INTO pins (scope, on_click, item_json, created_at)
             SELECT scope, on_click, item_json, created_at FROM pins_old ORDER BY rowid;
         DROP TABLE pins_old;",
    )?;
    Ok(())
}

#[cfg(test)]
pub fn init_schema(conn: &Connection) {
    conn.execute_batch(SCHEMA).ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Legacy layout: key = `on_click`, no `on_click` column.
    fn legacy_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE usage (
                key TEXT PRIMARY KEY,
                count INTEGER NOT NULL DEFAULT 1,
                last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
                item_json TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn migrate_merges_legacy_split_rows() {
        let conn = legacy_conn();
        // old layout: key IS the on_click; the same app under both forms
        conn.execute(
            "INSERT INTO usage (key, count, last_used_at, item_json)
             VALUES ('run:Telegram --', 8, '2026-09-03 15:40:00',
                     '{\"title\":\"Telegram\",\"on_click\":\"run:Telegram --\"}'),
                    ('launch:org.telegram.desktop.desktop', 3, '2026-09-04 07:43:22',
                     '{\"title\":\"Telegram\",\"on_click\":\"launch:org.telegram.desktop.desktop\"}')",
            [],
        )
        .unwrap();

        migrate_usage(&conn).unwrap();

        let (count, on_click): (i64, Option<String>) = conn
            .query_row(
                "SELECT count, on_click FROM usage WHERE key = 'Telegram'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 11);
        assert_eq!(
            on_click.as_deref(),
            Some("launch:org.telegram.desktop.desktop")
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM usage", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn migrate_pins_adds_the_id_primary_key_and_keeps_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE pins (
                scope TEXT NOT NULL,
                on_click TEXT NOT NULL,
                item_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (scope, on_click)
            );
            INSERT INTO pins (scope, on_click, item_json) VALUES
                ('a', 'run:a', '{\"title\":\"A\"}'),
                ('a', 'run:b', '{\"title\":\"B\"}');",
        )
        .unwrap();

        migrate_pins(&conn).unwrap();

        assert!(column_exists(&conn, "pins", "id").unwrap());
        let (count, max_id): (i64, i64) = conn
            .query_row("SELECT count(*), coalesce(max(id), 0) FROM pins", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(max_id, 2, "ids are assigned in insertion order");
        // the (scope, on_click) uniqueness survives the rewrite
        assert!(
            conn.execute(
                "INSERT INTO pins (scope, on_click, item_json) VALUES ('a','run:a','{}')",
                [],
            )
            .is_err()
        );
    }
}
