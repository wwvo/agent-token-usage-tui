//! Upsert + query helpers for the `windsurf_sessions` table.
//!
//! This module is the storage mirror of [`crate::domain::WindsurfSessionRecord`]
//! and exists so the Windsurf collector can land cascade-level metadata
//! (title / workspace / `created_time`) alongside the per-turn usage rows
//! it already writes to `usage_records`. The planned TUI per-cascade
//! drill-down view (see `plans/windsurf-exporter-future-improvements.md`,
//! "View B") reads via [`Db::fetch_windsurf_sessions_summary`].
//!
//! # Upsert semantics
//!
//! Mirrors `upsert_session` in spirit:
//!
//! * Non-empty string fields overwrite stale values; empty inputs keep
//!   whatever is already on disk. Useful because a later scan may see
//!   a richer `session_meta` than the first.
//! * `created_time` is **first-seen-wins** via `COALESCE`: once we have
//!   a non-NULL server timestamp we never overwrite it with a later
//!   reading (or a NULL).
//! * `last_seen` is **max-wins**: every scan bumps it forward so the
//!   "most recent cascade first" ordering stays accurate.

use anyhow::Context;
use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use rusqlite::params;

use super::Db;
use crate::domain::WindsurfSessionRecord;

/// SQL for the upsert. Kept as a module constant so sidecar tests can
/// reference the same text the prod path executes.
const UPSERT_WINDSURF_SESSION_SQL: &str = "\
INSERT INTO windsurf_sessions(
    cascade_id, summary, workspace, last_model, created_time, last_seen
) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(cascade_id) DO UPDATE SET
    summary      = CASE WHEN excluded.summary      != '' THEN excluded.summary      ELSE windsurf_sessions.summary      END,
    workspace    = CASE WHEN excluded.workspace    != '' THEN excluded.workspace    ELSE windsurf_sessions.workspace    END,
    last_model   = CASE WHEN excluded.last_model   != '' THEN excluded.last_model   ELSE windsurf_sessions.last_model   END,
    created_time = COALESCE(windsurf_sessions.created_time, excluded.created_time),
    last_seen    = CASE WHEN excluded.last_seen > windsurf_sessions.last_seen THEN excluded.last_seen ELSE windsurf_sessions.last_seen END";

/// View-model row emitted by [`Db::fetch_windsurf_sessions_summary`].
///
/// Joins `windsurf_sessions` to `usage_records` so the TUI can render a
/// single table without issuing per-row follow-up queries. A cascade that
/// hasn't produced any `usage_records` yet (first-seen `session_meta` with
/// no `turn_usage` lines flushed) still shows up with zero counters so the
/// drill-down doesn't lie about its existence.
#[derive(Clone, Debug, PartialEq)]
pub struct WindsurfSessionSummary {
    pub cascade_id: String,
    pub summary: String,
    pub workspace: String,
    pub last_model: String,
    pub created_time: Option<DateTime<Utc>>,
    pub last_seen: DateTime<Utc>,
    /// Count of `usage_records` rows whose `session_id == cascade_id`.
    pub turns: i64,
    /// Sum of `input + output + cache_read + cache_creation` across those rows.
    pub total_tokens: i64,
    /// Sum of `cost_usd` across those rows (atut's own estimate, derived
    /// from the litellm pricing table).
    pub total_cost_usd: f64,
    /// Sum of `server_cost_usd` across `windsurf_cost_diffs` rows for
    /// this cascade — Windsurf's own USD estimate. `0.0` when the
    /// exporter hasn't captured any checkpoint costs yet (either the
    /// cascade is brand-new or the v0.2.10 `checkpoint_cost` extractor
    /// didn't find a match on `metadata.modelCost`). Used by the TUI's
    /// Cascades view to surface pricing drift.
    pub server_cost_usd: f64,
}

impl Db {
    /// Upsert one cascade's metadata row.
    ///
    /// Idempotent by design: callers may run the scan loop repeatedly
    /// without worrying about duplicates. Returns `()` because the row
    /// count isn't useful here (the collector already knows whether it
    /// just saw a `session_meta` line).
    pub fn upsert_windsurf_session(&self, rec: &WindsurfSessionRecord) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            UPSERT_WINDSURF_SESSION_SQL,
            params![
                &rec.cascade_id,
                &rec.summary,
                &rec.workspace,
                &rec.last_model,
                rec.created_time,
                rec.last_seen,
            ],
        )
        .context("upsert windsurf_session")?;
        Ok(())
    }

    /// Fetch the most recent `limit` cascades, newest `last_seen` first,
    /// decorated with aggregate turn / token / cost counters.
    ///
    /// Uses a `LEFT JOIN` so cascades without any `usage_records` rows
    /// still appear with zeroed counters — this covers the edge case of
    /// a first-seen `session_meta` whose follow-up `turn_usage` lines
    /// haven't been flushed yet.
    pub fn fetch_windsurf_sessions_summary(
        &self,
        limit: usize,
    ) -> Result<Vec<WindsurfSessionSummary>> {
        let limit_i64 = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        let conn = self.lock();
        // The `windsurf_cost_diffs` join is pre-aggregated in a
        // subquery, not joined directly: a plain LEFT JOIN combined
        // with the existing GROUP BY on `usage_records` would build a
        // Cartesian product (turns × checkpoints) and silently multiply
        // `total_cost_usd`. The subquery hits `idx_windsurf_cost_diffs_
        // cascade` so the extra nest is cheap.
        let mut stmt = conn
            .prepare(
                "SELECT ws.cascade_id, ws.summary, ws.workspace, ws.last_model, \
                        ws.created_time, ws.last_seen, \
                        COUNT(ur.id) AS turns, \
                        COALESCE(SUM(ur.input_tokens + ur.output_tokens + \
                                     ur.cache_read_input_tokens + \
                                     ur.cache_creation_input_tokens),0) AS total_tokens, \
                        COALESCE(SUM(ur.cost_usd),0.0) AS total_cost_usd, \
                        COALESCE(scd.total_server_cost, 0.0) AS server_cost_usd \
                 FROM windsurf_sessions ws \
                 LEFT JOIN usage_records ur \
                        ON ur.session_id = ws.cascade_id AND ur.source = 'windsurf' \
                 LEFT JOIN ( \
                     SELECT cascade_id, SUM(server_cost_usd) AS total_server_cost \
                     FROM windsurf_cost_diffs \
                     GROUP BY cascade_id \
                 ) scd ON scd.cascade_id = ws.cascade_id \
                 GROUP BY ws.cascade_id \
                 ORDER BY ws.last_seen DESC \
                 LIMIT ?1",
            )
            .context("prepare windsurf_sessions summary")?;

        let rows = stmt
            .query_map(params![limit_i64], |r| {
                Ok(WindsurfSessionSummary {
                    cascade_id: r.get(0)?,
                    summary: r.get(1)?,
                    workspace: r.get(2)?,
                    last_model: r.get(3)?,
                    created_time: r.get(4)?,
                    last_seen: r.get(5)?,
                    turns: r.get(6)?,
                    total_tokens: r.get(7)?,
                    total_cost_usd: r.get(8)?,
                    server_cost_usd: r.get(9)?,
                })
            })
            .context("run windsurf_sessions summary")?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("row windsurf_sessions summary")?);
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "windsurf_sessions_tests.rs"]
mod tests;
