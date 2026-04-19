//! Coding agent session metadata.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::source::Source;

/// Metadata for a coding agent session (chat).
///
/// Sessions aggregate many [`crate::domain::UsageRecord`] entries. The fields
/// here are the *slow-moving* attributes: where the session started, which
/// project, which git branch, and how many user prompts it contains.
///
/// `UPSERT` semantics in the DB layer merge non-empty string fields and keep
/// the **earliest** `start_time` across collector passes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub source: Source,
    pub session_id: String,
    pub project: String,
    pub cwd: String,
    pub version: String,
    pub git_branch: String,
    pub start_time: DateTime<Utc>,
    pub prompts: i64,
}
