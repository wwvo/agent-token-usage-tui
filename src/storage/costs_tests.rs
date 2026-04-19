//! Sidecar tests for [`match_pricing`].

use std::collections::HashMap;

use chrono::DateTime;
use chrono::Utc;
use pretty_assertions::assert_eq;

use super::match_pricing;
use crate::domain::ModelPrice;

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
