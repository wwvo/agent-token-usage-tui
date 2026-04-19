//! Fuzzy model name matching and cost recalculation.
//!
//! LLM model identifiers are inconsistent across the ecosystem: the same
//! underlying model can appear as `claude-sonnet-4-5`, `anthropic/claude-sonnet-4.5`,
//! or `together_ai/anthropic/claude-sonnet-4-5` depending on who reports it.
//! [`match_pricing`] implements the same three-tier strategy as agent-usage's
//! reference implementation:
//!
//! 1. Direct lookup in the pricing map.
//! 2. Try every known provider prefix (`anthropic/`, `openai/`, ...).
//! 3. Normalize both sides (lowercase, `/` → `.`, common version rewrites) and
//!    pick the pricing entry whose normalized key is bidirectionally contained
//!    in the normalized model identifier, with **shorter keys winning** so
//!    reseller paths don't outrank the canonical vendor entries.
//!
//! `recalc_costs` (landing in M2 C5b) drives this across every `cost_usd = 0`
//! row in `usage_records`.

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use rusqlite::params;

use super::Db;
use crate::domain::ModelPrice;

/// Signature of the per-row cost calculator.
///
/// Called by [`Db::recalc_costs`] once per row to turn token counts + a
/// [`ModelPrice`] into a USD figure. Kept as a `fn` pointer (not a closure)
/// because the pricing formula is stable per build and we want it to be
/// trivially `Send + Sync` across any future threading model.
///
/// Signature mirrors the columns in `usage_records`:
/// `(input_tokens, output_tokens, cache_creation_input, cache_read_input, price)`
pub type CostCalcFn = fn(i64, i64, i64, i64, &ModelPrice) -> f64;

/// Internal row shape for the recalc candidate set.
struct Candidate {
    id: i64,
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_input_tokens: i64,
    cache_read_input_tokens: i64,
}

/// Provider prefixes we try when no direct match exists.
///
/// This list matches agent-usage's Go implementation verbatim so behavior is
/// consistent when users compare tools.
const PROVIDER_PREFIXES: &[&str] = &[
    "anthropic/",
    "openai/",
    "deepseek/",
    "gemini/",
    "google/",
    "mistral/",
    "cohere/",
    "azure_ai/",
];

/// Known dotted-version rewrites applied during normalization.
///
/// Some collectors emit `claude-sonnet-4.5` while litellm catalogs the same
/// model as `claude-sonnet-4-5`; this table bridges the difference without
/// resorting to fragile regex.
const VERSION_REPLACEMENTS: &[(&str, &str)] = &[
    ("4.6", "4-6"),
    ("4.5", "4-5"),
    ("3.5", "3-5"),
    ("5.4", "5-4"),
];

/// Find the pricing entry best matching `model` in `all_prices`.
///
/// Returns `None` when no candidate can be identified. See module docs for the
/// matching strategy and the `VERSION_REPLACEMENTS` / `PROVIDER_PREFIXES`
/// tunables.
#[must_use]
pub fn match_pricing<'a>(
    model: &str,
    all_prices: &'a HashMap<String, ModelPrice>,
) -> Option<&'a ModelPrice> {
    // 1. Direct hit.
    if let Some(p) = all_prices.get(model) {
        return Some(p);
    }

    // 2. Provider-prefix hit.
    for prefix in PROVIDER_PREFIXES {
        let prefixed = format!("{prefix}{model}");
        if let Some(p) = all_prices.get(&prefixed) {
            return Some(p);
        }
    }

    // 3. Normalized bidirectional substring match with shortest-key preference.
    let model_norm = normalize(model);
    let model_norm_dash = apply_version_replacements(&model_norm);

    let mut best: Option<(&'a ModelPrice, i64)> = None;

    for (candidate_key, candidate_price) in all_prices {
        let k_norm = normalize(candidate_key);
        for query in [&model_norm, &model_norm_dash] {
            if k_norm.contains(query.as_str()) || query.contains(&k_norm) {
                // Scoring: start negative in proportion to key length so
                // shorter keys come out ahead; exact normalized equality gets
                // a large bonus so "anthropic/claude-X" beats any substring
                // permutation.
                let mut score = 10_000_i64 - candidate_key.len() as i64;
                if k_norm == **query {
                    score += 100_000_i64;
                }

                if best.is_none_or(|(_, best_score)| score > best_score) {
                    best = Some((candidate_price, score));
                }
            }
        }
    }

    best.map(|(p, _)| p)
}

