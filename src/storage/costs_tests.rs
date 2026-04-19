//! Sidecar tests for [`match_pricing`].

use std::collections::HashMap;

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Db;
use super::match_pricing;
use crate::domain::ModelPrice;
use crate::domain::Source;
use crate::domain::UsageRecord;

fn mk(model: &str) -> ModelPrice {
    ModelPrice {
        model: model.to_owned(),
        input_cost_per_token: 0.001,
        output_cost_per_token: 0.002,
        cache_read_input_token_cost: 0.0,
        cache_creation_input_token_cost: 0.0,
        updated_at: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch"),
    }
}

fn price_map(models: &[&str]) -> HashMap<String, ModelPrice> {
    models.iter().map(|m| ((*m).to_owned(), mk(m))).collect()
}

#[test]
fn direct_match_wins() {
    let map = price_map(&["anthropic/claude-sonnet-4-5", "openai/gpt-5"]);
    let got = match_pricing("anthropic/claude-sonnet-4-5", &map).expect("direct hit");
    assert_eq!(got.model, "anthropic/claude-sonnet-4-5");
}

#[test]
fn provider_prefix_fills_in_missing_vendor() {
    // Collector only knows "claude-sonnet-4-5"; catalog key is the prefixed form.
    let map = price_map(&["anthropic/claude-sonnet-4-5"]);
    let got = match_pricing("claude-sonnet-4-5", &map).expect("prefixed hit");
    assert_eq!(got.model, "anthropic/claude-sonnet-4-5");
}

#[test]
fn normalized_dot_to_dash_version_matches() {
    // Dotted-version input (4.5) normalizes to dashed-version catalog entry (4-5).
    let map = price_map(&["anthropic/claude-sonnet-4-5"]);
    let got = match_pricing("claude-sonnet-4.5", &map).expect("normalized hit");
    assert_eq!(got.model, "anthropic/claude-sonnet-4-5");
}

#[test]
fn shortest_key_beats_reseller_path() {
    let map = price_map(&[
        "anthropic/claude-sonnet-4-5",
        "together_ai/anthropic/claude-sonnet-4-5",
    ]);
    let got = match_pricing("claude-sonnet-4-5", &map).expect("hit");
    assert_eq!(
        got.model, "anthropic/claude-sonnet-4-5",
        "shorter vendor-only key must outrank reseller-prefixed one"
    );
}

#[test]
fn no_match_returns_none() {
    let map = price_map(&["anthropic/claude-sonnet-4-5"]);
    assert!(match_pricing("totally-unknown-model", &map).is_none());
}

#[test]
fn openai_gpt5_codex_via_prefix() {
    let map = price_map(&["openai/gpt-5-codex"]);
    let got = match_pricing("gpt-5-codex", &map).expect("openai/ prefix");
    assert_eq!(got.model, "openai/gpt-5-codex");
}

#[test]
fn case_insensitive_match() {
    let map = price_map(&["anthropic/claude-sonnet-4-5"]);
    let got = match_pricing("CLAUDE-Sonnet-4-5", &map).expect("case-insensitive");
    assert_eq!(got.model, "anthropic/claude-sonnet-4-5");
}

#[test]
fn slash_normalized_to_dot() {
    // Model passed with `/` (treated as `.`) should still match a `.` key and vice versa.
    let map = price_map(&["anthropic.claude-sonnet-4-5"]);
    let got = match_pricing("anthropic/claude-sonnet-4-5", &map).expect("slash=dot");
    assert_eq!(got.model, "anthropic.claude-sonnet-4-5");
}

#[test]
fn empty_map_returns_none() {
    let map: HashMap<String, ModelPrice> = HashMap::new();
    assert!(match_pricing("whatever", &map).is_none());
}

// ---- recalc_costs --------------------------------------------------------

fn new_db() -> (tempfile::TempDir, Db) {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open db");
    (tmp, db)
}

fn ts(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).expect("valid epoch")
}

fn usage(model: &str, input: i64, output: i64, seed: i64) -> UsageRecord {
    UsageRecord {
        source: Source::Claude,
        session_id: format!("sess-{seed}"),
        model: model.to_owned(),
        input_tokens: input,
        output_tokens: output,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        reasoning_output_tokens: 0,
        cost_usd: 0.0,
        timestamp: ts(1_700_000_000 + seed),
        project: String::new(),
        git_branch: String::new(),
    }
}

