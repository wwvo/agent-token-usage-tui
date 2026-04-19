//! Sidecar tests for the OpenClaw collector.

use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

use super::OpenClawCollector;
use super::OpenClawState;
use super::parse_entry;
use crate::collector::Collector;
use crate::collector::NoopReporter;
use crate::domain::PromptEvent;
use crate::domain::Source;
use crate::domain::UsageRecord;
use crate::storage::Db;

#[test]
fn source_is_openclaw() {
    assert_eq!(
        OpenClawCollector::new(Vec::new()).source(),
        Source::OpenClaw
    );
}

// ---- parse_entry -----------------------------------------------------------

#[test]
fn session_entry_seeds_session_id_and_cwd() {
    let entry = json!({
        "type": "session",
        "id": "sess-a",
        "cwd": "/home/u/p",
        "timestamp": "2026-04-19T10:00:00Z"
    });
    let mut records = Vec::<UsageRecord>::new();
    let mut prompts = Vec::<PromptEvent>::new();
    let mut state = OpenClawState::default();
    parse_entry(&entry, "agent-x", &mut records, &mut prompts, &mut state);

    assert_eq!(state.session_id, "sess-a");
    assert_eq!(state.cwd, "/home/u/p");
}

#[test]
fn user_message_without_tool_result_is_real_prompt() {
    let entry = json!({
        "type": "message",
        "timestamp": "2026-04-19T10:00:05Z",
        "message": { "role": "user", "content": "hello" }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = OpenClawState::default();
    parse_entry(&entry, "agent-x", &mut records, &mut prompts, &mut state);
    assert_eq!(prompts.len(), 1);
    assert!(records.is_empty());
}

#[test]
fn user_message_with_tool_result_is_filtered() {
    let entry = json!({
        "type": "message",
        "timestamp": "2026-04-19T10:00:05Z",
        "message": {
            "role": "user",
            "content": [ { "type": "tool_result", "content": "ok" } ]
        }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = OpenClawState::default();
    parse_entry(&entry, "agent-x", &mut records, &mut prompts, &mut state);
    assert!(prompts.is_empty());
}

#[test]
fn assistant_message_with_usage_creates_record_with_all_four_buckets() {
    let entry = json!({
        "type": "message",
        "timestamp": "2026-04-19T10:00:10Z",
        "message": {
            "role": "assistant",
            "model": "claude-sonnet-4-5",
            "usage": {
                "input": 100,
                "output": 50,
                "cacheRead": 20,
                "cacheWrite": 30
            }
        }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = OpenClawState::default();
    parse_entry(&entry, "agent-x", &mut records, &mut prompts, &mut state);

    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.source, Source::OpenClaw);
    assert_eq!(r.model, "claude-sonnet-4-5");
    assert_eq!(
        r.input_tokens, 100,
        "OpenClaw input is already non-overlapping"
    );
    assert_eq!(r.output_tokens, 50);
    assert_eq!(r.cache_read_input_tokens, 20);
    assert_eq!(r.cache_creation_input_tokens, 30);
    assert_eq!(r.project, "agent-x");
}

#[test]
fn delivery_mirror_model_is_skipped() {
    let entry = json!({
        "type": "message",
        "timestamp": "2026-04-19T10:00:10Z",
        "message": {
            "role": "assistant",
            "model": "delivery-mirror",
            "usage": { "input": 9999, "output": 9999 }
        }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = OpenClawState::default();
    parse_entry(&entry, "agent-x", &mut records, &mut prompts, &mut state);
    assert!(records.is_empty(), "delivery-mirror must not record usage");
}

#[test]
fn assistant_without_usage_is_skipped() {
    let entry = json!({
        "type": "message",
        "timestamp": "2026-04-19T10:00:10Z",
        "message": { "role": "assistant", "model": "claude-sonnet-4-5" }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = OpenClawState::default();
    parse_entry(&entry, "agent-x", &mut records, &mut prompts, &mut state);
    assert!(records.is_empty());
}

#[test]
fn missing_message_object_on_message_type_is_skipped() {
    // Malformed: type=message but no nested message field.
    let entry = json!({
        "type": "message",
        "timestamp": "2026-04-19T10:00:10Z"
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = OpenClawState::default();
    parse_entry(&entry, "agent-x", &mut records, &mut prompts, &mut state);
    assert!(records.is_empty());
    assert!(prompts.is_empty());
}

#[test]
fn state_tracks_earliest_timestamp() {
    let mut state = OpenClawState::default();
    let later = chrono::DateTime::parse_from_rfc3339("2026-04-19T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let earlier = chrono::DateTime::parse_from_rfc3339("2026-04-19T09:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    state.note_ts(later);
    state.note_ts(earlier);
    assert_eq!(state.first_ts, Some(earlier));
}

// ---- scan skeleton ---------------------------------------------------------

#[tokio::test]
async fn scan_empty_bases_returns_empty_summary() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = OpenClawCollector::new(vec![tmp.path().join("missing")]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");
    assert_eq!(s.source, Source::OpenClaw);
    assert_eq!(s.files_scanned, 0);
    assert_eq!(s.records_inserted, 0);
}

#[tokio::test]
async fn scan_ignores_agent_dirs_without_sessions_subdir() {
    let tmp = tempdir().expect("tempdir");
    // base/agent-a exists but has no sessions/ dir — must be silently skipped.
    std::fs::create_dir_all(tmp.path().join("agent-a")).expect("mkdir");
    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = OpenClawCollector::new(vec![tmp.path().to_path_buf()]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");
    assert_eq!(s.files_scanned, 0);
}
