//! Sidecar tests for `Db::upsert_pricing` / `get_all_pricing` / `pricing_is_fresh`.

use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Db;
use crate::domain::ModelPrice;

fn new_db() -> (tempfile::TempDir, Db) {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open db");
    (tmp, db)
}

fn price(model: &str, input: f64, output: f64) -> ModelPrice {
    ModelPrice {
        model: model.to_owned(),
        input_cost_per_token: input,
        output_cost_per_token: output,
        cache_read_input_token_cost: 0.0,
        cache_creation_input_token_cost: 0.0,
        // ignored by upsert_pricing (it stamps now()).
        updated_at: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch"),
    }
}

#[test]
fn upsert_empty_batch_is_zero_rows() {
    let (_tmp, db) = new_db();
    assert_eq!(db.upsert_pricing(&[]).expect("empty"), 0);
}

#[test]
fn upsert_then_get_roundtrips_all_fields() {
    let (_tmp, db) = new_db();
    let prices = vec![
        price("anthropic/claude-sonnet-4", 0.000_003, 0.000_015),
        price("openai/gpt-5-codex", 0.000_005, 0.000_020),
    ];
    assert_eq!(db.upsert_pricing(&prices).expect("upsert"), 2);

    let got = db.get_all_pricing().expect("read");
    assert_eq!(got.len(), 2);

    let sonnet = got
        .get("anthropic/claude-sonnet-4")
        .expect("sonnet present");
    assert_eq!(sonnet.input_cost_per_token, 0.000_003);
    assert_eq!(sonnet.output_cost_per_token, 0.000_015);
}

#[test]
fn upsert_overwrites_existing_model() {
    let (_tmp, db) = new_db();
    db.upsert_pricing(&[price("m1", 1.0, 2.0)]).expect("first");
    db.upsert_pricing(&[price("m1", 10.0, 20.0)])
        .expect("second");

    let got = db.get_all_pricing().expect("read");
    let m1 = got.get("m1").expect("m1 exists");
    assert_eq!(m1.input_cost_per_token, 10.0, "second value wins");
    assert_eq!(m1.output_cost_per_token, 20.0);

    // Only 1 row: upsert merges, does not duplicate.
    assert_eq!(got.len(), 1);
}

#[test]
fn get_all_pricing_empty_table_returns_empty_map() {
    let (_tmp, db) = new_db();
    let got = db.get_all_pricing().expect("read");
    assert!(got.is_empty());
}

#[test]
fn pricing_is_fresh_false_for_empty_table() {
    let (_tmp, db) = new_db();
    assert!(
        !db.pricing_is_fresh(Duration::hours(24)).expect("check"),
        "empty table must not be considered fresh"
    );
}

#[test]
fn pricing_is_fresh_true_immediately_after_upsert() {
    let (_tmp, db) = new_db();
    db.upsert_pricing(&[price("x", 1.0, 2.0)]).expect("upsert");
    assert!(
        db.pricing_is_fresh(Duration::hours(24)).expect("check"),
        "just-upserted row should be fresh within 24h"
    );
}

#[test]
fn pricing_is_fresh_false_when_stale_window() {
    let (_tmp, db) = new_db();
    db.upsert_pricing(&[price("x", 1.0, 2.0)]).expect("upsert");

    // 0-second window: even "right now" is not within it.
    assert!(
        !db.pricing_is_fresh(Duration::seconds(-1)).expect("check"),
        "negative window is never fresh"
    );
}