fn test_calc(input: i64, output: i64, _cc: i64, _cr: i64, price: &ModelPrice) -> f64 {
    (input as f64) * price.input_cost_per_token + (output as f64) * price.output_cost_per_token
}

#[test]
fn recalc_empty_table_returns_zero_updates() {
    let (_tmp, db) = new_db();
    let prices = price_map(&["anthropic/claude-sonnet-4-5"]);
    assert_eq!(db.recalc_costs(&prices, test_calc).expect("recalc"), 0);
}

#[test]
fn recalc_updates_matched_rows_with_nonzero_cost() {
    let (_tmp, db) = new_db();
    db.insert_usage_batch(&[usage("claude-sonnet-4-5", 1000, 500, 1)])
        .expect("seed");

    let prices = price_map(&["anthropic/claude-sonnet-4-5"]);
    let updated = db.recalc_costs(&prices, test_calc).expect("recalc");
    assert_eq!(updated, 1);

    let conn = db.lock();
    let cost: f64 = conn
        .query_row("SELECT cost_usd FROM usage_records LIMIT 1", [], |r| {
            r.get(0)
        })
        .expect("query");
    // input * 0.001 + output * 0.002 = 1000*0.001 + 500*0.002 = 1.0 + 1.0 = 2.0
    assert!((cost - 2.0).abs() < 1e-9);
}

#[test]
fn recalc_leaves_unmatched_rows_at_zero() {
    let (_tmp, db) = new_db();
    db.insert_usage_batch(&[usage("total-unknown-model", 100, 50, 1)])
        .expect("seed");

    let prices = price_map(&["anthropic/claude-sonnet-4-5"]);
    let updated = db.recalc_costs(&prices, test_calc).expect("recalc");
    assert_eq!(updated, 0, "unmatched row must not be updated");

    let conn = db.lock();
    let cost: f64 = conn
        .query_row("SELECT cost_usd FROM usage_records LIMIT 1", [], |r| {
            r.get(0)
        })
        .expect("query");
    assert_eq!(cost, 0.0);
}

#[test]
fn recalc_skips_rows_with_existing_nonzero_cost() {
    let (_tmp, db) = new_db();
    // Seed two rows: one with cost 0 (to be updated), one with cost 5.0 (must be preserved).
    db.insert_usage_batch(&[usage("claude-sonnet-4-5", 100, 50, 1)])
        .expect("seed zero-cost");
    {
        let conn = db.lock();
        conn.execute(
            "INSERT INTO usage_records(source, session_id, model, input_tokens, output_tokens,
                                       cache_creation_input_tokens, cache_read_input_tokens,
                                       reasoning_output_tokens, cost_usd, timestamp,
                                       project, git_branch)
             VALUES('claude', 'preexisting', 'claude-sonnet-4-5', 100, 50, 0, 0, 0, 5.0,
                    '2020-01-01 00:00:00', '', '')",
            [],
        )
        .expect("seed pre-existing");
    }

    let prices = price_map(&["anthropic/claude-sonnet-4-5"]);
    let updated = db.recalc_costs(&prices, test_calc).expect("recalc");
    assert_eq!(updated, 1, "only the zero-cost row should be updated");

    let conn = db.lock();
    let preserved: f64 = conn
        .query_row(
            "SELECT cost_usd FROM usage_records WHERE session_id = 'preexisting'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(preserved, 5.0);
}

#[test]
fn recalc_runs_in_single_transaction() {
    // Sanity: commit must be atomic; running twice in a row is idempotent
    // because the WHERE clause excludes already-updated rows.
    let (_tmp, db) = new_db();
    db.insert_usage_batch(&[usage("claude-sonnet-4-5", 100, 50, 1)])
        .expect("seed");
    let prices = price_map(&["anthropic/claude-sonnet-4-5"]);

    assert_eq!(db.recalc_costs(&prices, test_calc).expect("first"), 1);
    assert_eq!(
        db.recalc_costs(&prices, test_calc).expect("second"),
        0,
        "second recalc finds no cost_usd = 0 rows"
    );
}
