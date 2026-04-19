//! Read-only aggregation queries for the TUI and CLI summaries.
//!
//! The TUI needs stable, typed access to a handful of common rollups:
//! "totals by source", "recent sessions", "by model", "by day". Putting
//! those behind `Db` methods keeps the raw SQL out of the UI layer and gives
//! us one place to optimize or test.
//!
//! All queries here are **read-only** — they acquire the connection, run a
//! `SELECT`, and release. The DB mutex is never held across an `.await`.

use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use chrono::DateTime;
use chrono::Duration;
use chrono::NaiveDate;
use chrono::Utc;
use rusqlite::params;

use super::Db;
use crate::domain::Source;

/// Aggregated counters for a single [`Source`].
///
/// Empty sources (no rows in `usage_records` and no rows in `prompt_events`)
/// still appear as rows with zero counters — the TUI renders a full coverage
/// table regardless of data presence.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceTally {
    pub source: Source,
    pub records: i64,
    pub prompts: i64,
    pub sessions: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_read: i64,
    pub total_cache_creation: i64,
    pub total_cost_usd: f64,
    pub last_activity: Option<DateTime<Utc>>,
}

impl SourceTally {
    /// Convenience: sum of every input-side bucket + output.
    #[must_use]
    pub const fn total_tokens(&self) -> i64 {
        self.total_input_tokens
            + self.total_output_tokens
            + self.total_cache_read
            + self.total_cache_creation
    }
}

/// One row of the "recent sessions" list.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub source: Source,
    pub session_id: String,
    pub project: String,
    pub start_time: DateTime<Utc>,
    pub prompts: i64,
    pub records: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
}

/// Per-model rollup (optionally scoped to a single source).
#[derive(Clone, Debug, PartialEq)]
pub struct ModelTally {
    pub source: Source,
    pub model: String,
    pub records: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
}

/// One UTC-day bucket in the Trend view's rolling window.
///
/// Days with no activity still appear with zero counters so the sparkline
/// renders a continuous axis (no phantom "skipped" dates).
#[derive(Clone, Debug, PartialEq)]
pub struct DailyTotal {
    pub date: NaiveDate,
    pub records: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
}

