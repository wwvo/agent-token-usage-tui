//! Session file collectors for each supported agent.
//!
//! # Architecture
//!
//! Each agent is its own struct implementing [`Collector`]; the [`pipeline`]
//! module (M4 C4) drives a `Vec<Box<dyn Collector>>`-equivalent via an
//! [`AnyCollector`] enum so we avoid `async_trait` overhead.
//!
//! # Roll-in schedule
//!
//! | Phase | Collector |
//! |-------|-----------|
//! | M3 C3 | Claude Code (`~/.claude/projects/**/*.jsonl`) |
//! | M3 C5 | Codex (`~/.codex/sessions/**/*.jsonl`) |
//! | M4 C1 | OpenClaw (`<base>/<agent>/sessions/*.jsonl`) |
//! | M4 C2 | OpenCode (SQLite read-only) |
//! | M4 C3 | Windsurf (placeholder; real exporter lands in Phase 2) |

pub mod claude;
pub mod codex;
pub mod openclaw;
pub mod reporter;
pub mod util;

pub use claude::ClaudeCollector;
pub use codex::CodexCollector;
pub use openclaw::OpenClawCollector;
pub use reporter::ChannelReporter;
pub use reporter::NoopReporter;
pub use reporter::Reporter;
pub use reporter::ScanProgress;

use anyhow::Result;

use crate::domain::Source;
use crate::storage::Db;

/// Per-collector result reported after a single scan pass.
///
/// Aggregates the counters the pipeline needs to print a summary and the
/// per-file errors that the collector swallowed to keep going.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanSummary {
    pub source: Source,
    /// Usage records the DB reports as newly inserted (post-dedup).
    pub records_inserted: usize,
    /// Prompt events the DB reports as newly inserted (post-dedup).
    pub prompts_inserted: usize,
    /// Number of sessions touched (upserted) during this scan.
    pub sessions_touched: usize,
    /// Number of source files walked; convenient for progress reporting.
    pub files_scanned: usize,
    /// Per-file errors swallowed so the rest of the scan could continue.
    /// The outer pipeline logs these via `tracing::warn` but never aborts.
    pub errors: Vec<String>,
}

impl ScanSummary {
    /// Construct an empty summary anchored to `source`.
    #[must_use]
    pub fn new(source: Source) -> Self {
        Self {
            source,
            records_inserted: 0,
            prompts_inserted: 0,
            sessions_touched: 0,
            files_scanned: 0,
            errors: Vec::new(),
        }
    }
}

/// Unified contract every agent collector implements.
///
/// Collectors walk their own on-disk format(s) and funnel records / prompts /
/// sessions into [`Db`]. They're expected to:
///
/// * honor `Db::get_file_state` / `set_file_state` for incremental scans,
/// * emit [`ScanProgress`] events via the provided [`Reporter`] at file
///   boundaries (not per-line — that would flood the TUI channel),
/// * swallow per-file parse errors into [`ScanSummary::errors`] rather than
///   aborting the whole scan.
///
/// `async fn` in traits is native to Rust 1.75+; we deliberately do **not**
/// expose this trait as `dyn Collector` — the pipeline uses an
/// [`AnyCollector`] enum instead, which keeps dispatch static-friendly and
/// avoids the `async_trait` crate.
pub trait Collector: Send + Sync {
    /// Which agent this collector targets.
    fn source(&self) -> Source;

    /// Walk the agent's on-disk data and apply deltas to `db`.
    ///
    /// Desugared return type (`impl Future<Output = ...> + Send` instead of
    /// `async fn`) so the pipeline can spawn the returned future onto a
    /// multi-threaded tokio runtime without losing `Send` bounds. Concrete
    /// implementors use `async fn` in their `impl Collector for ...` block
    /// and return an `async move { ... }` block inline.
    fn scan(
        &self,
        db: &Db,
        reporter: &dyn Reporter,
    ) -> impl std::future::Future<Output = Result<ScanSummary>> + Send;
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
