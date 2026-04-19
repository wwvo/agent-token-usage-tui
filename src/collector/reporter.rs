//! Scan progress reporting.
//!
//! Collectors emit [`ScanProgress`] events through a [`Reporter`] so the caller
//! (CLI in M4 C5, TUI in M5 C5) can show live status. Two built-in
//! implementations ship:
//!
//! * [`NoopReporter`] — swallow everything; used by the `scan` subcommand and
//!   unit tests that don't care about progress.
//! * [`ChannelReporter`] — forward to a `tokio::sync::mpsc::Sender`; used by
//!   the TUI backing task to pipe updates into the Elm-style event loop.

use std::path::PathBuf;

use tokio::sync::mpsc::Sender;

use super::ScanSummary;
use crate::domain::Source;

/// One progress tick reported from a collector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    pub source: Source,
    pub files_done: usize,
    pub files_total: usize,
    pub current_file: Option<PathBuf>,
}

/// Contract every progress sink implements.
///
/// `Send + Sync` so any reporter can be shared via `&dyn Reporter` across async
/// tasks; every method is deliberately synchronous — collectors call them from
/// hot paths and we don't want to sprinkle `.await` across them.
pub trait Reporter: Send + Sync {
    /// Non-blocking delivery of one tick. Implementations that use bounded
    /// channels SHOULD drop (not block) when full so slow UIs never stall a
    /// scan.
    fn on_progress(&self, progress: ScanProgress);

    /// Called by the pipeline **once** per source before its collector runs.
    ///
    /// Default is a no-op so existing impls (tests, `NoopReporter`) keep
    /// working without churn. The TUI startup reporter overrides this to
    /// print a "[  ] scanning {source}..." line.
    fn on_source_start(&self, source: Source) {
        let _ = source;
    }

    /// Called by the pipeline **once** per source after its collector returns.
    ///
    /// `summary` is the exact [`ScanSummary`] that will appear in
    /// [`crate::pipeline::ScanReport::summaries`].
    fn on_source_finished(&self, source: Source, summary: &ScanSummary) {
        let _ = (source, summary);
    }
}

/// `Reporter` that drops every tick on the floor.
///
/// The default for non-interactive contexts (CLI `scan` subcommand, tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopReporter;

impl Reporter for NoopReporter {
    fn on_progress(&self, _: ScanProgress) {}
}

/// `Reporter` that forwards ticks into a bounded tokio mpsc channel.
///
/// Uses `try_send` so collectors never block waiting for the UI; when the
/// channel is full (TUI is redrawing slower than the scan is emitting), the
/// progress tick is dropped — the TUI's next tick still sees up-to-date
/// aggregate state via the DB, so a missed progress event is cosmetic only.
#[derive(Clone, Debug)]
pub struct ChannelReporter {
    tx: Sender<ScanProgress>,
}

impl ChannelReporter {
    #[must_use]
    pub const fn new(tx: Sender<ScanProgress>) -> Self {
        Self { tx }
    }
}

impl Reporter for ChannelReporter {
    fn on_progress(&self, progress: ScanProgress) {
        // Intentionally ignore send errors: channel full or receiver dropped
        // are both acceptable "UI is slower than the scan" situations.
        let _ = self.tx.try_send(progress);
    }
}

#[cfg(test)]
#[path = "reporter_tests.rs"]
mod tests;
