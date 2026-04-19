//! Sidecar tests for OpenCode collector.
//!
//! Full end-to-end scanning against a real (in-memory) OpenCode-shaped
//! SQLite lives in `tests/collector_opencode_test.rs`; here we only cover
//! the small pure helpers that don't need a DB.

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::OpenCodeCollector;
use crate::collector::Collector;
use crate::collector::NoopReporter;
use crate::domain::Source;
use crate::storage::Db;

#[test]
fn source_is_opencode() {
    assert_eq!(
        OpenCodeCollector::new(Vec::new()).source(),
        Source::OpenCode
    );
}

#[test]
fn millis_to_ts_roundtrips_utc_epoch() {
    // Dynamically derive the expected millis so we don't re-fat-finger the
    // conversion (the earlier hard-coded literal was off by four months).
    let want = chrono::DateTime::parse_from_rfc3339("2026-04-19T10:00:00Z").unwrap();
    let ts = OpenCodeCollector::millis_to_ts(want.timestamp_millis());
    assert_eq!(ts.to_rfc3339(), "2026-04-19T10:00:00+00:00");
}

#[test]
fn millis_to_ts_handles_zero() {
    // Unix epoch (0 ms) must return a valid timestamp, not today's `Utc::now`.
    let ts = OpenCodeCollector::millis_to_ts(0);
    assert_eq!(ts.to_rfc3339(), "1970-01-01T00:00:00+00:00");
}

#[tokio::test]
async fn scan_missing_paths_returns_empty_summary() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("t.db")).expect("open");

    let c = OpenCodeCollector::new(vec![tmp.path().join("missing.db")]);
    let summary = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(summary.files_scanned, 0);
    assert_eq!(summary.records_inserted, 0);
}
