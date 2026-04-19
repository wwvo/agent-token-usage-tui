//! Scan pipeline — drives every collector in order and recomputes costs.
//!
//! This module exists as the single entry point that both the `atut scan`
//! CLI subcommand and (later) the TUI's startup scan funnel through. Keeping
//! the orchestration here avoids duplicating collector wiring and keeps the
//! CLI free of domain logic.
//!
//! # Ordering
//!
//! Collectors run sequentially in a deterministic order
//! (Claude → Codex → OpenClaw → OpenCode → Windsurf). Sequential execution
//! costs us nothing practical — each collector does its own short file walk
//! — and it keeps progress reports and log output predictable. Parallel
//! execution is trivial to add later if profiles ever demand it.
//!
//! # Configuration surface
//!
//! [`PipelineConfig`] carries the paths that can't be inferred from `$HOME`:
//!
//! * `openclaw_bases` — bases that contain `<agent>/sessions/*.jsonl` trees.
//! * `opencode_dbs` — absolute paths to OpenCode SQLite files.
//! * `windsurf_bases` — override for the VSCode exporter's JSONL output dir.
//!   Empty (the default) falls back to [`WindsurfCollector::with_default_paths`],
//!   which honours `ATUT_WINDSURF_SESSIONS_DIR` and otherwise lands on
//!   `~/.atut/windsurf-sessions/`.
//!
//! Claude and Codex use their well-known `$HOME`-relative directories and do
//! not appear in the config; see [`ClaudeCollector::with_default_paths`] and
//! [`CodexCollector::with_default_paths`].

use std::path::PathBuf;

use anyhow::Result;

use crate::collector::ClaudeCollector;
use crate::collector::CodexCollector;
use crate::collector::Collector;
use crate::collector::OpenClawCollector;
use crate::collector::OpenCodeCollector;
use crate::collector::Reporter;
use crate::collector::ScanSummary;
use crate::collector::WindsurfCollector;
use crate::pricing::cost::calc_cost;
use crate::storage::Db;

/// Paths not derivable from `$HOME` — sourced from config or CLI flags.
#[derive(Clone, Debug, Default)]
pub struct PipelineConfig {
    /// OpenClaw base directories (`<base>/<agent>/sessions/*.jsonl`).
    pub openclaw_bases: Vec<PathBuf>,
    /// OpenCode SQLite file paths.
    pub opencode_dbs: Vec<PathBuf>,
    /// Windsurf exporter output directories. Empty means "use the default"
    /// — see [`WindsurfCollector::with_default_paths`] for the env-var and
    /// `~/.atut/windsurf-sessions/` fallback.
    pub windsurf_bases: Vec<PathBuf>,
}

/// Aggregated result of a full pipeline run.
#[derive(Debug)]
pub struct ScanReport {
    /// One entry per collector, in run order.
    pub summaries: Vec<ScanSummary>,
    /// Rows whose `cost_usd` was updated from 0 to a positive number by the
    /// post-scan `recalc_costs` pass. `0` here is legitimate — it just means
    /// the pricing table was empty (first run before `sync-prices`).
    pub costs_recalculated: usize,
}

