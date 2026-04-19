//! Command-line surface and top-level dispatcher.
//!
//! This module owns `clap` parsing and routes each subcommand to the relevant
//! subsystem. `main.rs` is a thin wrapper around [`run`] so the dispatch logic
//! stays library-testable and reusable from integration tests or future tools
//! (e.g. the Phase 2 Windsurf exporter).

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::ArgAction;
use clap::Parser;
use clap::Subcommand;

use crate::app_dir;
use crate::collector::NoopReporter;
use crate::collector::ScanSummary;
use crate::config::Config;
use crate::logging;
use crate::logging::LogMode;
use crate::pipeline::run_scan as pipeline_run_scan;
use crate::pricing::PricingSyncOutcome;
use crate::pricing::cost::calc_cost;
use crate::pricing::sync_or_fallback;
use crate::storage::Db;

/// Default pricing freshness window. Reused by both `sync-prices` and the
/// TUI startup sync to keep user-facing behavior aligned.
const PRICING_FRESHNESS: chrono::Duration = chrono::Duration::hours(24);

/// Top-level CLI schema.
///
/// Global flags (`--config`, `--data-dir`, `-v`, `--no-scan`, `--no-prices`)
/// are defined here so every subcommand — and the default TUI entry point —
/// sees the same surface. Several fields are currently `#[allow(dead_code)]`:
/// they are parsed now to stabilize the UX while the subsystems that consume
/// them land in later phases (M2 C7, M4 C5, M5 C6, M7 C3).
#[derive(Debug, Parser)]
#[command(
    name = env!("CARGO_PKG_NAME"),
    version,
    about,
    long_about = None,
)]
#[allow(dead_code)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Override path to `config.toml` (defaults to `<exe-dir>/config.toml`).
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,

    /// Override the portable runtime directory (defaults to `<exe-dir>`).
    ///
    /// Useful for integration tests that want an isolated sandbox.
    #[arg(long, value_name = "DIR", global = true)]
    data_dir: Option<PathBuf>,

    /// Increase verbosity (`-v` = debug, `-vv` = trace). Overrides `RUST_LOG`.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    /// Skip the initial scan when entering the TUI.
    #[arg(long, global = true)]
    no_scan: bool,

    /// Skip the pricing sync on startup; use whatever is already cached.
    #[arg(long, global = true)]
    no_prices: bool,
}

/// Subcommands exposed to users.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scan known agent session directories, update the database, and exit.
    ///
    /// Intended for cron / scheduled invocation where a TUI is inappropriate.
    Scan,

    /// Refresh the litellm model pricing cache and recompute costs, then exit.
    SyncPrices,

    /// Print version information and exit.
    Version,
}

/// CLI entry point used by `main.rs`.
///
/// Errors bubble up from subsystems; `main.rs` is responsible for rendering
/// them and choosing an exit code.
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    // Route log output so it doesn't collide with user-facing modes:
    // the TUI owns stderr (raw mode + alt screen), so it needs File; every
    // other subcommand is line-oriented and benefits from immediate stderr.
    let mode = if cli.command.is_none() {
        LogMode::File
    } else {
        LogMode::Stderr
    };
    logging::init(mode)?;

    match cli.command {
        None => run_tui(&cli),
        Some(Commands::Scan) => run_scan(cli.data_dir.as_deref()),
        Some(Commands::SyncPrices) => run_sync_prices(cli.data_dir.as_deref()),
        Some(Commands::Version) => print_version(),
    }
}

/// Default `atut` entry point: prime the DB, then launch the TUI.
///
/// Primes in a fixed order because each step feeds the next:
/// 1. Sync pricing catalog (unless `--no-prices`).
/// 2. Run the scan pipeline over every collector (unless `--no-scan`).
///    Claude / Codex use their `$HOME`-derived defaults; OpenClaw / OpenCode
///    / Windsurf get empty configs (M6 C2 wires these to `config.toml`).
/// 3. Hand the fully-primed `Db` to the TUI event loop.
///
/// Both priming steps are best-effort: a network failure or malformed
/// session file surfaces as a tracing warning rather than aborting the TUI.
fn run_tui(cli: &Cli) -> Result<()> {
    let db_path = resolve_db_path(cli.data_dir.as_deref())?;
    let db =
        Db::open(&db_path).with_context(|| format!("open database at {}", db_path.display()))?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for TUI")?;

    rt.block_on(async {
        if !cli.no_prices {
            match sync_or_fallback(&db, PRICING_FRESHNESS).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "pricing sync failed; continuing with stale/missing prices");
                }
            }
        }
        if !cli.no_scan {
            let pipeline_cfg = match load_pipeline_config(cli.data_dir.as_deref()) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "config load failed; falling back to defaults");
                    crate::pipeline::PipelineConfig::default()
                }
            };
            match pipeline_run_scan(&db, &NoopReporter, &pipeline_cfg).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "scan pipeline failed; continuing with existing DB");
                }
            }
        }
        crate::tui::run(db).await
    })
}

/// Resolve the database path honoring the `--data-dir` override.
///
/// When an explicit directory is supplied we land `data.db` in it; otherwise
/// we defer to the portable `app_dir::db_path()` (alongside the executable).
fn resolve_db_path(data_dir: Option<&Path>) -> Result<PathBuf> {
    match data_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("create --data-dir {}", dir.display()))?;
            Ok(dir.join("data.db"))
        }
        None => app_dir::db_path(),
    }
}

