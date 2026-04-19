//! Sidecar tests for `pipeline::run_scan`.

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::PipelineConfig;
use super::run_scan;
use crate::collector::NoopReporter;
use crate::domain::Source;
use crate::storage::Db;

#[test]
fn pipeline_config_default_is_all_empty() {
    let c = PipelineConfig::default();
    assert!(c.openclaw_bases.is_empty());
    assert!(c.opencode_dbs.is_empty());
    assert!(c.windsurf_bases.is_empty());
}

#[tokio::test]
async fn run_scan_returns_five_summaries_in_declared_order() {
    // Point HOME at an empty dir so Claude/Codex find nothing; OpenClaw/
    // OpenCode/Windsurf come from PipelineConfig which we leave empty.
    let tmp = tempdir().expect("tempdir");
    // Scope the HOME override to this test with a best-effort set.
    // SAFETY: tests in this binary run single-threaded via `serial` macros
    // normally, but we don't use that here. On env pollution risk the
    // CLI integration test covers the real `atut scan` path.
    unsafe {
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("USERPROFILE", tmp.path());
    }

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let config = PipelineConfig::default();

    let report = run_scan(&db, &NoopReporter, &config)
        .await
        .expect("pipeline scan");

    assert_eq!(report.summaries.len(), 5);
    assert_eq!(report.summaries[0].source, Source::Claude);
    assert_eq!(report.summaries[1].source, Source::Codex);
    assert_eq!(report.summaries[2].source, Source::OpenClaw);
    assert_eq!(report.summaries[3].source, Source::OpenCode);
    assert_eq!(report.summaries[4].source, Source::Windsurf);
    assert_eq!(report.costs_recalculated, 0, "empty pricing table");
    for s in &report.summaries {
        assert_eq!(s.records_inserted, 0);
        assert_eq!(s.prompts_inserted, 0);
    }
}
