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

use crate::domain::ModelPrice;

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
/// matching strategy and the [`VERSION_REPLACEMENTS`] / [`PROVIDER_PREFIXES`]
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
