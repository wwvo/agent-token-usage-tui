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
    // OpenCode come from PipelineConfig which we leave empty. For Windsurf
    // we drop a minimal fixture at the exporter's default output path
    // (`<HOME>/.atut/windsurf-sessions/`) so the test *also* guards the
    // pipeline's empty-`windsurf_bases` → `with_default_paths()` fallback:
    // if somebody regresses `pipeline.rs` back to passing the empty vec
    // straight through, `records_inserted` for Windsurf drops to 0 and
    // this test fails.
    let tmp = tempdir().expect("tempdir");
    // Scope the HOME override to this test with a best-effort set.
    // SAFETY: tests in this binary run single-threaded via `serial` macros
    // normally, but we don't use that here. On env pollution risk the
    // CLI integration test covers the real `atut scan` path. We also
    // clear `ATUT_WINDSURF_SESSIONS_DIR` so a developer machine that
    // happens to export it doesn't steer `with_default_paths()` away
    // from the HOME-relative fallback we rely on below.
    unsafe {
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("USERPROFILE", tmp.path());
        std::env::remove_var("ATUT_WINDSURF_SESSIONS_DIR");
    }

    let windsurf_dir = tmp.path().join(".atut").join("windsurf-sessions");
    std::fs::create_dir_all(&windsurf_dir).expect("mkdir windsurf-sessions");
    let fixture = windsurf_dir.join("fixture.jsonl");
    std::fs::write(
        &fixture,
        concat!(
            r#"{"type":"session_meta","cascade_id":"pipeline-test","created_time":"2026-04-19T00:00:00Z","summary":"test","last_model":"claude-opus","workspace":""}"#,
            "\n",
            r#"{"type":"turn_usage","step_id":"turn-0","timestamp":"2026-04-19T00:00:00Z","model":"claude-opus","input_tokens":100,"output_tokens":50,"cached_input_tokens":0}"#,
            "\n",
        ),
    )
    .expect("write windsurf fixture");

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

    // Claude / Codex / OpenClaw / OpenCode: nothing to find.
    for s in &report.summaries[..4] {
        assert_eq!(s.records_inserted, 0);
        assert_eq!(s.prompts_inserted, 0);
    }

    // Windsurf: the pipeline saw our fixture via the default-paths
    // fallback. A value of 0 here means `pipeline.rs` stopped calling
    // `WindsurfCollector::with_default_paths()` on empty config.
    assert_eq!(
        report.summaries[4].records_inserted, 1,
        "empty `windsurf_bases` must fall back to ~/.atut/windsurf-sessions/",
    );
}
