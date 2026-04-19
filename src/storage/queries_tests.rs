//! Sidecar tests for `storage::queries`.
//!
//! We seed the DB by hand (no collector fixture dependency) so bugs in
//! collectors can't mask bugs in the query layer.

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use crate::domain::PromptEvent;
use crate::domain::SessionRecord;
use crate::domain::Source;
use crate::domain::UsageRecord;
use crate::storage::Db;

fn ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

fn seed_db() -> (tempfile::TempDir, Db) {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("t.db")).expect("open");

    // Two sessions under Claude, one under Codex.
    db.upsert_session(&SessionRecord {
        source: Source::Claude,
        session_id: "cl-s1".into(),
        project: "proj-a".into(),
        cwd: "/p/a".into(),
        version: String::new(),
        git_branch: String::new(),
        start_time: ts("2026-04-19T10:00:00Z"),
        prompts: 0,
    })
    .unwrap();
    db.upsert_session(&SessionRecord {
        source: Source::Claude,
        session_id: "cl-s2".into(),
        project: "proj-b".into(),
        cwd: "/p/b".into(),
        version: String::new(),
        git_branch: String::new(),
        start_time: ts("2026-04-19T11:00:00Z"),
        prompts: 0,
    })
    .unwrap();
    db.upsert_session(&SessionRecord {
        source: Source::Codex,
        session_id: "cx-s1".into(),
        project: "proj-a".into(),
        cwd: "/p/a".into(),
        version: String::new(),
        git_branch: String::new(),
        start_time: ts("2026-04-19T09:00:00Z"),
        prompts: 0,
    })
    .unwrap();

    // Usage: 2 Claude rows (different models) + 1 Codex row.
    let records = vec![
        UsageRecord {
            source: Source::Claude,
            session_id: "cl-s1".into(),
            model: "claude-sonnet-4-5".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: 10,
            cache_read_input_tokens: 5,
            reasoning_output_tokens: 0,
            cost_usd: 0.5,
            timestamp: ts("2026-04-19T10:05:00Z"),
            project: "proj-a".into(),
            git_branch: String::new(),
        },
        UsageRecord {
            source: Source::Claude,
            session_id: "cl-s2".into(),
            model: "claude-opus-4-7".into(),
            input_tokens: 200,
            output_tokens: 100,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 50,
            reasoning_output_tokens: 0,
            cost_usd: 2.0,
            timestamp: ts("2026-04-19T11:05:00Z"),
            project: "proj-b".into(),
            git_branch: String::new(),
        },
        UsageRecord {
            source: Source::Codex,
            session_id: "cx-s1".into(),
            model: "gpt-5-codex".into(),
            input_tokens: 80,
            output_tokens: 40,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 10,
            reasoning_output_tokens: 5,
            cost_usd: 0.2,
            timestamp: ts("2026-04-19T09:05:00Z"),
            project: "proj-a".into(),
            git_branch: String::new(),
        },
    ];
    db.insert_usage_batch(&records).unwrap();

    // Prompts: 3 Claude + 1 Codex.
    let prompts = vec![
        PromptEvent {
            source: Source::Claude,
            session_id: "cl-s1".into(),
            timestamp: ts("2026-04-19T10:00:01Z"),
        },
        PromptEvent {
            source: Source::Claude,
            session_id: "cl-s1".into(),
            timestamp: ts("2026-04-19T10:00:10Z"),
        },
        PromptEvent {
            source: Source::Claude,
            session_id: "cl-s2".into(),
            timestamp: ts("2026-04-19T11:00:01Z"),
        },
        PromptEvent {
            source: Source::Codex,
            session_id: "cx-s1".into(),
            timestamp: ts("2026-04-19T09:00:01Z"),
        },
    ];
    db.insert_prompt_batch(&prompts).unwrap();

    (tmp, db)
}

