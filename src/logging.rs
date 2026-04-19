//! Structured logging initialization.
//!
//! M1 baseline: every subcommand streams `tracing` events to stderr with an
//! `EnvFilter` respecting `RUST_LOG`. M7 C1 will plug in a daily-rolling file
//! writer via `tracing_appender::rolling::daily` for TUI mode — the public
//! surface here (`LogMode::File`) already carves out that slot so upstream
//! callers don't need to change.
//!
//! # Invariants
//!
//! * `init` must be called exactly once before any `tracing::*!` macro fires,
//!   otherwise logs go to the null sink.
//! * Repeat calls are benign (the underlying global subscriber is install-once,
//!   so we swallow `TryInitError`).

use std::io;

use anyhow::Context;
use anyhow::Result;
use tracing_subscriber::EnvFilter;

/// Default tracing level when `RUST_LOG` is unset.
const DEFAULT_FILTER: &str = "info";

/// Destination for `tracing` events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogMode {
    /// Human-readable logs on stderr. Used by CLI subcommands (`scan`,
    /// `sync-prices`) and during M1–M6 development when there is no TUI yet.
    Stderr,

    /// Daily rolling logs at `EXE_DIR/log/YYYY-MM-DD.log`. TUI mode must use
    /// this because raw-mode terminals cannot tolerate direct stderr writes.
    ///
    /// Implemented in M7 C1; calling `init(LogMode::File)` before that returns
    /// an `anyhow::Error` pointing at this commit.
    File,
}

/// Install the global `tracing` subscriber.
///
/// # Errors
///
/// * `LogMode::File` currently returns an error referencing M7 C1, where the
///   rolling file writer lands.
/// * `LogMode::Stderr` can fail only if the `EnvFilter` literal (default or
///   `RUST_LOG`) is malformed; `try_init` conflicts (subscriber already
///   installed) are treated as benign.
pub fn init(mode: LogMode) -> Result<()> {
    match mode {
        LogMode::Stderr => init_stderr(),
        LogMode::File => Err(anyhow::anyhow!(
            "LogMode::File is implemented in M7 C1 (rolling daily writer); use LogMode::Stderr for now",
        )),
    }
}

fn init_stderr() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(DEFAULT_FILTER))
        .context("build tracing env filter")?;

    // `try_init` returns Err when another subscriber is already installed
    // (common in tests where multiple test cases may each call `init`). Treat
    // repeat installs as benign — the first one wins.
    let _ = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(filter)
        .try_init();

    Ok(())
}

#[cfg(test)]
#[path = "logging_tests.rs"]
mod tests;
