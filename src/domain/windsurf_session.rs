//! Per-cascade Windsurf session metadata.
//!
//! Distinct from [`super::SessionRecord`]: that type captures the fields
//! shared across every agent source (project / cwd / version / git branch /
//! prompt count), whereas this one holds Windsurf-specific presentation
//! data that the official Cascade UI surfaces — the human-readable
//! `summary` title and the cascade's `created_time`. The TUI's planned
//! per-cascade drill-down view (see `plans/windsurf-exporter-future-
//! improvements.md`, "View B") relies on this as its row shape.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

/// One Windsurf cascade's metadata row.
///
/// `cascade_id` doubles as both the primary key in `windsurf_sessions`
/// AND the join key against `usage_records.session_id` / `sessions.
/// session_id` — we deliberately use the same value on every layer so
/// JOINs stay direct and no translation layer is needed between the
/// Windsurf-specific presentation and the cross-source usage data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindsurfSessionRecord {
    /// Cascade UUID, assigned server-side by the Windsurf Language
    /// Server. Stable for the lifetime of the cascade.
    pub cascade_id: String,
    /// Free-form human-readable title shown in Cascade's own UI
    /// (e.g. "Generating Git Commit Message"). May be empty when the
    /// exporter hasn't yet seen a `session_meta` line for this file.
    pub summary: String,
    /// Workspace URI the cascade was opened in, as reported by the
    /// exporter. May be empty — Windsurf itself doesn't always populate
    /// the `summary.workspaces` array (~32% gap on the dev machine per
    /// the v0.2.9 retention probe).
    pub workspace: String,
    /// Model UID the cascade last generated a response with.
    pub last_model: String,
    /// Server-recorded cascade creation time. `None` when the exporter
    /// didn't capture it (older JSONL files or a crash before the first
    /// `session_meta` flush); the storage layer falls back to `last_seen`
    /// for ordering in that case.
    pub created_time: Option<DateTime<Utc>>,
    /// Most recent time the collector observed this cascade in a scan.
    /// Monotonically non-decreasing across passes — the storage upsert
    /// keeps the greater of the two on conflict.
    pub last_seen: DateTime<Utc>,
}
