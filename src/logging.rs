//! Structured logging initialization.
//!
//! CLI subcommands stream to stderr; the TUI has to use file logging because
//! a raw-mode terminal cannot tolerate direct stderr writes (they corrupt
//! the alt-screen buffer). Callers pick via [`LogMode`] at startup.
//!
//! # Invariants
//!
//! * `init` must be called exactly once before any `tracing::*!` macro fires,
//!   otherwise logs go to the null sink.
//! * Repeat calls are benign (the underlying global subscriber is install-once,
//!   so we swallow `TryInitError`).

use std::io;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use tracing_subscriber::EnvFilter;

use crate::app_dir;

/// Default tracing level when `RUST_LOG` is unset.
const DEFAULT_FILTER: &str = "info";

/// File prefix for the daily rolling log inside `EXE_DIR/log/`.
///
/// `tracing_appender` writes `<prefix>.<YYYY-MM-DD>`; we use `atut.log` so the
/// result is e.g. `log/atut.log.2026-04-19`.
const LOG_FILE_PREFIX: &str = "atut.log";

/// Destination for `tracing` events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogMode {
    /// Human-readable logs on stderr. Used by CLI subcommands (`scan`,
    /// `sync-prices`) and dev invocations.
    Stderr,

    /// Daily rolling logs at `EXE_DIR/log/atut.log.YYYY-MM-DD`. TUI mode must
    /// use this because raw-mode terminals cannot tolerate direct stderr writes.
    File,
}

/// Install the global `tracing` subscriber.
///
/// # Errors
///
/// * `LogMode::File` fails if the log directory cannot be created or the
///   `EnvFilter` is malformed.
/// * `LogMode::Stderr` can fail only if the `EnvFilter` literal (default or
///   `RUST_LOG`) is malformed; `try_init` conflicts (subscriber already
///   installed) are treated as benign.
pub fn init(mode: LogMode) -> Result<()> {
    match mode {
        LogMode::Stderr => init_stderr(),
        LogMode::File => init_file(None),
    }
}

/// File-mode variant that lets callers override the log directory (tests pin
/// this to a tempdir so they don't write into `target/debug/`).
pub fn init_file_into(dir: PathBuf) -> Result<WorkerGuard> {
    init_file_inner(dir)
}

fn init_stderr() -> Result<()> {
    let filter = build_filter()?;

    let _ = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(filter)
        .try_init();

    Ok(())
}

fn init_file(override_dir: Option<PathBuf>) -> Result<()> {
    let dir = match override_dir {
        Some(d) => d,
        None => app_dir::log_dir().context("resolve log dir")?,
    };
    let _guard = init_file_inner(dir)?;
    // Intentional leak: the guard must outlive every future log write, which
    // is the entire process lifetime for a CLI / TUI binary. Dropping here
    // would flush and stop the background appender thread.
    std::mem::forget(_guard);
    Ok(())
}

fn init_file_inner(dir: PathBuf) -> Result<WorkerGuard> {
    std::fs::create_dir_all(&dir).with_context(|| format!("create log dir {}", dir.display()))?;

    let file_appender = tracing_appender::rolling::daily(&dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = build_filter()?;

    let _ = tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_env_filter(filter)
        .try_init();

    Ok(guard)
}

fn build_filter() -> Result<EnvFilter> {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(DEFAULT_FILTER))
        .context("build tracing env filter")
}

/// Non-blocking appender guard. Must be held for the rest of the process
/// lifetime (we `std::mem::forget` it internally for the "production" path).
pub type WorkerGuard = tracing_appender::non_blocking::WorkerGuard;

#[cfg(test)]
#[path = "logging_tests.rs"]
mod tests;
