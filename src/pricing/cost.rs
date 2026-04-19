//! Per-call USD cost formula.
//!
//! The formula mirrors agent-usage's reference implementation so migrating
//! datasets between the two tools yields matching figures:
//!
//! ```text
//! cost = input_tokens            × input_cost_per_token
//!      + cache_creation_tokens   × cache_creation_input_token_cost
//!      + cache_read_tokens       × cache_read_input_token_cost
//!      + output_tokens           × output_cost_per_token
//! ```
//!
//! All token buckets are **non-overlapping** (see `crate::domain::UsageRecord`
//! for the contract). Passing an overlapping breakdown (e.g. cache counts
//! embedded in `input_tokens`) double-bills those tokens.

use crate::domain::ModelPrice;

/// Compute the USD cost for a single API call.
///
/// `f64` return value: the fixed formula produces billions of possible
/// distinct outputs already; rounding happens at display time in the TUI.
///
/// Conforms to the [`crate::storage::costs::CostCalcFn`] signature so it
/// plugs directly into `Db::recalc_costs`.
#[must_use]
pub fn calc_cost(
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_input_tokens: i64,
    cache_read_input_tokens: i64,
    price: &ModelPrice,
) -> f64 {
    #[allow(clippy::cast_precision_loss)] // token counts fit comfortably in f64 mantissa
    let cost = (input_tokens as f64) * price.input_cost_per_token
        + (cache_creation_input_tokens as f64) * price.cache_creation_input_token_cost
        + (cache_read_input_tokens as f64) * price.cache_read_input_token_cost
        + (output_tokens as f64) * price.output_cost_per_token;
    cost
}

#[cfg(test)]
#[path = "cost_tests.rs"]
mod tests;
