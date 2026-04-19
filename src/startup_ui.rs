//! Pre-TUI startup progress reporting.
//!
//! Runs for the 3–5 seconds between "user types `atut`" and "TUI alt-screen
//! takes over". Without this module the user sees a blank terminal during
//! pricing sync + full scan; with it they see a running checklist:
//!
//! ```text
//! [OK] Syncing pricing        42 models
//! [OK] Scanning Claude        18 recs / 3 prompts / 2 files
//! [..] Scanning Codex...
//! ```
//!
//! # Output hygiene
//!
//! Writes go to **stderr** rather than stdout because:
//!
//! 1. stdout is reserved for machine-readable output (`atut version`, future
//!    `atut summary --json`). Users piping `atut | head` should not see
//!    interactive progress.
//! 2. The TUI enters alt-screen on stdout immediately after us; any stdout
//!    we wrote would still be visible below the alt-screen on exit and
//!    clutter the user's shell history.
//!
//! The crate-wide `clippy::print_stderr = "deny"` lint is intentionally
//! overridden here with a module-scoped allow — this is the one module
//! where direct stderr writes are the point.

#![allow(clippy::print_stderr)]

use std::io::Write;

use crate::collector::Reporter;
use crate::collector::ScanProgress;
use crate::collector::ScanSummary;
use crate::domain::Source;

/// Reporter implementation that draws a live progress checklist to stderr.
///
/// Stateless: each callback writes a self-contained line using ANSI `\r`
/// (carriage return) to overwrite the in-progress line with the done line.
/// No locking, no mutex — `Write::flush` on `io::stderr()` is already
/// serialized by the OS.
#[derive(Clone, Copy, Debug, Default)]
pub struct StartupReporter;

impl StartupReporter {
    /// Print the opening "[  ] {name}..." line without a trailing newline.
    ///
    /// Callers pair this with a subsequent [`Self::step_done`] /
    /// [`Self::step_warn`] that starts with `\r` to overwrite the line.
    pub fn step_start(&self, name: &str) {
        eprint!("[  ] {name}...");
        // Flush immediately so the user sees the pending step even when
        // stderr is line-buffered (tty default) or when the next syscall
        // takes seconds (pricing sync).
        let _ = std::io::stderr().flush();
    }

    /// Overwrite the in-progress line with `\[OK\] {name}  {detail}`.
    ///
    /// `detail` is a trailing annotation (e.g. "42 models", "18 recs / …")
    /// that gives the user just enough context to see the work *happened*.
    pub fn step_done(&self, name: &str, detail: &str) {
        // Pad to 60 columns so shorter "done" lines fully overwrite longer
        // "in-progress" prefixes; the extra spaces are harmless on terminals
        // that already auto-wrap or scroll.
        eprintln!("\r[OK] {name}  {detail:<40}");
    }

    /// Overwrite the in-progress line with a warning (non-fatal error).
    ///
    /// Used for partial failures like a pricing sync that couldn't reach
    /// the network but fell back to the embedded snapshot.
    pub fn step_warn(&self, name: &str, detail: &str) {
        eprintln!("\r[!!] {name}  {detail:<40}");
    }
}

impl Reporter for StartupReporter {
    fn on_progress(&self, _: ScanProgress) {
        // Startup UI only reports at source boundaries; per-file ticks would
        // flood stderr and hide the readable "N sources scanned" summary.
    }

    fn on_source_start(&self, source: Source) {
        self.step_start(&format!("scanning {source}"));
    }

    fn on_source_finished(&self, source: Source, summary: &ScanSummary) {
        let detail = format!(
            "{} recs / {} prompts / {} files",
            summary.records_inserted, summary.prompts_inserted, summary.files_scanned,
        );
        self.step_done(&format!("scanning {source}"), &detail);
    }
}

#[cfg(test)]
#[path = "startup_ui_tests.rs"]
mod tests;
