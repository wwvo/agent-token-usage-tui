//! Single API call's token usage and cost.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::source::Source;

/// One API call: tokens in, tokens out, and the cost in USD.
///
/// # Token semantics (non-overlapping)
///
/// This matches the agent-usage reference implementation and the intended
/// SQLite schema.
///
/// * `input_tokens` — **non-cached** input only; excludes cache reads/writes.
/// * `cache_read_input_tokens` — cache hits (billed at a reduced rate).
/// * `cache_creation_input_tokens` — cache writes (billed at a premium).
/// * `output_tokens` — total output produced by the model.
/// * `reasoning_output_tokens` — subset of `output_tokens` dedicated to hidden
///   reasoning (o1-style models). Display-only; never double-counted.
///
/// The total "input side" is therefore:
/// `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`.
///
/// Collectors that read upstream APIs using overlapping semantics (e.g. Codex's
/// `token_count` events put cache reads *inside* `input_tokens`) MUST normalize
/// to the non-overlapping layout before constructing this struct — see
/// `collector::codex` for the canonical transformation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub source: Source,
    pub session_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub cost_usd: f64,
    pub timestamp: DateTime<Utc>,
    pub project: String,
    pub git_branch: String,
}

impl UsageRecord {
    /// Sum of all input-side tokens (billable input + both cache variants).
    #[must_use]
    pub const fn total_input_tokens(&self) -> i64 {
        self.input_tokens + self.cache_read_input_tokens + self.cache_creation_input_tokens
    }

    /// Sum of input (including cache) and output tokens.
    #[must_use]
    pub const fn total_tokens(&self) -> i64 {
        self.total_input_tokens() + self.output_tokens
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
