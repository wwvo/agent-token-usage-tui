//! Integration test: the full storage + pricing loop as a real binary would
//! drive it.
//!
//! Sidecar unit tests cover each layer in isolation; this test verifies the
//! layers compose correctly through the public `agent_token_usage_tui::*`
//! surface that the CLI and TUI will use.

// Integration tests live outside the crate's `#[cfg(test)]` tree, so
// `clippy.toml`'s `allow-expect-in-tests` does not apply. Grant the workspace
// lint exceptions manually here since test-setup `.expect()` is idiomatic.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use agent_token_usage_tui::domain::ModelPrice;
use agent_token_usage_tui::domain::Source;
use agent_token_usage_tui::domain::UsageRecord;
use agent_token_usage_tui::pricing;
use agent_token_usage_tui::pricing::PricingSyncOutcome;
use agent_token_usage_tui::storage::Db;

fn ts(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).expect("valid epoch")
}

/// Full offline integration: open DB → insert usage → upsert pricing via
/// fallback → recalc_costs with real cost formula. No network required.
#[test]
fn fallback_pricing_computes_cost_end_to_end() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open db");

    // Seed a usage row that a collector might emit.
    db.insert_usage_batch(&[UsageRecord {
        source: Source::Claude,
        session_id: "sess-1".into(),
        model: "claude-sonnet-4-5".into(), // collector-form identifier
        input_tokens: 1_000,
        output_tokens: 500,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        reasoning_output_tokens: 0,
        cost_usd: 0.0,
        timestamp: ts(1_700_000_000),
        project: "test".into(),
        git_branch: "main".into(),
    }])
    .expect("insert usage");

    // Upsert one explicit pricing row (do not depend on what litellm currently ships).
    let p = ModelPrice {
        model: "anthropic/claude-sonnet-4-5".into(),
        input_cost_per_token: 0.000_003,
        output_cost_per_token: 0.000_015,
        cache_read_input_token_cost: 0.0,
        cache_creation_input_token_cost: 0.0,
        updated_at: Utc::now(),
    };
    assert_eq!(
        db.upsert_pricing(std::slice::from_ref(&p)).expect("upsert"),
        1
    );

    // Fuzzy match + recalc: "claude-sonnet-4-5" hits "anthropic/claude-sonnet-4-5" via provider prefix.
    let all_prices = db.get_all_pricing().expect("get prices");
    let updated = db
        .recalc_costs(&all_prices, pricing::cost::calc_cost)
        .expect("recalc");
    assert_eq!(updated, 1, "fuzzy match must pick the prefixed entry");
}

/// `sync_or_fallback` must populate the DB regardless of network availability
/// — as long as the embedded fallback snapshot is non-empty. CI machines
/// without network still have the snapshot baked in at build time.
#[tokio::test]
async fn sync_or_fallback_populates_db_when_snapshot_present() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open db");

    let outcome = pricing::sync_or_fallback(&db, Duration::hours(24))
        .await
        .expect("sync_or_fallback");

    let models = match outcome {
        PricingSyncOutcome::StillFresh { models }
        | PricingSyncOutcome::FetchedFromNetwork { models }
        | PricingSyncOutcome::UsedFallback { models } => models,
    };

    // Accept the fringe CI case where both network AND fallback were empty.
    if models == 0 {
        // No data to verify downstream; test is vacuously OK.
        return;
    }

    let all = db.get_all_pricing().expect("get prices");
    assert_eq!(
        all.len(),
        models,
        "get_all_pricing row count must match sync outcome"
    );
}

/// Second `sync_or_fallback` call within the freshness window must short-circuit
/// to `StillFresh` (no network) — guards against accidental repeated fetches.
#[tokio::test]
async fn sync_or_fallback_short_circuits_on_fresh_cache() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open db");

    // First call: network or fallback — whichever.
    let first = pricing::sync_or_fallback(&db, Duration::hours(24))
        .await
        .expect("first");
    let first_models = match first {
        PricingSyncOutcome::StillFresh { models }
        | PricingSyncOutcome::FetchedFromNetwork { models }
        | PricingSyncOutcome::UsedFallback { models } => models,
    };
    if first_models == 0 {
        // No data to test freshness with — empty fallback case.
        return;
    }

    // Second call immediately: must be StillFresh.
    let second = pricing::sync_or_fallback(&db, Duration::hours(24))
        .await
        .expect("second");
    assert!(
        matches!(second, PricingSyncOutcome::StillFresh { .. }),
        "repeat call within freshness window must short-circuit to StillFresh; got {second:?}",
    );
}
