//! Sidecar tests for `windsurf_cost_diffs` batch insert + fetch.

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Db;
use crate::domain::WindsurfCostDiff;

fn ts(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).expect("valid epoch")
}

fn open_db() -> Db {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.keep().join("t.db");
    Db::open(&path).expect("open db")
}

fn sample(step_id: &str, cascade_id: &str, cost: f64, at: i64) -> WindsurfCostDiff {
    WindsurfCostDiff {
        step_id: step_id.to_owned(),
        cascade_id: cascade_id.to_owned(),
        timestamp: ts(at),
        model: "gpt-5-codex".to_owned(),
        server_cost_usd: cost,
        server_input_tokens: 100,
        server_output_tokens: 50,
        server_cache_read_tokens: 20,
    }
}

#[test]
fn empty_batch_returns_zero_and_does_not_touch_db() {
    let db = open_db();
    let n = db
        .insert_windsurf_cost_diff_batch(&[])
        .expect("empty insert");
    assert_eq!(n, 0);
    assert!(
        db.fetch_recent_windsurf_cost_diffs(10)
            .expect("fetch")
            .is_empty(),
    );
}

#[test]
fn insert_then_fetch_roundtrip_preserves_all_fields() {
    let db = open_db();
    let input = sample("step-1", "casc-a", 0.1234, 1_700_000_000);
    let n = db
        .insert_windsurf_cost_diff_batch(std::slice::from_ref(&input))
        .expect("insert");
    assert_eq!(n, 1);

    let rows = db.fetch_recent_windsurf_cost_diffs(10).expect("fetch");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0], input, "roundtrip must preserve every field");
}

#[test]
fn duplicate_step_id_is_silently_ignored() {
    // PK collisions must not crash the collector — a rescan of the same
    // JSONL file will replay the same step_ids and we rely on "INSERT
    // OR IGNORE" to no-op rather than throw.
    let db = open_db();
    let r = sample("step-1", "casc-a", 0.1, 1_700_000_000);
    assert_eq!(
        db.insert_windsurf_cost_diff_batch(std::slice::from_ref(&r))
            .expect("first"),
        1
    );
    assert_eq!(
        db.insert_windsurf_cost_diff_batch(std::slice::from_ref(&r))
            .expect("second"),
        0,
        "second insert must dedup via step_id PK",
    );
    assert_eq!(
        db.fetch_recent_windsurf_cost_diffs(10)
            .expect("fetch")
            .len(),
        1
    );
}

#[test]
fn fetch_orders_by_timestamp_desc() {
    let db = open_db();
    db.insert_windsurf_cost_diff_batch(&[
        sample("older", "casc-a", 0.1, 1_700_000_000),
        sample("newest", "casc-a", 0.3, 1_700_000_300),
        sample("middle", "casc-a", 0.2, 1_700_000_150),
    ])
    .expect("insert");

    let rows = db.fetch_recent_windsurf_cost_diffs(10).expect("fetch");
    let ids: Vec<&str> = rows.iter().map(|r| r.step_id.as_str()).collect();
    assert_eq!(ids, vec!["newest", "middle", "older"]);
}

#[test]
fn fetch_limit_clamps_row_count() {
    let db = open_db();
    let batch: Vec<_> = (0..5)
        .map(|i| sample(&format!("s-{i}"), "casc-a", 0.1, 1_700_000_000 + i))
        .collect();
    db.insert_windsurf_cost_diff_batch(&batch).expect("insert");

    let rows = db.fetch_recent_windsurf_cost_diffs(3).expect("fetch");
    assert_eq!(rows.len(), 3);
}

#[test]
fn mixed_cascades_coexist_in_the_same_table() {
    // Cross-cascade separation is maintained via cascade_id — nothing
    // about the table schema "owns" rows to a single cascade beyond
    // the query layer's chosen filter.
    let db = open_db();
    db.insert_windsurf_cost_diff_batch(&[
        sample("s-a1", "casc-a", 0.1, 1_700_000_000),
        sample("s-b1", "casc-b", 0.2, 1_700_000_100),
    ])
    .expect("insert");

    let rows = db.fetch_recent_windsurf_cost_diffs(10).expect("fetch");
    assert_eq!(rows.len(), 2);
    // Spot-check that cascade_id is preserved.
    let a = rows.iter().find(|r| r.step_id == "s-a1").expect("a");
    let b = rows.iter().find(|r| r.step_id == "s-b1").expect("b");
    assert_eq!(a.cascade_id, "casc-a");
    assert_eq!(b.cascade_id, "casc-b");
}