impl Db {
    /// Return one [`SourceTally`] per [`Source::all`] variant, in display order.
    ///
    /// Sources with no rows still appear; everything is zero-filled.
    pub fn fetch_source_tallies(&self) -> Result<Vec<SourceTally>> {
        let conn = self.lock();
        let mut out: Vec<SourceTally> = Source::all()
            .iter()
            .map(|s| SourceTally {
                source: *s,
                records: 0,
                prompts: 0,
                sessions: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_cache_read: 0,
                total_cache_creation: 0,
                total_cost_usd: 0.0,
                last_activity: None,
            })
            .collect();

        // Usage-side rollup.
        let mut stmt = conn
            .prepare(
                "SELECT source, COUNT(*) AS records, \
                        COALESCE(SUM(input_tokens),0), \
                        COALESCE(SUM(output_tokens),0), \
                        COALESCE(SUM(cache_read_input_tokens),0), \
                        COALESCE(SUM(cache_creation_input_tokens),0), \
                        COALESCE(SUM(cost_usd),0.0), \
                        MAX(timestamp) \
                 FROM usage_records \
                 GROUP BY source",
            )
            .context("prepare source_tally usage")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, f64>(6)?,
                    r.get::<_, Option<DateTime<Utc>>>(7)?,
                ))
            })
            .context("run source_tally usage")?;

        for row in rows {
            let (src, records, inp, outp, cr, cc, cost, last) = row.context("row usage")?;
            if let Ok(s) = Source::from_str(&src) {
                if let Some(t) = out.iter_mut().find(|t| t.source == s) {
                    t.records = records;
                    t.total_input_tokens = inp;
                    t.total_output_tokens = outp;
                    t.total_cache_read = cr;
                    t.total_cache_creation = cc;
                    t.total_cost_usd = cost;
                    t.last_activity = last;
                }
            }
        }
        drop(stmt);

        // Prompt counts.
        let mut stmt = conn
            .prepare("SELECT source, COUNT(*) FROM prompt_events GROUP BY source")
            .context("prepare source_tally prompts")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .context("run source_tally prompts")?;
        for row in rows {
            let (src, n) = row.context("row prompts")?;
            if let Ok(s) = Source::from_str(&src) {
                if let Some(t) = out.iter_mut().find(|t| t.source == s) {
                    t.prompts = n;
                }
            }
        }
        drop(stmt);

        // Sessions touched.
        let mut stmt = conn
            .prepare("SELECT source, COUNT(*) FROM sessions GROUP BY source")
            .context("prepare source_tally sessions")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .context("run source_tally sessions")?;
        for row in rows {
            let (src, n) = row.context("row sessions")?;
            if let Ok(s) = Source::from_str(&src) {
                if let Some(t) = out.iter_mut().find(|t| t.source == s) {
                    t.sessions = n;
                }
            }
        }

        Ok(out)
    }

    /// Return the N most recent sessions (ordered by `start_time DESC`).
    ///
    /// `source_filter = None` returns across every source.
    pub fn fetch_recent_sessions(
        &self,
        source_filter: Option<Source>,
        limit: usize,
    ) -> Result<Vec<SessionSummary>> {
        let conn = self.lock();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);

        // Two-step: pull session rows then decorate with per-session usage
        // aggregates. Doing this in one SQL would need a correlated subquery
        // which SQLite can do but obscures the code more than it saves.
        let sql = if source_filter.is_some() {
            "SELECT source, session_id, project, start_time, prompts \
             FROM sessions WHERE source = ?1 ORDER BY start_time DESC LIMIT ?2"
        } else {
            "SELECT source, session_id, project, start_time, prompts \
             FROM sessions ORDER BY start_time DESC LIMIT ?1"
        };
        let mut stmt = conn.prepare(sql).context("prepare sessions")?;

        let rows = match source_filter {
            Some(s) => stmt
                .query_map(params![s.as_str(), limit], map_session_row)
                .context("run sessions")?,
            None => stmt
                .query_map(params![limit], map_session_row)
                .context("run sessions")?,
        };

        let mut out: Vec<SessionSummary> = Vec::new();
        for row in rows {
            out.push(row.context("row sessions")?);
        }
        drop(stmt);

        // Decorate with per-session token / cost totals.
        let mut totals_stmt = conn
            .prepare(
                "SELECT COUNT(*), \
                        COALESCE(SUM(input_tokens + output_tokens + cache_read_input_tokens \
                                     + cache_creation_input_tokens),0), \
                        COALESCE(SUM(cost_usd),0.0) \
                 FROM usage_records \
                 WHERE source = ?1 AND session_id = ?2",
            )
            .context("prepare session totals")?;
        // Take the prompt count from `prompt_events` rather than the
        // `sessions.prompts` column — the latter is a running tally the
        // collector writes and can lag behind the actual events on a partial
        // scan; `prompt_events` is the source of truth (dedup-indexed).
        let mut prompts_stmt = conn
            .prepare(
                "SELECT COUNT(*) FROM prompt_events \
                 WHERE source = ?1 AND session_id = ?2",
            )
            .context("prepare session prompts")?;
        for s in &mut out {
            let mut rows = totals_stmt
                .query(params![s.source.as_str(), &s.session_id])
                .context("run session totals")?;
            if let Some(row) = rows.next().context("read session totals")? {
                s.records = row.get(0)?;
                s.total_tokens = row.get(1)?;
                s.total_cost_usd = row.get(2)?;
            }

            let p: i64 = prompts_stmt
                .query_row(params![s.source.as_str(), &s.session_id], |r| r.get(0))
                .context("query session prompts")?;
            s.prompts = p;
        }

        Ok(out)
    }

    /// Return the N most recent sessions that ever used `model`, newest first.
    ///
    /// Unlike [`Self::fetch_recent_sessions`] which aggregates every usage
    /// row, the decorated totals here (records / tokens / cost) are scoped
    /// to `model` only — the drill-down's promise is "what did *this*
    /// model cost on each session", so mixing in other models' totals
    /// would mislead.
    ///
    /// `prompts` is still the session-wide user-input count; prompts
    /// aren't tied to a model in our schema.
    ///
    /// # Errors
    ///
    /// Surfaces rusqlite / mapping errors annotated with query context.
    pub fn fetch_recent_sessions_by_model(
        &self,
        model: &str,
        limit: usize,
    ) -> Result<Vec<SessionSummary>> {
        let conn = self.lock();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);

        // Sessions that have *any* usage row with this model. `DISTINCT`
        // protects against the join duplicating sessions with N rows.
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT s.source, s.session_id, s.project, s.start_time, s.prompts \
                 FROM sessions s \
                 INNER JOIN usage_records u \
                   ON u.source = s.source AND u.session_id = s.session_id \
                 WHERE u.model = ?1 \
                 ORDER BY s.start_time DESC \
                 LIMIT ?2",
            )
            .context("prepare sessions_by_model")?;
        let rows = stmt
            .query_map(params![model, limit], map_session_row)
            .context("run sessions_by_model")?;

        let mut out: Vec<SessionSummary> = Vec::new();
        for row in rows {
            out.push(row.context("row sessions_by_model")?);
        }
        drop(stmt);

        // Per-session totals, scoped to this model.
        let mut totals_stmt = conn
            .prepare(
                "SELECT COUNT(*), \
                        COALESCE(SUM(input_tokens + output_tokens + cache_read_input_tokens \
                                     + cache_creation_input_tokens),0), \
                        COALESCE(SUM(cost_usd),0.0) \
                 FROM usage_records \
                 WHERE source = ?1 AND session_id = ?2 AND model = ?3",
            )
            .context("prepare sessions_by_model totals")?;
        let mut prompts_stmt = conn
            .prepare(
                "SELECT COUNT(*) FROM prompt_events \
                 WHERE source = ?1 AND session_id = ?2",
            )
            .context("prepare sessions_by_model prompts")?;
        for s in &mut out {
            let mut rows = totals_stmt
                .query(params![s.source.as_str(), &s.session_id, model])
                .context("run sessions_by_model totals")?;
            if let Some(row) = rows.next().context("read sessions_by_model totals")? {
                s.records = row.get(0)?;
                s.total_tokens = row.get(1)?;
                s.total_cost_usd = row.get(2)?;
            }
            let p: i64 = prompts_stmt
                .query_row(params![s.source.as_str(), &s.session_id], |r| r.get(0))
                .context("query sessions_by_model prompts")?;
            s.prompts = p;
        }

        Ok(out)
    }

    /// Return one [`DailyTotal`] per UTC day in the rolling window
    /// `[today - days + 1 .. today]` (inclusive, ascending).
    ///
    /// Days without activity are zero-filled so the sparkline has a
    /// continuous axis. Uses the system clock for "today", which is fine for
    /// a display-only trend (a one-minute skew doesn't change buckets).
    ///
    /// # Errors
    ///
    /// Surfaces any rusqlite / date-parse error annotated with query context.
    pub fn fetch_daily_totals(&self, days: usize) -> Result<Vec<DailyTotal>> {
        self.fetch_daily_totals_as_of(days, Utc::now().date_naive())
    }

    /// Test-friendly variant that takes an explicit "today" anchor.
    ///
    /// Real callers use [`Self::fetch_daily_totals`]; this form exists so
    /// deterministic tests can seed data relative to a fixed date.
    pub fn fetch_daily_totals_as_of(
        &self,
        days: usize,
        today: NaiveDate,
    ) -> Result<Vec<DailyTotal>> {
        let days_i = i64::try_from(days.max(1)).unwrap_or(1);
        let start = today
            .checked_sub_signed(Duration::days(days_i - 1))
            .unwrap_or(today);

        // Zero-filled scaffold so we always return exactly `days` rows.
        let mut out: Vec<DailyTotal> = (0..days_i)
            .map(|i| {
                let d = start.checked_add_signed(Duration::days(i)).unwrap_or(start);
                DailyTotal {
                    date: d,
                    records: 0,
                    total_tokens: 0,
                    total_cost_usd: 0.0,
                }
            })
            .collect();

        let conn = self.lock();
        // Bind the cutoff as a real `DateTime<Utc>` so rusqlite serializes it
        // with the *same* format it used when writing rows (including the
        // "+00:00" offset). Using a format!-built string here risks a
        // lexicographic mismatch because `+` (0x2B) sorts before `Z` (0x5A),
        // which would silently drop in-window rows.
        let cutoff: DateTime<Utc> = start.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc();
        let mut stmt = conn
            .prepare(
                "SELECT DATE(timestamp) AS d, \
                        COUNT(*), \
                        COALESCE(SUM(input_tokens + output_tokens + cache_read_input_tokens \
                                     + cache_creation_input_tokens),0), \
                        COALESCE(SUM(cost_usd),0.0) \
                 FROM usage_records \
                 WHERE timestamp >= ?1 \
                 GROUP BY d \
                 ORDER BY d ASC",
            )
            .context("prepare daily_totals")?;

        let rows = stmt
            .query_map(params![cutoff], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, f64>(3)?,
                ))
            })
            .context("run daily_totals")?;

        for row in rows {
            let (d_str, records, tokens, cost) = row.context("row daily_totals")?;
            // SQLite's DATE() format is stable YYYY-MM-DD; parse-and-slot.
            let Ok(d) = NaiveDate::parse_from_str(&d_str, "%Y-%m-%d") else {
                continue;
            };
            if let Some(slot) = out.iter_mut().find(|t| t.date == d) {
                slot.records = records;
                slot.total_tokens = tokens;
                slot.total_cost_usd = cost;
            }
        }

        Ok(out)
    }

    /// Rollup by model; optional source filter.
    pub fn fetch_model_tallies(&self, source_filter: Option<Source>) -> Result<Vec<ModelTally>> {
        let conn = self.lock();
        let sql = if source_filter.is_some() {
            "SELECT source, model, COUNT(*), \
                    COALESCE(SUM(input_tokens + output_tokens + cache_read_input_tokens \
                                 + cache_creation_input_tokens),0), \
                    COALESCE(SUM(cost_usd),0.0) \
             FROM usage_records WHERE source = ?1 \
             GROUP BY source, model ORDER BY SUM(cost_usd) DESC"
        } else {
            "SELECT source, model, COUNT(*), \
                    COALESCE(SUM(input_tokens + output_tokens + cache_read_input_tokens \
                                 + cache_creation_input_tokens),0), \
                    COALESCE(SUM(cost_usd),0.0) \
             FROM usage_records \
             GROUP BY source, model ORDER BY SUM(cost_usd) DESC"
        };
        let mut stmt = conn.prepare(sql).context("prepare model_tallies")?;

        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ModelTally> {
            let src_str: String = row.get(0)?;
            let source = Source::from_str(&src_str).unwrap_or(Source::Claude);
            Ok(ModelTally {
                source,
                model: row.get(1)?,
                records: row.get(2)?,
                total_tokens: row.get(3)?,
                total_cost_usd: row.get(4)?,
            })
        };

        let rows = match source_filter {
            Some(s) => stmt
                .query_map(params![s.as_str()], map)
                .context("run model_tallies")?,
            None => stmt.query_map([], map).context("run model_tallies")?,
        };

        let mut out: Vec<ModelTally> = Vec::new();
        for row in rows {
            out.push(row.context("row model_tallies")?);
        }
        Ok(out)
    }
}

fn map_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    let src_str: String = row.get(0)?;
    let source = Source::from_str(&src_str).unwrap_or(Source::Claude);
    Ok(SessionSummary {
        source,
        session_id: row.get(1)?,
        project: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        start_time: row.get(3)?,
        prompts: row.get(4)?,
        records: 0,
        total_tokens: 0,
        total_cost_usd: 0.0,
    })
}

#[cfg(test)]
#[path = "queries_tests.rs"]
mod tests;
