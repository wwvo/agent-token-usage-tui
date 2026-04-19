//! User prompt events.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use super::source::Source;

/// A single real user prompt (not a tool result or streaming chunk).
///
/// Collectors filter out tool-result `user` messages before emitting this, so
/// the `prompt_events` table counts *actual* human-typed prompts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromptEvent {
    pub source: Source,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
}
