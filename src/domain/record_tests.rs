//! Sidecar tests for [`UsageRecord`].

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;

use super::super::source::Source;
use super::UsageRecord;

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(0, 0).expect("epoch 0 is a valid timestamp")
}

#[test]
fn total_input_sums_non_overlapping_buckets() {
    let record = UsageRecord {
        source: Source::Claude,
        session_id: "abc".into(),
        model: "claude-sonnet-4".into(),
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: 30,
        cache_read_input_tokens: 20,
        reasoning_output_tokens: 0,
        cost_usd: 0.0,
        timestamp: epoch(),
        project: String::new(),
        git_branch: String::new(),
    };

    assert_eq!(record.total_input_tokens(), 150);
    assert_eq!(record.total_tokens(), 200);
}

#[test]
fn zero_token_record_sums_to_zero() {
    let record = UsageRecord {
        source: Source::Codex,
        session_id: "empty".into(),
        model: "gpt-5-codex".into(),
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        reasoning_output_tokens: 0,
        cost_usd: 0.0,
        timestamp: epoch(),
        project: String::new(),
        git_branch: String::new(),
    };

    assert_eq!(record.total_tokens(), 0);
}

#[test]
fn serde_roundtrip_preserves_fields() {
    let record = UsageRecord {
        source: Source::OpenCode,
        session_id: "sess-42".into(),
        model: "gpt-4o".into(),
        input_tokens: 1_000,
        output_tokens: 500,
        cache_creation_input_tokens: 200,
        cache_read_input_tokens: 300,
        reasoning_output_tokens: 50,
        cost_usd: 0.01234,
        timestamp: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("valid timestamp"),
        project: "myproj".into(),
        git_branch: "main".into(),
    };

    let json = serde_json::to_string(&record).expect("serialize");
    let back: UsageRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(record, back);
}
