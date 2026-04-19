//! Windsurf collector — Phase 2 placeholder.
//!
//! Windsurf does not expose its per-call token usage in any persistent store
//! we can tail today (in-memory only; see the research note in plan/). The
//! real implementation lands in Phase 2 as a companion **VSCode extension**
//! (`agent-token-usage-exporter`) that observes the IDE's LLM requests and
//! writes session JSONL in the same shape we already consume elsewhere.
//!
//! For now this collector is a no-op whose only job is to keep
//! `Source::Windsurf` first-class in the pipeline so the TUI can show a
//! "source coverage" table without special-casing a missing variant. We
//! intentionally log at `info!` level (not `warn!`) so the user's scan output
//! stays calm — this is expected behavior, not a degradation.

use std::path::PathBuf;

use anyhow::Result;

use crate::collector::Collector;
use crate::collector::Reporter;
use crate::collector::ScanSummary;
use crate::domain::Source;
use crate::storage::Db;

/// Stub collector; flips to a real implementation when the Phase 2 exporter ships.
pub struct WindsurfCollector {
    /// Preserved for symmetry with the other collectors and for forward
    /// compatibility: once the VSCode exporter writes to a configurable path,
    /// this is where we'll honor it.
    #[allow(dead_code)]
    base_paths: Vec<PathBuf>,
}

impl WindsurfCollector {
    /// Build the placeholder with (currently ignored) base paths.
    #[must_use]
    pub fn new(base_paths: Vec<PathBuf>) -> Self {
        Self { base_paths }
    }
}

impl Collector for WindsurfCollector {
    fn source(&self) -> Source {
        Source::Windsurf
    }

    // `async fn` sugar would be enough here (no sync prelude), but we keep the
    // desugared signature to match every other collector in this module and
    // leave an obvious place to plug in discovery logic once Phase 2 ships.
    #[allow(clippy::manual_async_fn)]
    fn scan(
        &self,
        _db: &Db,
        _reporter: &dyn Reporter,
    ) -> impl std::future::Future<Output = Result<ScanSummary>> + Send {
        async move {
            tracing::info!(
                "windsurf: no on-disk session store available yet; real collector lands with the Phase 2 VSCode exporter"
            );
            Ok(ScanSummary::new(Source::Windsurf))
        }
    }
}

#[cfg(test)]
#[path = "windsurf_tests.rs"]
mod tests;