/// Run Claude → Codex → OpenClaw → OpenCode → Windsurf in order, then recompute costs.
///
/// Per-collector errors are surfaced in [`ScanSummary::errors`] — the pipeline
/// never aborts midway just because one agent's data is malformed.
///
/// # Errors
///
/// Propagates the first unrecoverable collector error (e.g. a database open
/// failure) or a `recalc_costs` SQLite error.
pub async fn run_scan(
    db: &Db,
    reporter: &dyn Reporter,
    config: &PipelineConfig,
) -> Result<ScanReport> {
    let mut summaries: Vec<ScanSummary> = Vec::with_capacity(5);

    let claude = ClaudeCollector::with_default_paths();
    summaries.push(run_one(&claude, db, reporter).await?);

    let codex = CodexCollector::with_default_paths();
    summaries.push(run_one(&codex, db, reporter).await?);

    let openclaw = OpenClawCollector::new(config.openclaw_bases.clone());
    summaries.push(run_one(&openclaw, db, reporter).await?);

    let opencode = OpenCodeCollector::new(config.opencode_dbs.clone());
    summaries.push(run_one(&opencode, db, reporter).await?);

    // Empty `windsurf_bases` falls back to the exporter's default output dir
    // (`~/.atut/windsurf-sessions/`, or `ATUT_WINDSURF_SESSIONS_DIR` if set).
    // Without this, the out-of-the-box experience silently scans zero files
    // because the default PipelineConfig is `vec![]`.
    let windsurf = if config.windsurf_bases.is_empty() {
        WindsurfCollector::with_default_paths()
    } else {
        WindsurfCollector::new(config.windsurf_bases.clone())
    };
    summaries.push(run_one(&windsurf, db, reporter).await?);

    // Best-effort cost recompute; empty pricing table → 0 rows, no warnings.
    let prices = db.get_all_pricing()?;
    let costs_recalculated = if prices.is_empty() {
        0
    } else {
        db.recalc_costs(&prices, calc_cost)?
    };

    // Windsurf cross-check: surface any cascade where atut's own
    // `cost_usd` estimate drifts past a few percent of the server's
    // own figure. Best-effort — errors fetching the summary aren't
    // fatal, they just skip the warning pass.
    warn_on_windsurf_cost_drift(db);

    Ok(ScanReport {
        summaries,
        costs_recalculated,
    })
}

/// Threshold (as a fraction) above which cost drift warrants a warning.
///
/// `0.05` (= 5%) is a cheap "have we noticed Windsurf shipping a price
/// change before litellm caught up?" tripwire. Anything below that is
/// usually within rounding noise or per-request provider variance.
const COST_DRIFT_THRESHOLD: f64 = 0.05;

/// Emit `tracing::warn!` for every Windsurf cascade whose ours-vs-server
/// cost differs by more than [`COST_DRIFT_THRESHOLD`]. Silent when the
/// summary query errors (we'd rather lose the warning than abort the
/// scan) or the server-side figure is missing / zero.
fn warn_on_windsurf_cost_drift(db: &Db) {
    // 512 is a soft cap — the TUI's Cascades view already uses
    // `SESSIONS_PAGE` (2k), but 512 most-recent cascades is plenty for
    // a drift canary and keeps the extra SELECT cheap.
    const DRIFT_SCAN_LIMIT: usize = 512;

    let Ok(rows) = db.fetch_windsurf_sessions_summary(DRIFT_SCAN_LIMIT) else {
        return;
    };
    for r in rows {
        if r.server_cost_usd <= 0.0 || r.total_cost_usd <= 0.0 {
            continue;
        }
        let delta = (r.total_cost_usd - r.server_cost_usd).abs();
        let drift = delta / r.server_cost_usd;
        if drift > COST_DRIFT_THRESHOLD {
            tracing::warn!(
                cascade_id = %r.cascade_id,
                model = %r.last_model,
                ours_usd = r.total_cost_usd,
                server_usd = r.server_cost_usd,
                drift_pct = drift * 100.0,
                "windsurf cost drift: atut's estimate differs from Windsurf's own figure by \
                 more than {:.0}% — litellm snapshot may be stale",
                COST_DRIFT_THRESHOLD * 100.0,
            );
        }
    }
}

/// Invoke one collector with lifecycle hooks on the reporter.
///
/// Calls `on_source_start` before the collector runs and `on_source_finished`
/// afterward — both hooks have `Reporter` default impls so legacy
/// reporters (`NoopReporter`, test fakes) are unaffected. The TUI's
/// startup reporter overrides them to draw progress lines.
async fn run_one<C: Collector>(
    collector: &C,
    db: &Db,
    reporter: &dyn Reporter,
) -> Result<ScanSummary> {
    let source = collector.source();
    reporter.on_source_start(source);
    let summary = collector.scan(db, reporter).await?;
    reporter.on_source_finished(source, &summary);
    Ok(summary)
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
