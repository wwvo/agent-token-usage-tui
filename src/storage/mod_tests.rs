//! Sidecar tests for `Db::open` end-to-end.

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Db;

#[test]
fn open_creates_file_and_runs_migrations() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("test.db");

    let db = Db::open(&path).expect("open db");
    assert!(path.exists(), "db file should be created");

    let conn = db.lock();
    let migration_value: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'migration_001_init'",
            [],
            |row| row.get(0),
        )
        .expect("migration state row exists");
    assert_eq!(migration_value, "done");
}

#[test]
fn open_preserves_existing_database_on_reopen() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("test.db");

    // First open creates and migrates.
    {
        let db = Db::open(&path).expect("first open");
        let conn = db.lock();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('user_marker', 'hello')",
            [],
        )
        .expect("seed meta row");
    }

    // Second open must find the marker intact (migrations idempotent, data preserved).
    let db = Db::open(&path).expect("second open");
    let conn = db.lock();
    let marker: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'user_marker'",
            [],
            |r| r.get(0),
        )
        .expect("marker row");
    assert_eq!(marker, "hello");
}

#[test]
fn open_enables_wal_journal_mode() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("test.db");
    let db = Db::open(&path).expect("open db");

    let conn = db.lock();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("query journal_mode");
    assert_eq!(mode.to_lowercase(), "wal");
}
