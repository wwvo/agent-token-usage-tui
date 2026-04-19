//! Sidecar tests for the collector module-level types.

use pretty_assertions::assert_eq;

use super::ScanSummary;
use crate::domain::Source;

#[test]
fn scan_summary_new_is_empty_per_source() {
    let s = ScanSummary::new(Source::Claude);
    assert_eq!(s.source, Source::Claude);
    assert_eq!(s.records_inserted, 0);
    assert_eq!(s.prompts_inserted, 0);
    assert_eq!(s.sessions_touched, 0);
    assert_eq!(s.files_scanned, 0);
    assert!(s.errors.is_empty());
}

#[test]
fn scan_summary_fields_are_mutable() {
    let mut s = ScanSummary::new(Source::Codex);
    s.records_inserted = 5;
    s.prompts_inserted = 3;
    s.sessions_touched = 1;
    s.files_scanned = 2;
    s.errors.push("boom".into());

    assert_eq!(s.records_inserted, 5);
    assert_eq!(s.errors.len(), 1);
}
