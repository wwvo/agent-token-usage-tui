//! Integration test: OpenCode collector against a fabricated OpenCode DB.
//!
//! We build a minimal OpenCode-shaped SQLite database in a temp directory:
//!
//! ```sql
//! CREATE TABLE session  (id TEXT PRIMARY KEY, directory TEXT);
//! CREATE TABLE message  (session_id TEXT, role TEXT,
//!                        time_created INTEGER, data TEXT);
//! ```
//!
//! Then seed a mix of rows covering every branch in `collector::opencode`:
//! * 1 session row
//! * 3 user messages (plain string role)
//! * 3 assistant messages with non-zero tokens (including reasoning + cache)
//! * 1 assistant with 0/0 tokens (failed call — must be filtered)
//! * 1 assistant with empty modelID (must be filtered)
//! * 1 system message (must be ignored)
//!
//! Expected after scan: records=3, prompts=3, sessions=1.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use rusqlite::Connection;
use tempfile::TempDir;

use agent_token_usage_tui::collector::Collector;
use agent_token_usage_tui::collector::NoopReporter;
use agent_token_usage_tui::collector::OpenCodeCollector;
use agent_token_usage_tui::storage::Db;

fn assistant_data(
    model: &str,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
) -> String {
    format!(
        r#"{{"role":"assistant","modelID":"{model}","tokens":{{"input":{input},"output":{output},"reasoning":0,"cache":{{"read":{cache_read},"write":{cache_write}}}}},"time":{{"created":0,"completed":0}}}}"#
    )
}

fn user_data() -> String {
    r#"{"role":"user","text":"hi"}"#.to_owned()
}

fn build_opencode_db(path: &Path) {
    let conn = Connection::open(path).expect("open opencode sqlite");
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT);
         CREATE TABLE message (session_id TEXT, role TEXT, time_created INTEGER, data TEXT);",
    )
    .expect("create schema");

    conn.execute(
        "INSERT INTO session(id, directory) VALUES('sess-1', '/home/u/proj')",
        [],
    )
    .expect("insert session");

    // Base timestamp 2026-04-19T10:00:00Z = 1766138400000 ms
    // We space them 1000 ms apart so the ORDER BY is deterministic.
    let rows: [(&str, &str, i64, String); 9] = [
        ("sess-1", "user", 1_766_138_400_000, user_data()),
        (
            "sess-1",
            "assistant",
            1_766_138_401_000,
            assistant_data("gpt-5-codex", 100, 50, 20, 30),
        ),
        ("sess-1", "user", 1_766_138_402_000, user_data()),
        (
            "sess-1",
            "assistant",
            1_766_138_403_000,
            assistant_data("gpt-5-codex", 80, 40, 10, 15),
        ),
        ("sess-1", "user", 1_766_138_404_000, user_data()),
        (
            "sess-1",
            "assistant",
            1_766_138_405_000,
            assistant_data("claude-sonnet-4-5", 60, 30, 0, 0),
        ),
        (
            "sess-1",
            "assistant",
            1_766_138_406_000,
            // Failed call: 0/0 tokens → must be filtered out.
            assistant_data("gpt-5-codex", 0, 0, 0, 0),
        ),
        (
            "sess-1",
            "assistant",
            1_766_138_407_000,
            // Empty modelID → must be filtered out.
            assistant_data("", 10, 5, 0, 0),
        ),
        (
            "sess-1",
            "system",
            1_766_138_408_000,
            r#"{"role":"system","text":"sys"}"#.to_owned(),
        ),
    ];

    {
        let mut stmt = conn
            .prepare(
                "INSERT INTO message(session_id, role, time_created, data) \
                 VALUES(?1, ?2, ?3, ?4)",
            )
            .expect("prepare");
        for (sid, role, ts, data) in &rows {
            stmt.execute(rusqlite::params![sid, role, ts, data])
                .expect("insert message");
        }
    }
}

fn stage_fixture() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("opencode.db");
    build_opencode_db(&db_path);
    (tmp, db_path)
}

fn new_our_db(dir: &Path) -> Db {
    Db::open(&dir.join("our.db")).expect("open our db")
}

#[tokio::test]
async fn full_scan_counts_match_expected() {
    let (tmp, oc_path) = stage_fixture();
    let db = new_our_db(tmp.path());

    let c = OpenCodeCollector::new(vec![oc_path]);
    let summary = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(summary.records_inserted, 3, "3 valid assistant rows");
    assert_eq!(summary.prompts_inserted, 3, "3 user messages");
    assert_eq!(summary.sessions_touched, 1);
    assert_eq!(summary.files_scanned, 1);
    assert!(summary.errors.is_empty());
}

#[tokio::test]
async fn rescan_is_idempotent_via_watermark() {
    let (tmp, oc_path) = stage_fixture();
    let db = new_our_db(tmp.path());
    let c = OpenCodeCollector::new(vec![oc_path]);

    let first = c.scan(&db, &NoopReporter).await.expect("first");
    assert_eq!(first.records_inserted, 3);

    // Second scan: watermark is now the max time_created; no new assistant
    // rows means 0 records. User prompt dedup index keeps prompts at 0.
    let second = c.scan(&db, &NoopReporter).await.expect("second");
    assert_eq!(second.records_inserted, 0);
    assert_eq!(second.prompts_inserted, 0);
}

#[tokio::test]
async fn project_column_comes_from_session_directory() {
    let (tmp, oc_path) = stage_fixture();
    let db = new_our_db(tmp.path());
    let c = OpenCodeCollector::new(vec![oc_path]);
    let _ = c.scan(&db, &NoopReporter).await.expect("scan");

    let conn = db.lock();
    let mut stmt = conn
        .prepare("SELECT DISTINCT project FROM usage_records")
        .expect("prepare");
    let iter = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query");

    let mut projects: Vec<String> = Vec::new();
    for row in iter {
        projects.push(row.expect("row"));
    }
    assert_eq!(projects, vec!["/home/u/proj".to_owned()]);
}
