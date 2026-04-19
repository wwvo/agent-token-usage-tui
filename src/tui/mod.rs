//! Terminal UI (ratatui + crossterm), populated in M5–M6.
//!
//! Until M5 C6 lands, [`run`] returns a descriptive error so callers don't
//! silently hang waiting for a non-existent UI loop.

use anyhow::Result;
use anyhow::anyhow;

/// Launch the k9s-style TUI.
///
/// Currently a stub: the real implementation (event loop, tab bar, Overview /
/// Sessions / Models / Trend views) lands in M5.
pub async fn run() -> Result<()> {
    Err(anyhow!(
        "TUI entry point is implemented in M5 C6; use `scan` / `sync-prices` / `version` for now",
    ))
}
