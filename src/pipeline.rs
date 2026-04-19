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
//! * `windsurf_bases` — reserved for the Phase 2 VSCode exporter output.
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
    /// Windsurf exporter output (Phase 2). Currently unused but plumbed.
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
    summaries.push(claude.scan(db, reporter).await?);

    let codex = CodexCollector::with_default_paths();
    summaries.push(codex.scan(db, reporter).await?);

    let openclaw = OpenClawCollector::new(config.openclaw_bases.clone());
    summaries.push(openclaw.scan(db, reporter).await?);

    let opencode = OpenCodeCollector::new(config.opencode_dbs.clone());
    summaries.push(opencode.scan(db, reporter).await?);

    let windsurf = WindsurfCollector::new(config.windsurf_bases.clone());
    summaries.push(windsurf.scan(db, reporter).await?);

    // Best-effort cost recompute; empty pricing table → 0 rows, no warnings.
    let prices = db.get_all_pricing()?;
    let costs_recalculated = if prices.is_empty() {
        0
    } else {
        db.recalc_costs(&prices, calc_cost)?
    };

    Ok(ScanReport {
        summaries,
        costs_recalculated,
    })
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
