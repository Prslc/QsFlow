use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::system::db::with_db;

/// Pin one row to the top of one exact query (`""` is the empty-query history).
/// The whole item is stored, re-emitted before its plugin runs.
pub fn pin(scope: &str, item_json: &str) -> Result<()> {
    with_db(|conn| pin_with(conn, scope, item_json))
}

/// Drop one pin, reporting whether one was really there.
pub fn unpin(scope: &str, on_click: &str) -> Result<bool> {
    with_db(|conn| unpin_with(conn, scope, on_click))
}

/// Every pin of one scope, most recently pinned first.
pub fn get_pins(scope: &str) -> Result<Vec<serde_json::Value>> {
    with_db(|conn| get_pins_with(conn, scope))
}

fn pin_with(conn: &Connection, scope: &str, item_json: &str) -> Result<()> {
    let item: serde_json::Value = serde_json::from_str(item_json)?;
    let on_click = item["on_click"].as_str().context("item missing on_click")?;
    // Delete-and-insert, not an upsert: a re-pin must get a fresh `id` so it
    // rises to the top of `id`-descending order.
    conn.execute(
        "DELETE FROM pins WHERE scope = ?1 AND on_click = ?2",
        rusqlite::params![scope, on_click],
    )?;
    conn.execute(
        "INSERT INTO pins (scope, on_click, item_json, created_at)
         VALUES (?1, ?2, ?3, datetime('now'))",
        rusqlite::params![scope, on_click, item_json],
    )?;
    Ok(())
}

fn unpin_with(conn: &Connection, scope: &str, on_click: &str) -> Result<bool> {
    let deleted = conn.execute(
        "DELETE FROM pins WHERE scope = ?1 AND on_click = ?2",
        rusqlite::params![scope, on_click],
    )?;
    Ok(deleted > 0)
}

fn get_pins_with(conn: &Connection, scope: &str) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT item_json FROM pins WHERE scope = ?1
         ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([scope], |row| row.get::<_, String>(0))?;
    // Same heal the history has: a corrupt row is skipped rather than emitted as
    // a null that would make the shell reject the whole payload.
    Ok(rows
        .filter_map(Result::ok)
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .filter(|value| value.get("title").and_then(|t| t.as_str()).is_some())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::system::db::init_schema(&conn);
        conn
    }

    #[test]
    fn pins_round_trip_per_scope() {
        let conn = test_conn();
        pin_with(
            &conn,
            "b firefox",
            r#"{"title":"GitHub","on_click":"https://github.com"}"#,
        )
        .unwrap();
        pin_with(
            &conn,
            "b firefox",
            r#"{"title":"Docs","on_click":"https://docs.rs"}"#,
        )
        .unwrap();
        pin_with(
            &conn,
            "",
            r#"{"title":"Files","on_click":"launch:files.desktop"}"#,
        )
        .unwrap();

        // a scope sees only its own pins, most recent first
        let items = get_pins_with(&conn, "b firefox").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["title"], "Docs");
        assert_eq!(get_pins_with(&conn, "").unwrap().len(), 1);

        assert!(unpin_with(&conn, "b firefox", "https://github.com").unwrap());
        assert!(!unpin_with(&conn, "b firefox", "https://github.com").unwrap());
        assert_eq!(get_pins_with(&conn, "b firefox").unwrap().len(), 1);
    }

    #[test]
    fn pinning_the_same_target_moves_it_to_the_front() {
        let conn = test_conn();
        pin_with(&conn, "gh", r#"{"title":"A","on_click":"run:a"}"#).unwrap();
        pin_with(&conn, "gh", r#"{"title":"B","on_click":"run:b"}"#).unwrap();
        pin_with(&conn, "gh", r#"{"title":"A v2","on_click":"run:a"}"#).unwrap();

        let items = get_pins_with(&conn, "gh").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["title"], "A v2", "the re-pin wins and surfaces");
        assert_eq!(items[1]["title"], "B");
    }

    #[test]
    fn a_pin_without_a_target_is_rejected() {
        let conn = test_conn();
        assert!(pin_with(&conn, "gh", r#"{"title":"no target"}"#).is_err());
    }
}
