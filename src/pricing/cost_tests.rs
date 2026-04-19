//! Sidecar tests for [`calc_cost`].

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;

use super::calc_cost;
use crate::domain::ModelPrice;

fn price() -> ModelPrice {
    ModelPrice {
        model: "test/model".into(),
        input_cost_per_token: 0.000_003,               // $3 / M
        output_cost_per_token: 0.000_015,              // $15 / M
        cache_read_input_token_cost: 0.000_000_3,      // $0.30 / M
        cache_creation_input_token_cost: 0.000_003_75, // $3.75 / M
        updated_at: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch"),
    }
}

#[test]
fn pure_input_output_only() {
    let got = calc_cost(1_000_000, 500_000, 0, 0, &price());
    // 1_000_000 * 3e-6 + 500_000 * 15e-6 = 3.0 + 7.5 = 10.5
    assert!((got - 10.5).abs() < 1e-9, "got {got}");
}

#[test]
fn all_four_buckets_sum_independently() {
    let got = calc_cost(1_000_000, 500_000, 100_000, 200_000, &price());
    // 1e6*3e-6 + 5e5*15e-6 + 1e5*3.75e-6 + 2e5*3e-7
    // = 3.0 + 7.5 + 0.375 + 0.06 = 10.935
    assert!((got - 10.935).abs() < 1e-9, "got {got}");
}

#[test]
fn zero_tokens_cost_zero() {
    assert_eq!(calc_cost(0, 0, 0, 0, &price()), 0.0);
}

#[test]
fn zero_price_rows_cost_zero_regardless_of_tokens() {
    let free = ModelPrice {
        input_cost_per_token: 0.0,
        output_cost_per_token: 0.0,
        cache_read_input_token_cost: 0.0,
        cache_creation_input_token_cost: 0.0,
        ..price()
    };
    assert_eq!(
        calc_cost(1_000_000, 1_000_000, 1_000_000, 1_000_000, &free),
        0.0
    );
}

#[test]
fn matches_agent_usage_reference_sample() {
    // Sanity-compatible with agent-usage's unit test numbers: input 5000, output 2500,
    // cache write 1000, cache read 2000; price table above → 0.000_003*5000 + 0.000_015*2500
    //  + 0.000_003_75*1000 + 0.000_000_3*2000 = 0.015 + 0.0375 + 0.00375 + 0.0006 = 0.05685
    let got = calc_cost(5_000, 2_500, 1_000, 2_000, &price());
    assert!((got - 0.056_85).abs() < 1e-9, "got {got}");
}