impl Db {
    /// Scan every `usage_records` row with `cost_usd = 0`, fuzzy-match its
    /// `model` against `all_prices`, and write back `cost_usd = calc_fn(...)`.
    ///
    /// Rows whose model has no pricing match (direct or fuzzy) are left with
    /// `cost_usd = 0` and a `tracing::warn` event so operators can see the
    /// gap. Rows that already have a non-zero cost are skipped (WHERE clause).
    ///
    /// Returns the number of rows actually updated with a positive cost.
    ///
    /// # Errors
    ///
    /// Any SQLite error during the SELECT or the batched UPDATE aborts the
    /// operation; the UPDATE side runs inside a single transaction so partial
    /// writes can't leak out on failure.
    pub fn recalc_costs(
        &self,
        all_prices: &HashMap<String, ModelPrice>,
        calc_fn: CostCalcFn,
    ) -> Result<usize> {
        // Step 1: collect candidates under the read lock; don't hold it while
        // we iterate match_pricing + tracing::warn below.
        let candidates = self.collect_recalc_candidates()?;
        if candidates.is_empty() {
            return Ok(0);
        }

        // Step 2: open a write transaction and apply per-row updates.
        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .context("begin recalc_costs transaction")?;

        let mut updated = 0usize;
        {
            let mut stmt = tx
                .prepare_cached("UPDATE usage_records SET cost_usd = ?1 WHERE id = ?2")
                .context("prepare recalc update")?;

            for c in &candidates {
                let Some(price) = match_pricing(&c.model, all_prices) else {
                    tracing::warn!(
                        model = %c.model,
                        row_id = c.id,
                        "no pricing match for model; cost stays at 0",
                    );
                    continue;
                };
                let cost = calc_fn(
                    c.input_tokens,
                    c.output_tokens,
                    c.cache_creation_input_tokens,
                    c.cache_read_input_tokens,
                    price,
                );
                if cost > 0.0 {
                    stmt.execute(params![cost, c.id])
                        .context("apply recalc update")?;
                    updated += 1;
                }
            }
        }

        tx.commit().context("commit recalc_costs")?;
        Ok(updated)
    }

    fn collect_recalc_candidates(&self) -> Result<Vec<Candidate>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, model, input_tokens, output_tokens,
                        cache_creation_input_tokens, cache_read_input_tokens
                 FROM usage_records
                 WHERE cost_usd = 0.0",
            )
            .context("prepare recalc select")?;

        let rows = stmt
            .query_map([], |r| {
                Ok(Candidate {
                    id: r.get(0)?,
                    model: r.get(1)?,
                    input_tokens: r.get(2)?,
                    output_tokens: r.get(3)?,
                    cache_creation_input_tokens: r.get(4)?,
                    cache_read_input_tokens: r.get(5)?,
                })
            })
            .context("run recalc select")?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("read recalc candidate row")?);
        }
        Ok(out)
    }
}

fn normalize(s: &str) -> String {
    s.to_lowercase().replace('/', ".")
}

fn apply_version_replacements(s: &str) -> String {
    let mut out = s.to_owned();
    for (from, to) in VERSION_REPLACEMENTS {
        out = out.replace(from, to);
    }
    out
}

#[cfg(test)]
#[path = "costs_tests.rs"]
mod tests;