/// `atut scan` — crawl every known agent and print a compact summary.
///
/// Inlines Claude + Codex collectors here rather than through a `pipeline`
/// abstraction; that's scheduled for M4 C5 when OpenClaw and OpenCode join
/// the set. Uses a dedicated current-thread tokio runtime so the sync `run()`
/// dispatcher doesn't have to depend on `#[tokio::main]`; when the TUI lands
/// in M5 C6 we can hoist the runtime into `run()`.
///
/// Cost recomputation runs post-scan **only if** the pricing table already has
/// rows (from a prior `sync-prices` call or the TUI's startup sync). That
/// keeps `atut scan` fully offline-capable as documented.
fn run_scan(data_dir: Option<&Path>) -> Result<()> {
    let db_path = resolve_db_path(data_dir)?;
    let db =
        Db::open(&db_path).with_context(|| format!("open database at {}", db_path.display()))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for scan")?;

    let pipeline_cfg = load_pipeline_config(data_dir)?;

    let report = rt
        .block_on(pipeline_run_scan(&db, &NoopReporter, &pipeline_cfg))
        .context("run scan pipeline")?;

    print_scan_summary(&report.summaries, report.costs_recalculated)
}

/// Resolve the `config.toml` path, honoring `--data-dir`.
///
/// When `--data-dir` is given we look for `config.toml` inside it; otherwise
/// we land on the portable `<exe-dir>/config.toml`. Missing files are not
/// errors; [`Config::load_or_default`] returns defaults transparently.
fn resolve_config_path(data_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = data_dir {
        Ok(dir.join("config.toml"))
    } else {
        app_dir::config_path()
    }
}

/// Load `config.toml` (or defaults) and project onto `PipelineConfig`.
fn load_pipeline_config(data_dir: Option<&Path>) -> Result<crate::pipeline::PipelineConfig> {
    let path = resolve_config_path(data_dir)?;
    let cfg = Config::load_or_default(&path)
        .with_context(|| format!("load config from {}", path.display()))?;
    Ok(cfg.to_pipeline())
}

/// `atut sync-prices` — refresh the litellm pricing cache and re-price rows.
///
/// Policy:
/// * Short-circuit when the DB's pricing is already younger than 24h.
/// * Otherwise hit litellm's GitHub raw JSON.
/// * On any network failure, fall back to the snapshot baked into the binary
///   (build.rs embeds it via `include_bytes!`), so the command never leaves
///   the user stuck on an airplane.
/// * After the sync, recompute `cost_usd` for every row that's still zero —
///   freshly-synced pricing is only useful if it propagates into historical
///   usage.
fn run_sync_prices(data_dir: Option<&Path>) -> Result<()> {
    let db_path = resolve_db_path(data_dir)?;
    let db =
        Db::open(&db_path).with_context(|| format!("open database at {}", db_path.display()))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime for sync-prices")?;

    let outcome = rt
        .block_on(sync_or_fallback(&db, PRICING_FRESHNESS))
        .context("sync pricing catalog")?;

    let prices = db.get_all_pricing().context("read pricing table")?;
    let costs_updated = if prices.is_empty() {
        0
    } else {
        db.recalc_costs(&prices, calc_cost)
            .context("recalculate costs after sync")?
    };

    print_sync_summary(outcome, costs_updated)
}

/// Render the pricing sync outcome + cost recompute count to stdout.
fn print_sync_summary(outcome: PricingSyncOutcome, costs_updated: usize) -> Result<()> {
    let mut out = std::io::stdout().lock();
    match outcome {
        PricingSyncOutcome::StillFresh { models } => {
            writeln!(
                out,
                "Pricing cache is fresh: {models} models (no network fetch)"
            )?;
        }
        PricingSyncOutcome::FetchedFromNetwork { models } => {
            writeln!(out, "Pricing refreshed from litellm: {models} models")?;
        }
        PricingSyncOutcome::UsedFallback { models } => {
            writeln!(
                out,
                "Pricing fetch failed; used embedded fallback: {models} models"
            )?;
        }
    }
    writeln!(out, "Re-priced {costs_updated} usage rows.")?;
    Ok(())
}

/// Emit a compact one-line-per-source summary table to stdout.
///
/// Acquired-handle `writeln!` instead of `println!` to keep `clippy::print_stdout`
/// enforced (same rationale as [`print_version`]).
fn print_scan_summary(summaries: &[ScanSummary], costs_updated: usize) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "Scan complete:")?;
    for s in summaries {
        writeln!(
            out,
            "  {:<8} files={:<4} records={:<5} prompts={:<5} sessions={:<3} errors={}",
            format!("{}:", s.source),
            s.files_scanned,
            s.records_inserted,
            s.prompts_inserted,
            s.sessions_touched,
            s.errors.len(),
        )?;
    }
    writeln!(out, "  costs:   {costs_updated} rows re-priced")?;
    Ok(())
}

/// Write the long-form version block to stdout.
///
/// Output shape (example):
///
/// ```text
/// agent-token-usage-tui 0.1.0
/// commit: abcdef012  build: 2026-04-19
/// rust:   1.85.0
/// ```
///
/// `ATUT_GIT_HASH` and `ATUT_BUILD_DATE` are injected by `build.rs`. When
/// either is unavailable (crates.io tarball, system clock broken) we
/// substitute "unknown" rather than failing — a partial version string is
/// still useful for bug reports.
fn print_version() -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    )?;

    let git = option_env!("ATUT_GIT_HASH").unwrap_or("");
    let date = option_env!("ATUT_BUILD_DATE").unwrap_or("");
    let commit = if git.is_empty() { "unknown" } else { git };
    let built = if date.is_empty() { "unknown" } else { date };
    writeln!(out, "commit: {commit}  build: {built}")?;

    // `rustc_version_runtime` would cost a dep; CARGO_PKG_RUST_VERSION is the
    // MSRV declared in Cargo.toml, which is the more useful bug-report field.
    writeln!(
        out,
        "rust:   {}",
        option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown"),
    )?;
    Ok(())
}
