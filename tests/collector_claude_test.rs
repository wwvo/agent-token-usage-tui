//! Integration test: Claude collector against a committed fixture.
//!
//! The fixture lives at `tests/fixtures/claude/simple.jsonl` so reviewers can
//! read it directly and future schema changes are visible in `git diff`.
//!
//! The sidecar tests in `src/collector/claude_tests.rs` cover parse behavior
//! at the unit level; this test locks the **public API contract** (the flow a
//! CLI / TUI implementation will use):
//!
//! 1. Open a `Db`.
//! 2. Construct a `ClaudeCollector` against a bases list.
//! 3. `.scan(&db, &NoopReporter).await` and inspect the returned summary.
//! 4. Repeat the scan → dedup + file_state prevent duplicate inserts.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use agent_token_usage_tui::collector::ClaudeCollector;
use agent_token_usage_tui::collector::Collector;
use agent_token_usage_tui::collector::NoopReporter;
use agent_token_usage_tui::storage::Db;

const FIXTURE_PATH: &str = "tests/fixtures/claude/simple.jsonl";

/// Stage the fixture under `<tmp>/projects/<some-hash>/<session>.jsonl`,
/// returning the temp dir and the Claude base path to hand to the collector.
fn stage_fixture() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let projects = tmp.path().join("projects").join("demo-proj");
    std::fs::create_dir_all(&projects).expect("mkdir projects");

    let fixture_bytes = std::fs::read(Path::new(FIXTURE_PATH))
        .unwrap_or_else(|e| panic!("read fixture {FIXTURE_PATH}: {e}"));
    std::fs::write(projects.join("session-demo.jsonl"), fixture_bytes)
        .expect("write fixture to temp");

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

    let collector = ClaudeCollector::new(vec![base]);
    let summary = collector
        .scan(&db, &NoopReporter)
        .await
        .expect("scan should succeed");

    // Fixture contains:
    // * 3 real user prompts (prompt_events)
    // * 3 assistant-with-usage (usage_records)
    // * 1 tool_result user (filtered)
    // * 1 <synthetic> (filtered)
    // * 1 streaming chunk w/o usage (filtered)
    assert_eq!(summary.records_inserted, 3);
    assert_eq!(summary.prompts_inserted, 3);
    assert_eq!(summary.sessions_touched, 1);
    assert_eq!(summary.files_scanned, 1);
    assert!(summary.errors.is_empty(), "no per-file errors expected");
}

#[tokio::test]
async fn ten_rescans_in_a_row_insert_zero_rows() {
    let (tmp, base) = stage_fixture();
    let db = new_db(tmp.path());
    let collector = ClaudeCollector::new(vec![base]);

    // First scan establishes file_state + seeds DB.
    let first = collector
        .scan(&db, &NoopReporter)
        .await
        .expect("first scan");
    assert_eq!(first.records_inserted, 3);
    assert_eq!(first.prompts_inserted, 3);

    // Subsequent scans must be completely idempotent.
    for i in 1..=10 {
        let r = collector.scan(&db, &NoopReporter).await.expect("rescan");
        assert_eq!(
            r.records_inserted, 0,
            "rescan #{i} inserted new usage rows unexpectedly",
        );
        assert_eq!(
            r.prompts_inserted, 0,
            "rescan #{i} inserted new prompts unexpectedly",
        );
        assert_eq!(
            r.sessions_touched, 0,
            "rescan #{i} touched sessions without any new data",
        );
    }
}