#[test]
fn fetch_source_tallies_returns_five_rows_in_display_order() {
    let (_tmp, db) = seed_db();
    let tallies = db.fetch_source_tallies().expect("tallies");
    assert_eq!(tallies.len(), 5);
    assert_eq!(tallies[0].source, Source::Claude);
    assert_eq!(tallies[1].source, Source::Codex);
    assert_eq!(tallies[2].source, Source::OpenClaw);
    assert_eq!(tallies[3].source, Source::OpenCode);
    assert_eq!(tallies[4].source, Source::Windsurf);
}

#[test]
fn fetch_source_tallies_aggregates_usage_and_prompts() {
    let (_tmp, db) = seed_db();
    let tallies = db.fetch_source_tallies().expect("tallies");

    let claude = &tallies[0];
    assert_eq!(claude.records, 2);
    assert_eq!(claude.prompts, 3);
    assert_eq!(claude.sessions, 2);
    assert_eq!(claude.total_input_tokens, 300);
    assert_eq!(claude.total_output_tokens, 150);
    assert_eq!(claude.total_cache_read, 55);
    assert_eq!(claude.total_cache_creation, 10);
    assert!((claude.total_cost_usd - 2.5).abs() < 1e-9);

    let codex = &tallies[1];
    assert_eq!(codex.records, 1);
    assert_eq!(codex.prompts, 1);
    assert_eq!(codex.sessions, 1);
    assert!((codex.total_cost_usd - 0.2).abs() < 1e-9);

    let openclaw = &tallies[2];
    assert_eq!(openclaw.records, 0);
    assert_eq!(openclaw.prompts, 0);
    assert_eq!(openclaw.sessions, 0);
}

#[test]
fn fetch_recent_sessions_orders_by_start_time_desc() {
    let (_tmp, db) = seed_db();
    let sessions = db.fetch_recent_sessions(None, 10).expect("sessions");
    assert_eq!(sessions.len(), 3);
    assert_eq!(sessions[0].session_id, "cl-s2"); // 11:00
    assert_eq!(sessions[1].session_id, "cl-s1"); // 10:00
    assert_eq!(sessions[2].session_id, "cx-s1"); // 09:00
}

#[test]
fn fetch_recent_sessions_filters_by_source() {
    let (_tmp, db) = seed_db();
    let codex_only = db
        .fetch_recent_sessions(Some(Source::Codex), 10)
        .expect("sessions");
    assert_eq!(codex_only.len(), 1);
    assert_eq!(codex_only[0].session_id, "cx-s1");
}

#[test]
fn fetch_recent_sessions_populates_totals() {
    let (_tmp, db) = seed_db();
    let all = db.fetch_recent_sessions(None, 10).expect("sessions");
    // cl-s2 is first.
    let cs2 = &all[0];
    assert_eq!(cs2.records, 1);
    // cl-s2 has one usage row: input=200, output=100, cache_read=50,
    // cache_creation=0. `total_tokens` is the sum of all four buckets.
    assert_eq!(cs2.total_tokens, 200 + 100 + 50);
    assert!((cs2.total_cost_usd - 2.0).abs() < 1e-9);
    assert_eq!(cs2.prompts, 1);
}

#[test]
fn fetch_model_tallies_sorts_by_cost_desc() {
    let (_tmp, db) = seed_db();
    let models = db.fetch_model_tallies(None).expect("models");

    assert_eq!(models.len(), 3);
    // claude-opus has cost 2.0 (highest)
    assert_eq!(models[0].model, "claude-opus-4-7");
    assert_eq!(models[1].model, "claude-sonnet-4-5");
    assert_eq!(models[2].model, "gpt-5-codex");
}

#[test]
fn fetch_model_tallies_filters_by_source() {
    let (_tmp, db) = seed_db();
    let claude_only = db
        .fetch_model_tallies(Some(Source::Claude))
        .expect("models");
    assert_eq!(claude_only.len(), 2);
    assert!(claude_only.iter().all(|m| m.source == Source::Claude));
}
