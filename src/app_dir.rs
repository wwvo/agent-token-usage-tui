//! Portable application directory resolution.
//!
//! `agent-token-usage-tui` is designed to be fully portable: configuration,
//! database, logs, and pricing cache all live **next to the executable** rather
//! than in per-user directories like `%APPDATA%` or `~/.config`. This module
//! centralizes path resolution so higher layers never hard-code these strings.
//!
//! Under `cargo run` during development, `std::env::current_exe()` returns a
//! path inside `target/<profile>/`, which is exactly the intended portable
//! behavior — the workspace becomes the runtime directory automatically.
//!
//! # Errors
//!
//! All public helpers return [`anyhow::Result`] so the caller can attach
//! context. The underlying failures are either:
//! * `std::env::current_exe()` failing (rare; permissions / procfs issues), or
//! * The resolved executable path having no parent (theoretically impossible
//!   on supported platforms but handled defensively).

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;

/// Default file name for the optional TOML config.
const CONFIG_FILENAME: &str = "config.toml";

/// Default file name for the SQLite database.
const DB_FILENAME: &str = "data.db";

/// Directory name (relative to `exe_dir`) holding daily rolling log files.
const LOG_DIRNAME: &str = "log";

/// Cached litellm pricing JSON (last successful sync).
const PRICING_CACHE_FILENAME: &str = "pricing.json";

/// Returns the directory that contains the current executable.
///
/// During `cargo run` this is `target/<profile>/`, which is the correct portable
/// behavior for development. In release builds shipped to users it is wherever
/// they dropped the binary.
pub fn exe_dir() -> Result<PathBuf> {
    let exe = env::current_exe().context("failed to read current executable path")?;
    let parent = exe
        .parent()
        .ok_or_else(|| anyhow!("executable path has no parent directory: {}", exe.display()))?;
    Ok(parent.to_path_buf())
}

/// Path to the optional `config.toml` living next to the executable.
///
/// The file does **not** need to exist; callers should treat a missing file as
/// "use defaults".
pub fn config_path() -> Result<PathBuf> {
    Ok(exe_dir()?.join(CONFIG_FILENAME))
}

/// Path to the SQLite database file.
pub fn db_path() -> Result<PathBuf> {
    Ok(exe_dir()?.join(DB_FILENAME))
}

/// Directory holding the daily rolling log files (`log/YYYY-MM-DD.log`).
///
/// Creates the directory (and any missing parents) on first access. Subsequent
/// calls are cheap because `fs::create_dir_all` short-circuits when the path
/// already exists.
pub fn log_dir() -> Result<PathBuf> {
    let dir = exe_dir()?.join(LOG_DIRNAME);
    fs::create_dir_all(&dir).with_context(|| format!("create log directory {}", dir.display()))?;
    Ok(dir)
}

/// Path to the cached litellm pricing JSON (most recent successful sync).
///
/// The file may not exist on first run; `pricing::sync_or_fallback` will use
/// the compile-time embedded fallback in that case.
pub fn pricing_cache_path() -> Result<PathBuf> {
    Ok(exe_dir()?.join(PRICING_CACHE_FILENAME))
}

#[cfg(test)]
#[path = "app_dir_tests.rs"]
mod tests;
