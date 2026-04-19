//! Sidecar tests for the Windsurf placeholder collector.

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::WindsurfCollector;
use crate::collector::Collector;
use crate::collector::NoopReporter;
use crate::domain::Source;
use crate::storage::Db;

#[test]
fn source_is_windsurf() {
    assert_eq!(
        WindsurfCollector::new(Vec::new()).source(),
        Source::Windsurf
    );
}

#[tokio::test]
async fn scan_always_returns_empty_summary() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("t.db")).expect("open");

    // Even if the caller passed a plausible-looking base path, we don't scan.
    let c = WindsurfCollector::new(vec![tmp.path().to_path_buf()]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(s.source, Source::Windsurf);
    assert_eq!(s.files_scanned, 0);
    assert_eq!(s.records_inserted, 0);
    assert_eq!(s.prompts_inserted, 0);
    assert_eq!(s.sessions_touched, 0);
    assert!(s.errors.is_empty());
}
