//! Per-model pricing data synced from litellm.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

/// Per-token pricing for a single model.
///
/// Units are **USD per single token** (litellm's convention). Multiply by token
/// counts directly; do not scale by 1_000_000.
///
/// # Invariants
///
/// * `input_cost_per_token` and `output_cost_per_token` are always present for
///   any model litellm exposes; the cache fields default to 0 when the provider
///   doesn't bill cache differently.
/// * `updated_at` is stamped by storage on upsert, not provided by litellm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelPrice {
    /// Canonical model id as published by litellm (e.g. `"anthropic/claude-sonnet-4-5"`).
    pub model: String,
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub cache_read_input_token_cost: f64,
    pub cache_creation_input_token_cost: f64,
    pub updated_at: DateTime<Utc>,
}
