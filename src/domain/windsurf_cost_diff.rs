//! One `checkpoint_cost` snapshot written by the Windsurf exporter.
//!
//! Mirrors the wire format in `tools/windsurf-exporter/src/writer.ts::
//! CheckpointCostLine`. Deliberately separate from [`super::UsageRecord`]:
//! the two quantities answer different questions (ours vs server's) and
//! merging them would double-count. See `migrations/003_windsurf_cost_
//! diffs.sql` for the matching storage schema.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

/// One server-side cost snapshot captured at a Windsurf CHECKPOINT step.
///
/// Field layout matches `windsurf_cost_diffs` 1:1 so the storage layer
/// can bind positionally without a translation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindsurfCostDiff {
    /// Server-assigned `executionId` UUID. Unique per checkpoint
    /// step — the storage layer uses it as the table's primary key.
    pub step_id: String,
    /// Cascade this checkpoint belongs to; mirrors
    /// `windsurf_sessions.cascade_id` + `usage_records.session_id`.
    pub cascade_id: String,
    /// Checkpoint timestamp (preferred: per-step `metadata.createdAt`,
    /// fallback: cascade-level times). Never `1970-01-01` — the
    /// exporter bails on empty timestamps before we ever see them.
    pub timestamp: DateTime<Utc>,
    /// Model UID the checkpoint was generated for. Empty string if
    /// Windsurf's step metadata didn't carry one.
    pub model: String,
    /// Windsurf's own USD estimate for the cascade up to this
    /// checkpoint.
    pub server_cost_usd: f64,
    /// Server-reported input token count, `0` when unreported.
    pub server_input_tokens: i64,
    /// Server-reported output token count, `0` when unreported.
    pub server_output_tokens: i64,
    /// Server-reported cache-read token count, `0` when unreported.
    pub server_cache_read_tokens: i64,
}
