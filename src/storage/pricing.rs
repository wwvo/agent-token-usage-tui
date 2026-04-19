//! Per-model pricing storage.
//!
//! Pricing rows are effectively a key-value table keyed by `model` — each
//! upsert overwrites the four per-token costs and stamps `updated_at` with
//! "right now". The freshness check lets `pricing::sync_or_fallback` skip the
//! network round-trip when the cache is still recent.

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use rusqlite::params;

use super::Db;
use crate::domain::ModelPrice;

const UPSERT_PRICING_SQL: &str = "\
INSERT INTO pricing(
    model,
    input_cost_per_token, output_cost_per_token,
    cache_read_input_token_cost, cache_creation_input_token_cost,
    updated_at
) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(model) DO UPDATE SET
    input_cost_per_token            = excluded.input_cost_per_token,
    output_cost_per_token           = excluded.output_cost_per_token,
    cache_read_input_token_cost     = excluded.cache_read_input_token_cost,
    cache_creation_input_token_cost = excluded.cache_creation_input_token_cost,
    updated_at                      = excluded.updated_at";

const SELECT_ALL_PRICING_SQL: &str = "\
SELECT model,
       input_cost_per_token, output_cost_per_token,
       cache_read_input_token_cost, cache_creation_input_token_cost,
       updated_at
FROM pricing";

impl Db {
    /// Upsert a batch of pricing rows, stamping `updated_at = now()`.
    ///
    /// The `updated_at` field on the input [`ModelPrice`]s is **ignored** —
    /// freshness is defined by "when did we sync", not by whatever litellm
    /// declared. The batch runs in a single transaction.
    ///
    /// Returns the number of rows touched (inserted + updated).
    pub fn upsert_pricing(&self, prices: &[ModelPrice]) -> Result<usize> {
        if prices.is_empty() {
            return Ok(0);
        }

        let now = Utc::now();
        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .context("begin pricing upsert transaction")?;

        let mut touched = 0usize;
        {
            let mut stmt = tx
                .prepare_cached(UPSERT_PRICING_SQL)
                .context("prepare pricing upsert")?;
            for p in prices {
                touched += stmt
                    .execute(params![
                        &p.model,
                        p.input_cost_per_token,
                        p.output_cost_per_token,
                        p.cache_read_input_token_cost,
                        p.cache_creation_input_token_cost,
                        now,
                    ])
                    .with_context(|| format!("upsert pricing for model {}", p.model))?;
            }
        }

        tx.commit().context("commit pricing upsert")?;
        Ok(touched)
    }

    /// Read every pricing row, keyed by `model` id.
    ///
    /// Returns an empty map when the table is empty. Meant to be called once
    /// per scan (not per row); the `HashMap` keeps later fuzzy lookups O(1).
    pub fn get_all_pricing(&self) -> Result<HashMap<String, ModelPrice>> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare_cached(SELECT_ALL_PRICING_SQL)
            .context("prepare pricing select")?;

        let rows = stmt
            .query_map([], |r| {
                Ok(ModelPrice {
                    model: r.get(0)?,
                    input_cost_per_token: r.get(1)?,
                    output_cost_per_token: r.get(2)?,
                    cache_read_input_token_cost: r.get(3)?,
                    cache_creation_input_token_cost: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            })
            .context("run pricing select")?;

        let mut out = HashMap::new();
        for row in rows {
            let p = row.context("read pricing row")?;
            out.insert(p.model.clone(), p);
        }
        Ok(out)
    }

    /// Is the most recent pricing update within `max_age` of "now"?
    ///
    /// Returns `false` for an empty table (fresh sync should happen on first run).
    pub fn pricing_is_fresh(&self, max_age: Duration) -> Result<bool> {
        let conn = self.lock();
        let max_ts: Option<DateTime<Utc>> = conn
            .query_row("SELECT MAX(updated_at) FROM pricing", [], |r| r.get(0))
            .context("query MAX(updated_at) from pricing")?;

        Ok(match max_ts {
            None => false,
            Some(ts) => (Utc::now() - ts) <= max_age,
        })
    }
}

#[cfg(test)]
#[path = "pricing_tests.rs"]
mod tests;
