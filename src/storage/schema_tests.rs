//! Sidecar tests for [`schema::migrate`].

use pretty_assertions::assert_eq;
use rusqlite::Connection;

use super::migrate;

/// Utility: list table names in alphabetical order.
fn table_names(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("prepare tables query");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("run tables query");
    rows.filter_map(Result::ok).collect()
}

#[test]
fn migrate_creates_all_expected_tables() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate succeeds");

    let tables = table_names(&conn);
    for expected in [
        "file_state",
        "meta",
        "pricing",
        "prompt_events",
        "sessions",
        "usage_records",
        "windsurf_sessions",
    ] {
        assert!(
            tables.iter().any(|t| t == expected),
            "expected table {expected} in {tables:?}",
        );
    }
}

#[test]
fn migrate_is_idempotent() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("first migrate");
    migrate(&conn).expect("second migrate must be a no-op");

    // Re-run list: should still be the same set of tables.
    let tables = table_names(&conn);
    let unique: std::collections::HashSet<&String> = tables.iter().collect();
    assert_eq!(tables.len(), unique.len(), "no duplicate tables");
}

#[test]
fn migrate_records_state_in_meta_table() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate");

    for id in ["migration_001_init", "migration_002_windsurf_sessions"] {
        let value: String = conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [id], |row| {
                row.get(0)
            })
            .unwrap_or_else(|e| panic!("meta row for {id} must exist: {e}"));
        assert_eq!(value, "done", "{id} must be marked done");
    }
}

#[test]
fn migrate_creates_windsurf_sessions_index() {
    // Regression guard for the new per-cascade drill-down path: the
    // `last_seen DESC` index is the primary access pattern the future
    // TUI view relies on, so losing it would silently turn "SELECT …
    // ORDER BY last_seen DESC" into a table scan on the busy collector
    // path.
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate");

    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type = 'index' AND name = 'idx_windsurf_sessions_last_seen'",
            [],
            |row| row.get(0),
        )
        .expect("query index existence");
    assert_eq!(exists, 1, "windsurf_sessions last_seen index missing");
}

#[test]
fn migrate_creates_dedup_indices() {
    let conn = Connection::open_in_memory().expect("in-memory db");
    migrate(&conn).expect("migrate");

    // Query sqlite_master for unique indices that the dedup logic relies on.
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND name LIKE 'idx_%_dedup'",
        )
        .expect("prepare indices query");
    let indices: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query indices")
        .filter_map(Result::ok)
        .collect();

    assert!(indices.contains(&"idx_usage_dedup".to_owned()));
    assert!(indices.contains(&"idx_prompt_dedup".to_owned()));
}
