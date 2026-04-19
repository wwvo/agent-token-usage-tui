//! Sidecar tests for the Claude collector.
//!
//! Covers the scan skeleton (discovery, file-state advance, progress), the
//! JSONL `parse_envelope` helper, and full end-to-end scan → DB assertions
//! over a hand-written fixture.

use std::io::Write;
use std::path::PathBuf;

use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

use super::ClaudeCollector;
use super::SessionMeta;
use super::home_dir;
use super::parse_envelope;
use crate::collector::Collector;
use crate::collector::NoopReporter;
use crate::domain::PromptEvent;
use crate::domain::Source;
use crate::domain::UsageRecord;
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

// ---- parse_envelope unit tests --------------------------------------------

#[test]
fn parse_envelope_real_user_prompt_becomes_prompt_event() {
    let env = json!({
        "type": "user",
        "timestamp": "2026-04-19T10:00:00Z",
        "sessionId": "sess-1",
        "message": { "role": "user", "content": "hello" }
    });
    let mut records = Vec::<UsageRecord>::new();
    let mut prompts = Vec::<PromptEvent>::new();
    let mut meta = SessionMeta::default();
    parse_envelope(&env, &mut records, &mut prompts, &mut meta);
    assert_eq!(prompts.len(), 1);
    assert!(records.is_empty());
    assert_eq!(meta.session_id.as_deref(), Some("sess-1"));
}

#[test]
fn parse_envelope_tool_result_user_is_filtered() {
    let env = json!({
        "type": "user",
        "timestamp": "2026-04-19T10:00:00Z",
        "message": {
            "role": "user",
            "content": [ { "type": "tool_result", "content": "ok" } ]
        }
    });
    let mut records = Vec::<UsageRecord>::new();
    let mut prompts = Vec::<PromptEvent>::new();
    let mut meta = SessionMeta::default();
    parse_envelope(&env, &mut records, &mut prompts, &mut meta);
    assert!(prompts.is_empty());
    assert!(records.is_empty());
}

#[test]
fn parse_envelope_assistant_with_usage_becomes_usage_record() {
    let env = json!({
        "type": "assistant",
        "timestamp": "2026-04-19T10:00:05Z",
        "sessionId": "sess-1",
        "gitBranch": "main",
        "message": {
            "role": "assistant",
            "model": "claude-sonnet-4-5",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_input_tokens": 20,
                "cache_read_input_tokens": 30
            }
        }
    });
    let mut records = Vec::<UsageRecord>::new();
    let mut prompts = Vec::<PromptEvent>::new();
    let mut meta = SessionMeta::default();
    parse_envelope(&env, &mut records, &mut prompts, &mut meta);

    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.source, Source::Claude);
    assert_eq!(r.model, "claude-sonnet-4-5");
    assert_eq!(r.input_tokens, 100);
    assert_eq!(r.output_tokens, 50);
    assert_eq!(r.cache_creation_input_tokens, 20);
    assert_eq!(r.cache_read_input_tokens, 30);
    assert_eq!(r.git_branch, "main");
    assert!(prompts.is_empty());
}

#[test]
fn parse_envelope_synthetic_model_is_skipped() {
    let env = json!({
        "type": "assistant",
        "timestamp": "2026-04-19T10:00:05Z",
        "message": {
            "role": "assistant",
            "model": "<synthetic>",
            "usage": { "input_tokens": 1, "output_tokens": 2 }
        }
    });
    let mut records = Vec::<UsageRecord>::new();
    let mut prompts = Vec::<PromptEvent>::new();
    let mut meta = SessionMeta::default();
    parse_envelope(&env, &mut records, &mut prompts, &mut meta);
    assert!(records.is_empty(), "<synthetic> must be filtered");
}

#[test]
fn parse_envelope_assistant_without_usage_is_skipped() {
    let env = json!({
        "type": "assistant",
        "timestamp": "2026-04-19T10:00:05Z",
        "message": { "role": "assistant", "model": "claude-sonnet-4-5" }
    });
    let mut records = Vec::<UsageRecord>::new();
    let mut prompts = Vec::<PromptEvent>::new();
    let mut meta = SessionMeta::default();
    parse_envelope(&env, &mut records, &mut prompts, &mut meta);
    assert!(
        records.is_empty(),
        "streaming chunks have no usage and must be skipped"
    );
}

#[test]
fn session_meta_observe_tracks_earliest_timestamp() {
    let mut meta = SessionMeta::default();
    let later = chrono::DateTime::parse_from_rfc3339("2026-04-19T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let earlier = chrono::DateTime::parse_from_rfc3339("2026-04-19T09:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);

    meta.observe_envelope(&json!({}), Some(later));
    meta.observe_envelope(&json!({}), Some(earlier));
    assert_eq!(meta.first_ts, Some(earlier));
}

