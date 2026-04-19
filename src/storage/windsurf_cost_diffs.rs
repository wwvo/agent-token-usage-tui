//! Batch insert + query helpers for `windsurf_cost_diffs`.
//!
//! Mirrors the collector's intended usage:
//! [`Db::insert_windsurf_cost_diff_batch`] is called by the Windsurf
//! collector after it parses `checkpoint_cost` JSONL lines;
//! [`Db::fetch_recent_windsurf_cost_diffs`] is called by the TUI to
//! display the rolling "ours vs server" delta.
//!
//! Dedup is handled by the table's `step_id PRIMARY KEY` + `INSERT OR
//! IGNORE`: rows whose `step_id` is already on disk from a previous
//! scan are silently dropped, matching every other collector batch in
//! this crate (records / prompts).

use anyhow::Context;
use anyhow::Result;
use rusqlite::params;

use super::Db;
use crate::domain::WindsurfCostDiff;

const INSERT_COST_DIFF_SQL: &str = "\
INSERT OR IGNORE INTO windsurf_cost_diffs(
    step_id, cascade_id, timestamp, model,
    server_cost_usd,
    server_input_tokens, server_output_tokens, server_cache_read_tokens
) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

impl Db {
    /// Insert a batch of [`WindsurfCostDiff`] rows.
    ///
    /// Duplicates (same `step_id`) are silently ignored. Returns the
    /// number of rows actually inserted.
    pub fn insert_windsurf_cost_diff_batch(&self, records: &[WindsurfCostDiff]) -> Result<usize> {
        if records.is_empty() {
            return Ok(0);
        }

        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .context("begin windsurf_cost_diffs batch")?;

        let mut inserted = 0usize;
        {
            let mut stmt = tx
                .prepare_cached(INSERT_COST_DIFF_SQL)
                .context("prepare windsurf_cost_diffs insert")?;
            for r in records {
                inserted += stmt
                    .execute(params![
                        &r.step_id,
                        &r.cascade_id,
                        r.timestamp,
                        &r.model,
                        r.server_cost_usd,
                        r.server_input_tokens,
                        r.server_output_tokens,
                        r.server_cache_read_tokens,
                    ])
                    .context("insert windsurf_cost_diffs row")?;
            }
        }

        tx.commit().context("commit windsurf_cost_diffs batch")?;
        Ok(inserted)
    }

    /// Fetch the most recent `limit` checkpoint cost diffs, newest
    /// `timestamp` first.
    ///
    /// Used by the TUI / `sanity-check` workflows to spot pricing
    /// drift between atut's litellm table and Windsurf's own cost
    /// estimate.
    pub fn fetch_recent_windsurf_cost_diffs(&self, limit: usize) -> Result<Vec<WindsurfCostDiff>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT step_id, cascade_id, timestamp, model, \
                        server_cost_usd, \
                        server_input_tokens, server_output_tokens, \
                        server_cache_read_tokens \
                 FROM windsurf_cost_diffs \
                 ORDER BY timestamp DESC \
                 LIMIT ?1",
            )
            .context("prepare windsurf_cost_diffs fetch")?;

        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(WindsurfCostDiff {
                    step_id: r.get(0)?,
                    cascade_id: r.get(1)?,
                    timestamp: r.get(2)?,
                    model: r.get(3)?,
                    server_cost_usd: r.get(4)?,
                    server_input_tokens: r.get(5)?,
                    server_output_tokens: r.get(6)?,
                    server_cache_read_tokens: r.get(7)?,
                })
            })
            .context("run windsurf_cost_diffs fetch")?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("row windsurf_cost_diffs fetch")?);
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "windsurf_cost_diffs_tests.rs"]
mod tests;
