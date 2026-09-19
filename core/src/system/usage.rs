use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, PoisonError};

use crate::system::fs::get_home;

fn db_path() -> Result<PathBuf> {
    let dir = get_home()?.join(".local/share/wayrun");
    std::fs::create_dir_all(&dir).context("Failed to create wayrun data directory")?;
    Ok(dir.join("usage.db"))
}

fn open_conn() -> Result<Connection> {
    let conn = Connection::open(db_path()?)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage (
            key TEXT PRIMARY KEY,
            on_click TEXT,
            count INTEGER NOT NULL DEFAULT 1,
            last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
            item_json TEXT NOT NULL
        );",
    )?;
    migrate(&conn)?;
    Ok(conn)
}

/// One connection for the process lifetime: an empty query calls `get_top` on
/// every keystroke. Opened lazily, and left unset after a failure so a later
/// call retries: a missing `$HOME`, a read-only dir or a full disk must degrade
/// history to empty rather than panic the core.
static DB: LazyLock<Mutex<Option<Connection>>> = LazyLock::new(|| Mutex::new(None));

/// Run `f` on the shared connection, surviving lock poisoning.
fn with_db<T>(f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let mut guard = DB.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_none() {
        *guard = Some(open_conn()?);
    }
    f(guard.as_ref().expect("opened just above"))
}

/// Migrate a database that lacks the `on_click` column, merging same-title
/// rows (counts sum, the most recent item wins) so one app reached via
/// `run:<exec>` and `launch:<id>` is a single entry.
fn migrate(conn: &Connection) -> Result<()> {
    let has_on_click: bool = {
        let mut stmt = conn.prepare("PRAGMA table_info(usage)")?;
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        names.iter().any(|n| n == "on_click")
    };
    if has_on_click {
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

pub fn record(item_json: &str) -> Result<()> {
    with_db(|conn| record_with(conn, item_json))
}

/// Drop one history entry, reporting whether a row was really there.
pub fn forget(on_click: &str) -> Result<bool> {
    with_db(|conn| forget_with(conn, on_click))
}

pub fn get_top(limit: i32) -> Result<Vec<serde_json::Value>> {
    with_db(|conn| get_top_with(conn, limit))
}

#[cfg(test)]
fn init_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage (
            key TEXT PRIMARY KEY,
            on_click TEXT,
            count INTEGER NOT NULL DEFAULT 1,
            last_used_at TEXT NOT NULL DEFAULT (datetime('now')),
            item_json TEXT NOT NULL
        );",
    )
    .ok();
}

/// A row the host marked `ephemeral`, or one whose `on_click` is a clipboard
/// write (`copy:`), is not a re-launchable target: the first is a deliberate
/// choice (a one-shot search hit, say), the second carries the copied text
/// rather than something to re-open. Neither belongs in the usage-ranked
/// history. The field/scheme declares the semantics, so this holds for every
/// source — built-in providers and external JSON-RPC hosts alike.
fn is_ephemeral(item: &serde_json::Value) -> bool {
    item["ephemeral"].as_bool().unwrap_or(false)
        || item["on_click"]
            .as_str()
            .is_some_and(|on_click| on_click.starts_with("copy:"))
}

fn record_with(conn: &Connection, item_json: &str) -> Result<()> {
    let item: serde_json::Value = serde_json::from_str(item_json)?;
    if is_ephemeral(&item) {
        return Ok(());
    }
    let on_click = item["on_click"].as_str().context("item missing on_click")?;

    // Key by display title so alternate launch actions for one app merge
    // into a single entry; empty titles fall back to the action.
    let title = item["title"].as_str().context("item missing title")?;
    let key = if title.is_empty() {
        on_click.to_string()
    } else {
        title.to_string()
    };

    conn.execute(
        "INSERT INTO usage (key, on_click, count, last_used_at, item_json)
         VALUES (?1, ?2, 1, datetime('now'), ?3)
         ON CONFLICT(key) DO UPDATE SET
             count = count + 1,
             last_used_at = datetime('now'),
             on_click = ?2,
             item_json = ?3",
        rusqlite::params![key, on_click, item_json],
    )?;
    Ok(())
}

/// Drop one history entry. Callers pass the row's `on_click`; resolve it to
/// the title-keyed entry first so a merged row (same title, several actions)
/// is removed whole instead of leaving its siblings behind as ghosts. `true`
/// when a row was actually deleted (`forget`'s answer rides on this).
fn forget_with(conn: &Connection, on_click: &str) -> Result<bool> {
    let title: Option<String> = conn
        .query_row(
            "SELECT key FROM usage WHERE on_click = ?1 LIMIT 1",
            [on_click],
            |r| r.get(0),
        )
        .ok();
    let deleted = match title {
        Some(t) => conn.execute("DELETE FROM usage WHERE key = ?1", [t])?,
        // legacy keyed-by-action rows that never went through migrate
        None => conn.execute("DELETE FROM usage WHERE key = ?1", [on_click])?,
    };
    Ok(deleted > 0)
}

/// Drop `copy:` rows, which are not re-launchable targets. Runs at core
/// startup so an older database heals on upgrade; idempotent.
pub fn purge_ephemeral() -> Result<()> {
    with_db(purge_with)
}

fn purge_with(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM usage WHERE on_click LIKE 'copy:%'", [])?;
    Ok(())
}

