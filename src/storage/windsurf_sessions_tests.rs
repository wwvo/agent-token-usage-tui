//! Sidecar tests for `windsurf_sessions` upsert + fetch helpers.

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Db;
use super::WindsurfSessionSummary;
use crate::domain::Source;
use crate::domain::UsageRecord;
use crate::domain::WindsurfSessionRecord;

fn ts(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).expect("valid epoch")
}

fn open_db() -> Db {
    let tmp = tempdir().expect("tempdir");
    // `keep()` so the path survives past this fn; the tempdir is cleaned
    // up by the OS on process exit, which is fine for tests.
    let path = tmp.keep().join("t.db");
    Db::open(&path).expect("open db")
}

fn rec(
    cascade: &str,
    summary: &str,
    workspace: &str,
    model: &str,
    created: i64,
    seen: i64,
) -> WindsurfSessionRecord {
    WindsurfSessionRecord {
        cascade_id: cascade.to_owned(),
        summary: summary.to_owned(),
        workspace: workspace.to_owned(),
        last_model: model.to_owned(),
        created_time: Some(ts(created)),
        last_seen: ts(seen),
    }
}

#[test]
fn upsert_then_fetch_roundtrip_single_row() {
    let db = open_db();
    let r = rec(
        "casc-1",
        "Generating commit msg",
        "file:///home/u/proj",
        "gpt-5-codex",
        1_700_000_000,
        1_700_001_000,
    );
    db.upsert_windsurf_session(&r).expect("upsert");

    let rows = db
        .fetch_windsurf_sessions_summary(10)
        .expect("fetch summary");
    assert_eq!(rows.len(), 1);
    let got = &rows[0];
    assert_eq!(got.cascade_id, "casc-1");
    assert_eq!(got.summary, "Generating commit msg");
    assert_eq!(got.workspace, "file:///home/u/proj");
    assert_eq!(got.last_model, "gpt-5-codex");
    assert_eq!(got.created_time, Some(ts(1_700_000_000)));
    assert_eq!(got.last_seen, ts(1_700_001_000));
    // No usage_records yet, so aggregate counters are zero.
    assert_eq!(got.turns, 0);
    assert_eq!(got.total_tokens, 0);
    assert_eq!(got.total_cost_usd, 0.0);
}

#[test]
fn upsert_merges_non_empty_strings_and_keeps_earliest_created_time() {
    let db = open_db();

    // First pass: partial data (empty workspace, created_time known).
    db.upsert_windsurf_session(&rec(
        "casc-1",
        "Title v1",
        "",
        "gpt-5-codex",
        1_700_000_000,
        1_700_001_000,
    ))
    .expect("first");

    // Second pass: workspace now known + later created_time (should be
    // ignored via COALESCE) + later last_seen (should win).
    db.upsert_windsurf_session(&rec(
        "casc-1",
        "Title v2",
        "file:///home/u/proj",
        "",
        1_700_500_000, // later than first — must NOT overwrite
        1_700_002_000,
    ))
    .expect("second");

    let rows = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    assert_eq!(rows.len(), 1);
    let got = &rows[0];
    // Non-empty new values overwrite.
    assert_eq!(got.summary, "Title v2");
    assert_eq!(got.workspace, "file:///home/u/proj");
    // Empty new value preserves old.
    assert_eq!(got.last_model, "gpt-5-codex");
    // first-seen-wins for created_time.
    assert_eq!(got.created_time, Some(ts(1_700_000_000)));
    // max-wins for last_seen.
    assert_eq!(got.last_seen, ts(1_700_002_000));
}

#[test]
fn upsert_accepts_null_created_time() {
    // Older JSONL files sometimes have no `created_time` in the
    // `session_meta` line. The upsert path must accept `None` and leave
    // the column NULL, not crash on the binding.
    let db = open_db();
    let r = WindsurfSessionRecord {
        cascade_id: "casc-null".to_owned(),
        summary: String::new(),
        workspace: String::new(),
        last_model: String::new(),
        created_time: None,
        last_seen: ts(1_700_000_000),
    };
    db.upsert_windsurf_session(&r).expect("upsert null");

    let rows = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].created_time, None);
}

#[test]
fn null_created_time_gets_backfilled_when_later_pass_provides_it() {
    // Contract: first-seen-wins only applies among *non-null* values.
    // A NULL starting point should still accept the first real timestamp.
    let db = open_db();

    db.upsert_windsurf_session(&WindsurfSessionRecord {
        cascade_id: "casc-null".to_owned(),
        summary: String::new(),
        workspace: String::new(),
        last_model: String::new(),
        created_time: None,
        last_seen: ts(1_700_000_000),
    })
    .expect("first");

    db.upsert_windsurf_session(&rec("casc-null", "", "", "", 1_700_500_000, 1_700_001_000))
        .expect("second");

    let rows = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    assert_eq!(rows[0].created_time, Some(ts(1_700_500_000)));
}

