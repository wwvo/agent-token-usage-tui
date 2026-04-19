//! Compile-time-embedded litellm pricing snapshot.
//!
//! `build.rs` downloads the upstream catalog to
//! `assets/litellm-prices.fallback.json` on every build; this module embeds
//! that file via `include_bytes!` so the binary carries an offline-usable
//! snapshot. Runtime syncing layers (M2 C7) prefer live data and fall back to
//! [`fallback_prices`] only when the network is unreachable.
//!
//! # Format
//!
//! litellm publishes a single JSON object keyed by model identifier
//! (`"anthropic/claude-sonnet-4-5"`, `"openai/gpt-5-codex"`, ...) with
//! sibling fields for cost-per-token. We keep only the four cost fields our
//! pricing schema uses and ignore the rest (context window, provider, etc).

use chrono::Utc;

use crate::domain::ModelPrice;

/// Raw bytes of the JSON catalog embedded at compile time.
///
/// Always exists (build.rs writes `{}` when the download fails with no prior
/// file), so callers don't need to handle a missing asset.
pub const FALLBACK_BYTES: &[u8] = include_bytes!("../../assets/litellm-prices.fallback.json");

/// Parse the embedded JSON into a vector of [`ModelPrice`] entries.
///
/// * Entries without `input_cost_per_token` or `output_cost_per_token` are
///   skipped (they are e.g. fine-tune placeholders that litellm lists without
///   prices).
/// * The `sample_spec` schema example entry is always skipped.
/// * Missing `cache_*` costs default to `0.0`.
/// * `updated_at` on every result is stamped with "now" since the snapshot's
///   own timestamps are not present in the source JSON.
///
/// Always returns a valid (possibly empty) vector — malformed bytes log an
/// error and produce an empty result rather than panicking, so an offline
/// build with an empty placeholder is still usable.
#[must_use]
pub fn fallback_prices() -> Vec<ModelPrice> {
    let raw: serde_json::Value = match serde_json::from_slice(FALLBACK_BYTES) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "embedded litellm fallback JSON is invalid");
            return Vec::new();
        }
    };

    let Some(obj) = raw.as_object() else {
        tracing::error!("embedded litellm fallback JSON is not a top-level object");
        return Vec::new();
    };

    let now = Utc::now();
    let mut out = Vec::with_capacity(obj.len());

    for (model, val) in obj {
        if model == "sample_spec" {
            continue;
        }

        let (Some(input), Some(output)) = (
            val.get("input_cost_per_token").and_then(|v| v.as_f64()),
            val.get("output_cost_per_token").and_then(|v| v.as_f64()),
        ) else {
            continue;
        };

        let cache_read = val
            .get("cache_read_input_token_cost")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let cache_creation = val
            .get("cache_creation_input_token_cost")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        out.push(ModelPrice {
            model: model.clone(),
            input_cost_per_token: input,
            output_cost_per_token: output,
            cache_read_input_token_cost: cache_read,
            cache_creation_input_token_cost: cache_creation,
            updated_at: now,
        });
    }

    out
}

#[cfg(test)]
#[path = "fallback_tests.rs"]
mod tests;
