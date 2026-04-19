//! User configuration loaded from `<exe-dir>/config.toml`.
//!
//! The full schema lands in later phases (default paths, per-source enable
//! flags, pricing overrides). For now this is a placeholder so modules that
//! will plug into it can `use crate::config::Config;` without churn.

/// Runtime configuration.
///
/// Defaults are chosen so that running the binary with **no** `config.toml`
/// does the right thing for the vast majority of users (portable mode,
/// auto-detected agent paths, 24h pricing freshness, etc.).
#[derive(Debug, Clone, Default)]
pub struct Config {}