#[test]
fn fetch_orders_by_last_seen_desc() {
    let db = open_db();
    db.upsert_windsurf_session(&rec("older", "O", "", "", 1, 100))
        .expect("older");
    db.upsert_windsurf_session(&rec("newer", "N", "", "", 2, 200))
        .expect("newer");
    db.upsert_windsurf_session(&rec("middle", "M", "", "", 3, 150))
        .expect("middle");

    let rows = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    let ids: Vec<&str> = rows.iter().map(|r| r.cascade_id.as_str()).collect();
    assert_eq!(ids, vec!["newer", "middle", "older"]);
}

#[test]
fn fetch_limit_clamps_row_count() {
    let db = open_db();
    for i in 0..5 {
        db.upsert_windsurf_session(&rec(&format!("casc-{i}"), "", "", "", i, 1_700_000_000 + i))
            .expect("upsert");
    }
    let rows = db.fetch_windsurf_sessions_summary(3).expect("fetch");
    assert_eq!(rows.len(), 3);
}

#[test]
fn fetch_summary_joins_usage_records_aggregates() {
    // End-to-end: a cascade with two usage rows should surface their
    // sum as `turns`/`total_tokens`/`total_cost_usd` in the summary.
    let db = open_db();
    db.upsert_windsurf_session(&rec(
        "casc-joined",
        "Joined",
        "",
        "gpt-5-codex",
        1_700_000_000,
        1_700_001_000,
    ))
    .expect("meta");

    db.insert_usage_batch(&[
        UsageRecord {
            source: Source::Windsurf,
            session_id: "casc-joined".into(),
            model: "gpt-5-codex".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: 10,
            cache_read_input_tokens: 5,
            reasoning_output_tokens: 0,
            cost_usd: 0.1234,
            timestamp: ts(1_700_000_500),
            project: "proj".into(),
            git_branch: String::new(),
        },
        UsageRecord {
            source: Source::Windsurf,
            session_id: "casc-joined".into(),
            model: "gpt-5-codex".into(),
            input_tokens: 200,
            output_tokens: 150,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 20,
            reasoning_output_tokens: 0,
            cost_usd: 0.4321,
            timestamp: ts(1_700_000_700),
            project: "proj".into(),
            git_branch: String::new(),
        },
    ])
    .expect("usage batch");

    let rows = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    assert_eq!(rows.len(), 1);
    let got = &rows[0];
    assert_eq!(got.turns, 2);
    // 100+50+10+5 + 200+150+0+20 = 535
    assert_eq!(got.total_tokens, 535);
    assert!(
        (got.total_cost_usd - 0.5555).abs() < 1e-9,
        "cost should accumulate; got {}",
        got.total_cost_usd,
    );
}

#[test]
fn fetch_summary_excludes_non_windsurf_usage() {
    // The LEFT JOIN filters on `source = 'windsurf'` so rows from other
    // agents that happen to share a session_id (unlikely but possible in
    // a mixed-dev test DB) must not pollute the cascade's totals.
    let db = open_db();
    db.upsert_windsurf_session(&rec("shared-id", "", "", "", 1, 100))
        .expect("meta");

    db.insert_usage_batch(&[UsageRecord {
        source: Source::Claude,
        session_id: "shared-id".into(),
        model: "claude-sonnet".into(),
        input_tokens: 999,
        output_tokens: 999,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        reasoning_output_tokens: 0,
        cost_usd: 9.9,
        timestamp: ts(10),
        project: String::new(),
        git_branch: String::new(),
    }])
    .expect("claude row");

    let rows = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    assert_eq!(rows.len(), 1);
    let got = &rows[0];
    assert_eq!(
        got.turns, 0,
        "Claude row must not leak into Windsurf totals"
    );
    assert_eq!(got.total_tokens, 0);
    assert_eq!(got.total_cost_usd, 0.0);
}

#[test]
fn summary_has_expected_field_shape() {
    // Compile-time regression: struct is pub and has the named fields
    // the TUI will destructure. Breaking the shape breaks the TUI.
    let _ = WindsurfSessionSummary {
        cascade_id: String::new(),
        summary: String::new(),
        workspace: String::new(),
        last_model: String::new(),
        created_time: None,
        last_seen: ts(0),
        turns: 0,
        total_tokens: 0,
        total_cost_usd: 0.0,
    };
}
