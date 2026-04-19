//! Sidecar tests for [`fallback_prices`].
//!
//! These run against whatever snapshot `build.rs` baked in: a populated JSON
//! when the fetch succeeded, or `{}` when the CI machine had no network.
//! Tests tolerate either case.

use super::fallback_prices;

#[test]
fn fallback_parses_without_panic() {
    // Whatever was baked in must parse (valid JSON — build.rs writes `{}` as
    // a last resort so the serde step always succeeds).
    let prices = fallback_prices();
    for p in &prices {
        assert!(!p.model.is_empty(), "every entry must have a model name");
        assert!(
            p.input_cost_per_token >= 0.0,
            "input cost negative for {}: {}",
            p.model,
            p.input_cost_per_token,
        );
        assert!(
            p.output_cost_per_token >= 0.0,
            "output cost negative for {}: {}",
            p.model,
            p.output_cost_per_token,
        );
    }
}

#[test]
fn fallback_skips_sample_spec_if_present() {
    let prices = fallback_prices();
    assert!(
        !prices.iter().any(|p| p.model == "sample_spec"),
        "sample_spec must be filtered out",
    );
}

#[test]
fn fallback_is_populated_when_build_had_network() {
    let prices = fallback_prices();
    if prices.is_empty() {
        // Offline build / empty placeholder — accept and skip the downstream
        // sanity check.
        return;
    }

    // At least one well-known vendor should be present.
    let has_vendor = prices.iter().any(|p| {
        p.model.contains("claude")
            || p.model.contains("gpt")
            || p.model.contains("gemini")
            || p.model.contains("mistral")
    });
    assert!(
        has_vendor,
        "expected at least one common vendor model (claude/gpt/gemini/mistral) in a populated fallback snapshot; got {} entries",
        prices.len(),
    );
}
