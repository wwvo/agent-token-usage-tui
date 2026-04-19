//! Integration test: Codex collector against a committed fixture.
//!
//! The fixture at `tests/fixtures/codex/simple.jsonl` exercises:
//! * 1 `session_meta` seeding id / cwd / cli_version
//! * 2 `turn_context` entries switching model mid-session
//!   (`gpt-5-codex` → `gpt-5-codex-mini`)
//! * 3 real user `response_item`s + 1 `function_call_output` (filtered)
//!   + 1 assistant item (filtered)
//! * 3 `event_msg` `token_count` entries (two under the first model, one
//!   under the second)
//! * 1 non-`token_count` event_msg (filtered)
//!
//! That gives:
//!
//! * `usage_records` = 3 — with non-overlapping input correction on each
//! * `prompt_events` = 3 — function_call_output and assistant are not prompts
//! * `sessions_touched` = 1
//!
//! The test also verifies the model attribution: the third token_count must
//! record `gpt-5-codex-mini`, not the original `gpt-5-codex`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use agent_token_usage_tui::collector::CodexCollector;
use agent_token_usage_tui::collector::Collector;
use agent_token_usage_tui::collector::NoopReporter;
use agent_token_usage_tui::storage::Db;

const FIXTURE_PATH: &str = "tests/fixtures/codex/simple.jsonl";

fn stage_fixture() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let sessions = tmp.path().join("sessions").join("2026").join("04");
    std::fs::create_dir_all(&sessions).expect("mkdir sessions");

    let fixture_bytes = std::fs::read(Path::new(FIXTURE_PATH))
        .unwrap_or_else(|e| panic!("read fixture {FIXTURE_PATH}: {e}"));
    std::fs::write(sessions.join("rollout-codex-sess-1.jsonl"), fixture_bytes)
        .expect("write fixture");

    let base = tmp.path().to_path_buf();
    (tmp, base)
}

fn new_db(dir: &Path) -> Db {
    Db::open(&dir.join("data.db")).expect("open db")
}

#[tokio::test]
async fn full_scan_counts_match_expected() {
    let (tmp, base) = stage_fixture();
    let db = new_db(tmp.path());

    let collector = CodexCollector::new(vec![base]);
    let summary = collector
        .scan(&db, &NoopReporter)
        .await
        .expect("scan should succeed");

    assert_eq!(summary.records_inserted, 3);
    assert_eq!(summary.prompts_inserted, 3);
    assert_eq!(summary.sessions_touched, 1);
    assert_eq!(summary.files_scanned, 1);
    assert!(summary.errors.is_empty(), "no per-file errors expected");
}

#[tokio::test]
async fn model_attribution_follows_latest_turn_context() {
    // Query the DB after scan to verify that usage rows under each model are
    // attributed to the *current* turn_context model, not the session's first.
    let (tmp, base) = stage_fixture();
    let db = new_db(tmp.path());
    let collector = CodexCollector::new(vec![base]);
    let _ = collector.scan(&db, &NoopReporter).await.expect("scan");

    let conn = db.lock();
    let mut stmt = conn
        .prepare("SELECT model, COUNT(*) FROM usage_records GROUP BY model ORDER BY model")
        .expect("prepare");
    let iter = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .expect("query_map");

    let mut rows: Vec<(String, i64)> = Vec::new();
    for row in iter {
        rows.push(row.expect("row"));
    }

    assert_eq!(
        rows,
        vec![
            ("gpt-5-codex".to_owned(), 2),
            ("gpt-5-codex-mini".to_owned(), 1),
        ]
    );
}

#[tokio::test]
async fn non_overlapping_input_correction_applied_in_db() {
    // Row 1 upstream: input=100 cached=20 → stored input must be 80.
    // Row 2 upstream: input=150 cached=50 → stored input must be 100.
    // Row 3 upstream: input=80 cached=10 → stored input must be 70.
    let (tmp, base) = stage_fixture();
    let db = new_db(tmp.path());
    let collector = CodexCollector::new(vec![base]);
    let _ = collector.scan(&db, &NoopReporter).await.expect("scan");

    let conn = db.lock();
    let mut stmt = conn
        .prepare(
            "SELECT input_tokens, cache_read_input_tokens, output_tokens \
             FROM usage_records ORDER BY timestamp ASC",
        )
        .expect("prepare");
    let iter = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .expect("query_map");

    let mut rows: Vec<(i64, i64, i64)> = Vec::new();
    for row in iter {
        rows.push(row.expect("row"));
    }

    assert_eq!(
        rows,
        vec![(80, 20, 50), (100, 50, 75), (70, 10, 40)],
        "input_tokens must be raw minus cached for every row",
    );
}

#[tokio::test]
async fn ten_rescans_in_a_row_insert_zero_rows() {
    let (tmp, base) = stage_fixture();
    let db = new_db(tmp.path());
    let collector = CodexCollector::new(vec![base]);

    let first = collector
        .scan(&db, &NoopReporter)
        .await
        .expect("first scan");
    assert_eq!(first.records_inserted, 3);
    assert_eq!(first.prompts_inserted, 3);

    for i in 1..=10 {
        let r = collector.scan(&db, &NoopReporter).await.expect("rescan");
        assert_eq!(r.records_inserted, 0, "rescan #{i} inserted new rows");
        assert_eq!(r.prompts_inserted, 0, "rescan #{i} inserted new prompts");
        assert_eq!(r.sessions_touched, 0, "rescan #{i} touched sessions");
    }
}