fn get_top_with(conn: &Connection, limit: i32) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT item_json FROM usage ORDER BY count DESC, last_used_at DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit], |row| row.get::<_, String>(0))?;

    // A corrupt row, or one with no `title` at all, is skipped, never emitted
    // as `null`: `title` is required on the wire, so one bad entry would make
    // the shell reject the whole history array. An empty title is a real row.
    Ok(rows
        .filter_map(Result::ok)
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .filter(|value| value.get("title").and_then(|t| t.as_str()).is_some())
        .collect())
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
    fn a_corrupt_or_titleless_row_does_not_poison_the_history() {
        let conn = test_conn();
        record_with(&conn, r#"{"title":"Good","on_click":"run:good"}"#).unwrap();
        conn.execute(
            "INSERT INTO usage (key, on_click, item_json) VALUES ('bad', 'run:bad', 'not json')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage (key, on_click, item_json) VALUES ('empty', 'run:empty', '{\"on_click\":\"run:empty\"}')",
            [],
        )
        .unwrap();

        let items = get_top_with(&conn, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Good");
    }

    #[test]
    fn forget_removes_entry() {
        let conn = test_conn();
        record_with(&conn, r#"{"title":"A","on_click":"run:a"}"#).unwrap();
        assert_eq!(get_top_with(&conn, 10).unwrap().len(), 1);

        assert!(forget_with(&conn, "run:a").unwrap(), "a row was there");
        assert!(get_top_with(&conn, 10).unwrap().is_empty());
        assert!(
            !forget_with(&conn, "run:a").unwrap(),
            "nothing left to drop, so `forget` answers false"
        );
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

    #[test]
    fn same_title_actions_merge_into_one_entry() {
        let conn = test_conn();
        // both on_click forms for the same app
        record_with(
            &conn,
            r#"{"title":"Telegram","on_click":"run:Telegram --"}"#,
        )
        .unwrap();
        record_with(
            &conn,
            r#"{"title":"Telegram","on_click":"launch:org.telegram.desktop.desktop"}"#,
        )
        .unwrap();
        record_with(
            &conn,
            r#"{"title":"Telegram","on_click":"launch:org.telegram.desktop.desktop"}"#,
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT count FROM usage WHERE key = 'Telegram'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 3);

        let items = get_top_with(&conn, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Telegram");
        // the merged entry keeps the most recently used action
        assert!(
            items[0]["on_click"]
                .as_str()
                .unwrap()
                .starts_with("launch:")
        );
    }

    #[test]
    fn forget_removes_merged_siblings() {
        let conn = test_conn();
        record_with(
            &conn,
            r#"{"title":"Telegram","on_click":"run:Telegram --"}"#,
        )
        .unwrap();
        record_with(
            &conn,
            r#"{"title":"Telegram","on_click":"launch:org.telegram.desktop.desktop"}"#,
        )
        .unwrap();

        // ⌫ passes the current launch: action — the whole merged entry goes
        forget_with(&conn, "launch:org.telegram.desktop.desktop").unwrap();
        assert!(get_top_with(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn empty_title_falls_back_to_action() {
        let conn = test_conn();
        record_with(&conn, r#"{"title":"","on_click":"run:a"}"#).unwrap();
        record_with(&conn, r#"{"title":"","on_click":"run:b"}"#).unwrap();
        assert_eq!(get_top_with(&conn, 10).unwrap().len(), 2);
    }

    #[test]
    fn one_shot_rows_are_never_recorded() {
        let conn = test_conn();
        for (title, on_click, ephemeral) in [
            ("Clipboard", r#"copy:{"text": "clip"}"#, false),
            ("github hit", "https://github.com/Prslc/WayRun", true),
        ] {
            record_with(
                &conn,
                &serde_json::json!({
                    "title": title,
                    "on_click": on_click,
                    "ephemeral": ephemeral,
                })
                .to_string(),
            )
            .unwrap();
        }
        // A URL the host did not mark, plus a launch, a command and a desktop
        // action, are all re-launchable targets.
        for (title, on_click) in [
            ("Prslc/WayRun", "https://github.com/Prslc/WayRun"),
            ("Firefox", "launch:firefox.desktop"),
            ("btop", "run:btop"),
            ("New Window", "action:firefox.desktop:new-window"),
        ] {
            record_with(
                &conn,
                &serde_json::json!({ "title": title, "on_click": on_click }).to_string(),
            )
            .unwrap();
        }

        let mut titles: Vec<String> = get_top_with(&conn, 10)
            .unwrap()
            .iter()
            .filter_map(|item| item["title"].as_str().map(str::to_owned))
            .collect();
        titles.sort();
        assert_eq!(titles, ["Firefox", "New Window", "Prslc/WayRun", "btop"]);
    }

    #[test]
    fn purge_removes_legacy_copy_rows() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO usage (key, on_click, count, last_used_at, item_json)
             VALUES ('copy:{\"text\": \"old\"}', 'copy:{\"text\": \"old\"}', 4,
                     datetime('now'), '{\"title\":\"old\"}')",
            [],
        )
        .unwrap();
        record_with(&conn, r#"{"title":"A","on_click":"run:a"}"#).unwrap();
        record_with(&conn, r#"{"title":"URL","on_click":"https://example.com"}"#).unwrap();

        purge_with(&conn).unwrap();

        let mut titles: Vec<String> = get_top_with(&conn, 10)
            .unwrap()
            .iter()
            .filter_map(|item| item["title"].as_str().map(str::to_owned))
            .collect();
        titles.sort();
        assert_eq!(titles, ["A", "URL"], "a URL row is not ephemeral");
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

        migrate(&conn).unwrap();

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
}
