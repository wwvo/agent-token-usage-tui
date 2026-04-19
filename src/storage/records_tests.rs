//! Sidecar tests for `insert_usage_batch`, `insert_prompt_batch`, `upsert_session`.

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Db;
use crate::domain::PromptEvent;
use crate::domain::SessionRecord;
use crate::domain::Source;
use crate::domain::UsageRecord;

fn ts(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).expect("valid epoch second")
}

fn new_db() -> (tempfile::TempDir, Db) {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open db");
    (tmp, db)
}

fn sample_usage() -> UsageRecord {
    UsageRecord {
        source: Source::Claude,
        session_id: "abc".into(),
        model: "claude-sonnet-4".into(),
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_input_tokens: 20,
        cache_read_input_tokens: 30,
        reasoning_output_tokens: 0,
        cost_usd: 0.0,
        timestamp: ts(1_700_000_000),
        project: "proj".into(),
        git_branch: "main".into(),
    }
}

// ---- insert_usage_batch ----------------------------------------------------

#[test]
fn insert_usage_empty_batch_is_zero_rows() {
    let (_tmp, db) = new_db();
    assert_eq!(db.insert_usage_batch(&[]).expect("empty"), 0);
}

#[test]
fn insert_usage_returns_inserted_count() {
    let (_tmp, db) = new_db();
    let mut r2 = sample_usage();
    r2.timestamp = ts(1_700_000_001);
    let inserted = db
        .insert_usage_batch(&[sample_usage(), r2])
        .expect("insert");
    assert_eq!(inserted, 2);
}

#[test]
fn insert_usage_respects_dedup_index() {
    let (_tmp, db) = new_db();
    let r = sample_usage();

    assert_eq!(
        db.insert_usage_batch(std::slice::from_ref(&r))
            .expect("first"),
        1
    );
    assert_eq!(
        db.insert_usage_batch(std::slice::from_ref(&r))
            .expect("second"),
        0,
        "dedup index must block duplicate (session,model,ts,in,out)"
    );

    // Changing any tuple member yields a distinct row.
    let mut r2 = r.clone();
    r2.timestamp = ts(1_700_000_999);
    assert_eq!(db.insert_usage_batch(&[r2]).expect("new ts"), 1);

    let mut r3 = r.clone();
    r3.model = "claude-opus-4".into();
    assert_eq!(db.insert_usage_batch(&[r3]).expect("new model"), 1);
}

// ---- insert_prompt_batch ---------------------------------------------------

#[test]
fn insert_prompt_empty_batch_is_zero_rows() {
    let (_tmp, db) = new_db();
    assert_eq!(db.insert_prompt_batch(&[]).expect("empty"), 0);
}

#[test]
fn insert_prompt_dedup_on_session_and_ts() {
    let (_tmp, db) = new_db();
    let e = PromptEvent {
        source: Source::Claude,
        session_id: "s1".into(),
        timestamp: ts(1_700_000_000),
    };

    // Same event twice inside the same batch: dedup leaves one row.
    assert_eq!(
        db.insert_prompt_batch(&[e.clone(), e.clone()])
            .expect("batch"),
        1
    );
    // Replay: zero new rows.
    assert_eq!(db.insert_prompt_batch(&[e]).expect("replay"), 0);
}

// ---- upsert_session --------------------------------------------------------

#[test]
fn upsert_session_accumulates_prompts_and_merges_non_empty_fields() {
    let (_tmp, db) = new_db();
    let first = SessionRecord {
        source: Source::Claude,
        session_id: "s1".into(),
        project: "alpha".into(),
        cwd: String::new(),
        version: String::new(),
        git_branch: String::new(),
        start_time: ts(1_700_000_100),
        prompts: 3,
    };
    db.upsert_session(&first).expect("first upsert");

    let second = SessionRecord {
        source: Source::Claude,
        session_id: "s1".into(),
        project: String::new(), // empty: must not overwrite "alpha"
        cwd: "/home/u/proj".into(),
        version: String::new(),
        git_branch: "main".into(),
        start_time: ts(1_700_000_050), // earlier: must win
        prompts: 2,                    // delta add
    };
    db.upsert_session(&second).expect("second upsert");

    let conn = db.lock();
    let (project, cwd, git_branch, prompts): (String, String, String, i64) = conn
        .query_row(
            "SELECT project, cwd, git_branch, prompts FROM sessions WHERE session_id = 's1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("session row");

    assert_eq!(project, "alpha", "non-empty project preserved");
    assert_eq!(cwd, "/home/u/proj", "empty cwd replaced by later non-empty");
    assert_eq!(git_branch, "main");
    assert_eq!(prompts, 5, "3 + 2 delta-accumulated");
}

#[test]
fn upsert_session_keeps_earliest_start_time() {
    let (_tmp, db) = new_db();
    let later = SessionRecord {
        source: Source::Codex,
        session_id: "order-check".into(),
        project: String::new(),
        cwd: String::new(),
        version: String::new(),
        git_branch: String::new(),
        start_time: ts(1_700_000_500),
        prompts: 0,
    };
    let earlier = SessionRecord {
        start_time: ts(1_700_000_100),
        ..later.clone()
    };

    db.upsert_session(&later).expect("later first");
    db.upsert_session(&earlier).expect("earlier after");

    let conn = db.lock();
    let stored: DateTime<Utc> = conn
        .query_row(
            "SELECT start_time FROM sessions WHERE session_id = 'order-check'",
            [],
            |r| r.get(0),
        )
        .expect("start_time");
    assert_eq!(stored, ts(1_700_000_100));
}

#[test]
fn upsert_session_independent_per_session_id() {
    let (_tmp, db) = new_db();
    let template = SessionRecord {
        source: Source::Codex,
        session_id: String::new(),
        project: String::new(),
        cwd: String::new(),
        version: String::new(),
        git_branch: String::new(),
        start_time: ts(0),
        prompts: 1,
    };
    let a = SessionRecord {
        session_id: "A".into(),
        ..template.clone()
    };
    let b = SessionRecord {
        session_id: "B".into(),
        ..template.clone()
    };
    db.upsert_session(&a).expect("A");
    db.upsert_session(&b).expect("B");

    let conn = db.lock();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 2);
}
