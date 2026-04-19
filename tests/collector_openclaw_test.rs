//! Integration test: OpenClaw collector against a committed fixture.
//!
//! The fixture lives at `tests/fixtures/openclaw/demo-agent/sessions/s1.jsonl`
//! so the `project` column is driven by the `demo-agent` directory name —
//! that mirrors production layout where the first subdir under the base is
//! the agent / project slug.
//!
//! Fixture summary (8 entries):
//! * 1 `session` header
//! * 2 real user messages (plain string + tool_result-bearing variant)
//! * 2 assistant messages with full usage (claude-sonnet-4-5)
//! * 1 assistant `delivery-mirror` (must be filtered)
//! * 1 assistant without usage (must be filtered)
//! * 1 tool_result user (must be filtered)
//!
//! Expected: records=2, prompts=2, sessions=1.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use agent_token_usage_tui::collector::Collector;
use agent_token_usage_tui::collector::NoopReporter;
use agent_token_usage_tui::collector::OpenClawCollector;
use agent_token_usage_tui::storage::Db;

const FIXTURE_PATH: &str = "tests/fixtures/openclaw/demo-agent/sessions/s1.jsonl";

/// Stage the fixture under `<tmp>/demo-agent/sessions/s1.jsonl` and hand the
/// collector `<tmp>` as the base — its contract is that immediate children of
/// the base are agent / project directories.
fn stage_fixture_base() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let sessions = tmp.path().join("demo-agent").join("sessions");
    std::fs::create_dir_all(&sessions).expect("mkdir sessions");

    let fixture_bytes = std::fs::read(Path::new(FIXTURE_PATH))
        .unwrap_or_else(|e| panic!("read fixture {FIXTURE_PATH}: {e}"));
    std::fs::write(sessions.join("s1.jsonl"), fixture_bytes).expect("write fixture");

    let base = tmp.path().to_path_buf();
    (tmp, base)
}

fn new_db(dir: &Path) -> Db {
    Db::open(&dir.join("data.db")).expect("open db")
}

#[tokio::test]
async fn full_scan_counts_match_expected() {
    let (tmp, base) = stage_fixture_base();
    let db = new_db(tmp.path());

    let collector = OpenClawCollector::new(vec![base]);
    let summary = collector
        .scan(&db, &NoopReporter)
        .await
        .expect("scan should succeed");

    assert_eq!(summary.records_inserted, 2);
    assert_eq!(summary.prompts_inserted, 2);
    assert_eq!(summary.sessions_touched, 1);
    assert_eq!(summary.files_scanned, 1);
    assert!(summary.errors.is_empty(), "no per-file errors");
}

#[tokio::test]
async fn project_column_is_populated_from_agent_dir_name() {
    let (tmp, base) = stage_fixture_base();
    let db = new_db(tmp.path());
    let c = OpenClawCollector::new(vec![base]);
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

    assert_eq!(projects, vec!["demo-agent".to_owned()]);
}

#[tokio::test]
async fn cache_buckets_preserve_both_read_and_write() {
    let (tmp, base) = stage_fixture_base();
    let db = new_db(tmp.path());
    let c = OpenClawCollector::new(vec![base]);
    let _ = c.scan(&db, &NoopReporter).await.expect("scan");

    let conn = db.lock();
    let mut stmt = conn
        .prepare(
            "SELECT input_tokens, cache_read_input_tokens, cache_creation_input_tokens \
             FROM usage_records ORDER BY timestamp ASC",
        )
        .expect("prepare");
    let iter = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .expect("query");

    let mut rows: Vec<(i64, i64, i64)> = Vec::new();
    for row in iter {
        rows.push(row.expect("row"));
    }

    // Fixture row 1: input=100 cacheRead=20 cacheWrite=30
    // Fixture row 2: input=80  cacheRead=10 cacheWrite=15
    assert_eq!(rows, vec![(100, 20, 30), (80, 10, 15)]);
}

#[tokio::test]
async fn rescan_is_idempotent() {
    let (tmp, base) = stage_fixture_base();
    let db = new_db(tmp.path());
    let c = OpenClawCollector::new(vec![base]);

    let first = c.scan(&db, &NoopReporter).await.expect("first");
    assert_eq!(first.records_inserted, 2);
    assert_eq!(first.prompts_inserted, 2);

    for i in 1..=5 {
        let r = c.scan(&db, &NoopReporter).await.expect("rescan");
        assert_eq!(r.records_inserted, 0, "rescan #{i}");
        assert_eq!(r.prompts_inserted, 0, "rescan #{i}");
    }
}
