//! User configuration loaded from `<exe-dir>/config.toml`.
//!
//! The schema is deliberately sparse: Claude and Codex use `$HOME`-derived
//! defaults and never appear here, so most users never need to touch
//! `config.toml`. The fields below are reserved for agents whose storage
//! paths can't be auto-detected:
//!
//! ```toml
//! # config.toml — all fields optional.
//! openclaw_bases = ["/home/u/.local/share/openclaw"]
//! opencode_dbs   = ["/home/u/.local/share/opencode/opencode.db"]
//! windsurf_bases = []  # reserved for Phase 2 VSCode exporter
//! ```
//!
//! When the file is absent or empty, every field defaults to `Vec::new()` and
//! those collectors simply find no files to scan.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::pipeline::PipelineConfig;

/// Runtime configuration sourced from `config.toml`.
///
/// Uses `#[serde(default)]` on every field so a partial file (or a totally
/// empty one) still deserializes successfully.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// OpenClaw base directories (`<base>/<agent>/sessions/*.jsonl`).
    pub openclaw_bases: Vec<PathBuf>,
    /// OpenCode SQLite database files.
    pub opencode_dbs: Vec<PathBuf>,
    /// Windsurf exporter output — reserved for the Phase 2 VSCode extension.
    pub windsurf_bases: Vec<PathBuf>,
}

impl Config {
    /// Load a config from the given TOML path.
    ///
    /// Returns `Ok(Config::default())` when the file does not exist —
    /// having *no* config is the expected common case for Claude / Codex-only
    /// users; it is not an error.
    ///
    /// # Errors
    ///
    /// Reports I/O errors for unreadable files and parse errors for malformed
    /// TOML so the user can see exactly which key broke.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            tracing::info!(
                path = %path.display(),
                "no config.toml found; using defaults",
            );
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read config {}", path.display()))?;
        let cfg: Self =
            toml::from_str(&text).with_context(|| format!("parse config {}", path.display()))?;
        Ok(cfg)
    }

    /// Project the config onto the subset the scan pipeline needs.
    #[must_use]
    pub fn to_pipeline(&self) -> PipelineConfig {
        PipelineConfig {
            openclaw_bases: self.openclaw_bases.clone(),
            opencode_dbs: self.opencode_dbs.clone(),
            windsurf_bases: self.windsurf_bases.clone(),
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
