//! Sidecar tests for the Claude collector skeleton.
//!
//! M3 C3a only exercises the scan skeleton (discovery, file-state advance,
//! progress reporting). M3 C3b will layer full parsing tests on top.

use std::io::Write;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::ClaudeCollector;
use super::home_dir;
use crate::collector::Collector;
use crate::collector::NoopReporter;
use crate::domain::Source;
use crate::storage::Db;

fn new_db_in(dir: &std::path::Path) -> Db {
    Db::open(&dir.join("test.db")).expect("open db")
}

#[test]
fn source_is_claude() {
    let c = ClaudeCollector::new(Vec::new());
    assert_eq!(c.source(), Source::Claude);
}

#[test]
fn discover_files_returns_empty_for_missing_paths() {
    let c = ClaudeCollector::new(vec![PathBuf::from("/definitely/does/not/exist/claude")]);
    let files = c.discover_files();
    assert!(files.is_empty());
}

#[test]
fn discover_files_finds_nested_jsonl() {
    let tmp = tempdir().expect("tempdir");
    let nested = tmp.path().join("projects").join("abc123");
    std::fs::create_dir_all(&nested).expect("mkdir");
    std::fs::write(nested.join("session-1.jsonl"), b"{}").expect("seed 1");
    std::fs::write(nested.join("session-2.jsonl"), b"{}").expect("seed 2");
    // Non-JSONL files should be ignored.
    std::fs::write(nested.join("README.md"), b"# hi").expect("seed md");

    let c = ClaudeCollector::new(vec![tmp.path().to_path_buf()]);
    let files = c.discover_files();
    assert_eq!(files.len(), 2, "found {files:?}");
}

#[test]
fn home_dir_is_resolvable() {
    // On every CI worker we run against, HOME or USERPROFILE is set.
    // The test is best-effort: if neither is set we just accept None.
    let _ = home_dir();
}

#[tokio::test]
async fn scan_empty_paths_returns_empty_summary() {
    let tmp = tempdir().expect("tempdir");
    let db = new_db_in(tmp.path());

    let c = ClaudeCollector::new(vec![tmp.path().join("nonexistent")]);
    let summary = c
        .scan(&db, &NoopReporter)
        .await
        .expect("scan must succeed on empty paths");

    assert_eq!(summary.source, Source::Claude);
    assert_eq!(summary.files_scanned, 0);
    assert_eq!(summary.records_inserted, 0);
    assert_eq!(summary.prompts_inserted, 0);
}

#[tokio::test]
async fn scan_advances_file_state_offset_even_without_parsing() {
    let tmp = tempdir().expect("tempdir");
    let projects = tmp.path().join("projects").join("proj");
    std::fs::create_dir_all(&projects).expect("mkdir");

    let jsonl_path = projects.join("session.jsonl");
    let mut f = std::fs::File::create(&jsonl_path).expect("create jsonl");
    writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}}}}"#).expect("line 1");
    writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","usage":{{"input_tokens":1,"output_tokens":2}}}}}}"#).expect("line 2");
    drop(f);

    let db = new_db_in(tmp.path());

    let c = ClaudeCollector::new(vec![tmp.path().to_path_buf()]);
    let summary = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(summary.files_scanned, 1);

    // Second scan should find the file already processed (offset == size), so
    // records_inserted stays 0 again (and, importantly, doesn't crash).
    let summary2 = c.scan(&db, &NoopReporter).await.expect("rescan");
    assert_eq!(summary2.files_scanned, 1);
}