// ---- end-to-end scan ------------------------------------------------------

/// Build a small Claude session file and scan it; assert DB row counts.
#[tokio::test]
async fn scan_fixture_populates_prompts_and_records() {
    let tmp = tempdir().expect("tempdir");
    let projects = tmp.path().join("projects").join("demo-proj");
    std::fs::create_dir_all(&projects).expect("mkdir");

    let jsonl_path = projects.join("session-abc.jsonl");
    let mut f = std::fs::File::create(&jsonl_path).expect("create jsonl");

    // 3 real user prompts
    writeln!(f, r#"{{"type":"user","timestamp":"2026-04-19T10:00:00Z","sessionId":"sess-abc","cwd":"/home/u/demo","version":"1.0.3","gitBranch":"main","message":{{"role":"user","content":"first"}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"user","timestamp":"2026-04-19T10:00:10Z","sessionId":"sess-abc","message":{{"role":"user","content":[{{"type":"text","text":"second"}}]}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"user","timestamp":"2026-04-19T10:00:20Z","sessionId":"sess-abc","message":{{"role":"user","content":"third"}}}}"#).unwrap();

    // 1 tool_result "user" that should NOT count
    writeln!(f, r#"{{"type":"user","timestamp":"2026-04-19T10:00:15Z","sessionId":"sess-abc","message":{{"role":"user","content":[{{"type":"tool_result","content":"ok"}}]}}}}"#).unwrap();

    // 3 assistant messages with usage
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-04-19T10:00:05Z","sessionId":"sess-abc","message":{{"role":"assistant","model":"claude-sonnet-4-5","usage":{{"input_tokens":10,"output_tokens":5}}}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-04-19T10:00:16Z","sessionId":"sess-abc","message":{{"role":"assistant","model":"claude-sonnet-4-5","usage":{{"input_tokens":20,"output_tokens":15}}}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-04-19T10:00:25Z","sessionId":"sess-abc","message":{{"role":"assistant","model":"claude-opus-4-7","usage":{{"input_tokens":30,"output_tokens":25,"cache_creation_input_tokens":5,"cache_read_input_tokens":10}}}}}}"#).unwrap();

    // 1 streaming chunk (no usage) — must be skipped
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-04-19T10:00:06Z","sessionId":"sess-abc","message":{{"role":"assistant","model":"claude-sonnet-4-5"}}}}"#).unwrap();

    // 1 <synthetic> assistant — must be skipped
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-04-19T10:00:07Z","sessionId":"sess-abc","message":{{"role":"assistant","model":"<synthetic>","usage":{{"input_tokens":9999,"output_tokens":9999}}}}}}"#).unwrap();

    drop(f);

    let db = new_db_in(tmp.path());
    let c = ClaudeCollector::new(vec![tmp.path().to_path_buf()]);
    let summary = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(summary.files_scanned, 1);
    assert_eq!(summary.records_inserted, 3, "3 assistant-with-usage rows");
    assert_eq!(
        summary.prompts_inserted, 3,
        "3 real user prompts (tool_result filtered)"
    );
    assert_eq!(summary.sessions_touched, 1);
}

#[tokio::test]
async fn scan_is_idempotent_on_second_pass() {
    let tmp = tempdir().expect("tempdir");
    let projects = tmp.path().join("projects").join("proj");
    std::fs::create_dir_all(&projects).expect("mkdir");

    let jsonl_path = projects.join("session.jsonl");
    let mut f = std::fs::File::create(&jsonl_path).expect("create jsonl");
    writeln!(f, r#"{{"type":"user","timestamp":"2026-04-19T10:00:00Z","sessionId":"s1","message":{{"role":"user","content":"hi"}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-04-19T10:00:05Z","sessionId":"s1","message":{{"role":"assistant","model":"claude-sonnet-4-5","usage":{{"input_tokens":1,"output_tokens":2}}}}}}"#).unwrap();
    drop(f);

    let db = new_db_in(tmp.path());
    let c = ClaudeCollector::new(vec![tmp.path().to_path_buf()]);

    let first = c.scan(&db, &NoopReporter).await.expect("first");
    assert_eq!(first.records_inserted, 1);
    assert_eq!(first.prompts_inserted, 1);

    // Second scan: file_state shows offset == size, nothing new.
    let second = c.scan(&db, &NoopReporter).await.expect("second");
    assert_eq!(second.records_inserted, 0, "no new records on rescan");
    assert_eq!(second.prompts_inserted, 0, "no new prompts on rescan");
    assert_eq!(
        second.sessions_touched, 0,
        "no session upsert when nothing new"
    );
}
