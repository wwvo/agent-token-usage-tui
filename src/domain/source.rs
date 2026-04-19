//! `Source` enumerates every coding agent this tool ingests.
//!
//! Using an enum (rather than raw strings) catches typos at compile time and
//! guarantees that the pipeline can exhaustively match on "which agent".

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// Agents whose local session data we aggregate.
///
/// Serialized representation is lowercase (`"claude"`, `"codex"`, ...), matching
/// how the SQLite `source` column stores values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Claude,
    Codex,
    /// Serialized as `"openclaw"` (all lowercase, no separator) to match the
    /// agent-usage SQLite convention. `rename_all = "lowercase"` drops the
    /// internal capital without inserting an underscore.
    OpenClaw,
    /// Serialized as `"opencode"` for the same reason as `OpenClaw`.
    OpenCode,
    /// Placeholder until Phase 2 lands the VSCode exporter. `collector::windsurf`
    /// already exists so the enum can stay exhaustive.
    Windsurf,
}

impl Source {
    /// Stable lowercase identifier used in the DB `source` column, file paths,
    /// and JSON serialization.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenClaw => "openclaw",
            Self::OpenCode => "opencode",
            Self::Windsurf => "windsurf",
        }
    }

    /// All variants, in display order (used by the TUI source tally and CLI
    /// summary output).
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Claude,
            Self::Codex,
            Self::OpenClaw,
            Self::OpenCode,
            Self::Windsurf,
        ]
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing a string into a [`Source`] fails.
#[derive(Debug, Error)]
#[error("unknown agent source: {0}")]
pub struct UnknownSource(pub String);

impl FromStr for Source {
    type Err = UnknownSource;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "openclaw" => Ok(Self::OpenClaw),
            "opencode" => Ok(Self::OpenCode),
            "windsurf" => Ok(Self::Windsurf),
            other => Err(UnknownSource(other.to_owned())),
        }
    }
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
