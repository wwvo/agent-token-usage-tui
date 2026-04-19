//! Sidecar tests for the Codex collector.
//!
//! Covers `parse_entry` dispatch (session_meta / turn_context / response_item /
//! event_msg + token_count) and the critical non-overlapping token correction.
//! The end-to-end fixture test lives in `tests/collector_codex_test.rs`.

use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

use super::CodexCollector;
use super::CodexState;
use super::parse_entry;
use crate::collector::Collector;
use crate::collector::NoopReporter;
use crate::domain::PromptEvent;
use crate::domain::Source;
use crate::domain::UsageRecord;
use crate::storage::Db;

fn new_state_with_model(model: &str) -> CodexState {
    CodexState {
        model: model.to_owned(),
        ..CodexState::default()
    }
}

#[test]
fn source_is_codex() {
    assert_eq!(CodexCollector::new(Vec::new()).source(), Source::Codex);
}

// ---- parse_entry: session_meta / turn_context -----------------------------

#[test]
fn session_meta_populates_state_fields() {
    let entry = json!({
        "timestamp": "2026-04-19T10:00:00Z",
        "type": "session_meta",
        "payload": {
            "id": "sess-xyz",
            "cwd": "/home/u/proj",
            "cli_version": "0.42.0"
        }
    });
    let mut records = Vec::<UsageRecord>::new();
    let mut prompts = Vec::<PromptEvent>::new();
    let mut state = CodexState::default();
    parse_entry(&entry, &mut records, &mut prompts, &mut state);

    assert_eq!(state.session_id, "sess-xyz");
    assert_eq!(state.cwd, "/home/u/proj");
    assert_eq!(state.version, "0.42.0");
}

#[test]
fn turn_context_updates_model_only() {
    let entry = json!({
        "timestamp": "2026-04-19T10:00:01Z",
        "type": "turn_context",
        "payload": { "model": "gpt-5-codex" }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = CodexState::default();
    parse_entry(&entry, &mut records, &mut prompts, &mut state);

    assert_eq!(state.model, "gpt-5-codex");
    assert!(state.session_id.is_empty());
}

// ---- parse_entry: response_item --------------------------------------------

#[test]
fn response_item_user_becomes_prompt() {
    let entry = json!({
        "timestamp": "2026-04-19T10:00:02Z",
        "type": "response_item",
        "payload": { "role": "user", "type": "message" }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = new_state_with_model("gpt-5-codex");
    parse_entry(&entry, &mut records, &mut prompts, &mut state);
    assert_eq!(prompts.len(), 1);
    assert!(records.is_empty());
}

#[test]
fn response_item_function_call_output_does_not_count() {
    let entry = json!({
        "timestamp": "2026-04-19T10:00:02Z",
        "type": "response_item",
        "payload": { "role": "user", "type": "function_call_output" }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = new_state_with_model("gpt-5-codex");
    parse_entry(&entry, &mut records, &mut prompts, &mut state);
    assert!(
        prompts.is_empty(),
        "function_call_output must not count as prompt"
    );
}

#[test]
fn response_item_assistant_is_not_prompt() {
    let entry = json!({
        "timestamp": "2026-04-19T10:00:02Z",
        "type": "response_item",
        "payload": { "role": "assistant" }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = new_state_with_model("gpt-5-codex");
    parse_entry(&entry, &mut records, &mut prompts, &mut state);
    assert!(prompts.is_empty());
}

// ---- parse_entry: token_count non-overlapping correction ------------------

#[test]
fn token_count_applies_non_overlapping_correction() {
    let entry = json!({
        "timestamp": "2026-04-19T10:00:05Z",
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": 100,          // includes cached
                    "cached_input_tokens": 30,
                    "output_tokens": 50,
                    "reasoning_output_tokens": 10
                }
            }
        }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = new_state_with_model("gpt-5-codex");
    parse_entry(&entry, &mut records, &mut prompts, &mut state);

    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.source, Source::Codex);
    assert_eq!(r.model, "gpt-5-codex");
    assert_eq!(
        r.input_tokens, 70,
        "input must be raw (100) minus cached (30)"
    );
    assert_eq!(r.cache_read_input_tokens, 30, "cache = cached_input");
    assert_eq!(
        r.cache_creation_input_tokens, 0,
        "Codex has no cache-creation split"
    );
    assert_eq!(r.output_tokens, 50);
    assert_eq!(r.reasoning_output_tokens, 10);
}

#[test]
fn token_count_without_model_in_state_is_skipped() {
    // Without a prior turn_context, we don't trust the usage row.
    let entry = json!({
        "timestamp": "2026-04-19T10:00:05Z",
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": { "last_token_usage": { "input_tokens": 100, "output_tokens": 50 } }
        }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = CodexState::default();
    parse_entry(&entry, &mut records, &mut prompts, &mut state);
    assert!(records.is_empty());
}

#[test]
fn token_count_cached_greater_than_input_saturates_to_zero() {
    // Pathological upstream data: cached > raw input. Non-overlapping subtraction
    // would underflow; saturating_sub keeps stored input at 0 instead of panicking.
    let entry = json!({
        "timestamp": "2026-04-19T10:00:05Z",
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": { "input_tokens": 20, "cached_input_tokens": 50 }
            }
        }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = new_state_with_model("gpt-5-codex");
    parse_entry(&entry, &mut records, &mut prompts, &mut state);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].input_tokens, 0, "saturating_sub clamps to 0");
    assert_eq!(records[0].cache_read_input_tokens, 50);
}

#[test]
fn non_token_count_event_is_ignored() {
    let entry = json!({
        "timestamp": "2026-04-19T10:00:05Z",
        "type": "event_msg",
        "payload": { "type": "some_other_event" }
    });
    let mut records = Vec::new();
    let mut prompts = Vec::new();
    let mut state = new_state_with_model("gpt-5-codex");
    parse_entry(&entry, &mut records, &mut prompts, &mut state);
    assert!(records.is_empty());
    assert!(prompts.is_empty());
}

// ---- End-to-end ----------------------------------------------------------

#[tokio::test]
async fn scan_empty_directories_returns_empty_summary() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open");

    let c = CodexCollector::new(vec![tmp.path().join("missing")]);
    let summary = c.scan(&db, &NoopReporter).await.expect("scan");
    assert_eq!(summary.source, Source::Codex);
    assert_eq!(summary.files_scanned, 0);
    assert_eq!(summary.records_inserted, 0);
}
