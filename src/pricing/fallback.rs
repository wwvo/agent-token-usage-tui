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
    super::parse_litellm_json(FALLBACK_BYTES)
}

#[cfg(test)]
#[path = "fallback_tests.rs"]
mod tests;
