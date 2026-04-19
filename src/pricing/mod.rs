//! litellm pricing sync, fallback, and cost calculation.
//!
//! The pricing subsystem is a three-tier cascade (see M2 architecture §11):
//!
//! 1. Freshness check (`Db::pricing_is_fresh`) — if the SQLite cache is newer
//!    than the configured TTL, short-circuit and use it.
//! 2. Network sync (M2 C7, `sync`) — pull the latest JSON from GitHub and
//!    upsert into SQLite.
//! 3. Compile-time fallback ([`fallback`]) — if the network is unreachable,
//!    load the snapshot baked into the binary at build time.

pub mod cost;
pub mod fallback;
pub mod sync;

use anyhow::Context;
use anyhow::Result;
use chrono::Duration;
use chrono::Utc;

use crate::domain::ModelPrice;
use crate::storage::Db;

/// Outcome of [`sync_or_fallback`], useful for CLI summary output and tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PricingSyncOutcome {
    /// DB was already fresh enough; no network round-trip was made.
    StillFresh { models: usize },
    /// Pulled a fresh snapshot from litellm and upserted it into the DB.
    FetchedFromNetwork { models: usize },
    /// Network failed (or returned empty); loaded the compile-time embedded
    /// snapshot and upserted that.
    UsedFallback { models: usize },
}

impl std::fmt::Display for PricingSyncOutcome {
    /// Human-readable form for startup / CLI summaries.
    ///
    /// Shape: "<N> models (<source>)" — concise enough to append to a
    /// progress line, detailed enough for a bug report.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StillFresh { models } => write!(f, "{models} models (fresh)"),
            Self::FetchedFromNetwork { models } => write!(f, "{models} models (network)"),
            Self::UsedFallback { models } => write!(f, "{models} models (fallback)"),
        }
    }
}

/// Keep DB pricing fresh, falling back to the embedded snapshot when needed.
///
/// Three-tier cascade:
/// 1. If `Db::pricing_is_fresh(freshness)` is true, skip the network.
/// 2. Otherwise, fetch from upstream litellm.
/// 3. If the fetch fails (or returns zero entries), load [`fallback::fallback_prices`].
///
/// All outcomes leave the DB with some pricing data (unless the fallback is
/// also empty, which would mean the build-time download failed *and* no prior
/// snapshot existed).
pub async fn sync_or_fallback(db: &Db, freshness: Duration) -> Result<PricingSyncOutcome> {
    if db.pricing_is_fresh(freshness)? {
        let models = db.get_all_pricing()?.len();
        return Ok(PricingSyncOutcome::StillFresh { models });
    }

    match sync::sync_from_github().await {
        Ok(prices) if !prices.is_empty() => {
            let models = db
                .upsert_pricing(&prices)
                .context("upsert pricing from network sync")?;
            Ok(PricingSyncOutcome::FetchedFromNetwork { models })
        }
        outcome => {
            match outcome {
                Ok(_) => {
                    tracing::warn!("litellm returned empty catalog; using embedded fallback");
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "litellm sync failed; using embedded fallback"
                    );
                }
            }
            let prices = fallback::fallback_prices();
            let models = db
                .upsert_pricing(&prices)
                .context("upsert pricing from embedded fallback")?;
            Ok(PricingSyncOutcome::UsedFallback { models })
        }
    }
}

/// Parse a litellm `model_prices_and_context_window.json` payload.
///
/// Shared by [`fallback::fallback_prices`] and [`sync::sync_from_github`] so
/// both use the same filtering (skip `sample_spec`, require
/// `input_cost_per_token` + `output_cost_per_token`, default cache costs to 0)
/// and the same `updated_at = now()` stamping.
pub(crate) fn parse_litellm_json(bytes: &[u8]) -> Vec<ModelPrice> {
    let raw: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "litellm JSON parse failed");
            return Vec::new();
        }
    };

    let Some(obj) = raw.as_object() else {
        tracing::error!("litellm JSON is not a top-level object");
        return Vec::new();
    };

    let now = Utc::now();
    let mut out = Vec::with_capacity(obj.len());

    for (model, val) in obj {
        if model == "sample_spec" {
            continue;
        }

        let (Some(input), Some(output)) = (
            val.get("input_cost_per_token")
                .and_then(serde_json::Value::as_f64),
            val.get("output_cost_per_token")
                .and_then(serde_json::Value::as_f64),
        ) else {
            continue;
        };

        let cache_read = val
            .get("cache_read_input_token_cost")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let cache_creation = val
            .get("cache_creation_input_token_cost")
            .and_then(serde_json::Value::as_f64)
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
