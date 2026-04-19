//! Per-file scan checkpoint and parser context.
//!
//! Incremental scanning is the foundation that keeps repeat `scan` runs cheap:
//! each collector records where it stopped reading a given JSONL/SQLite file,
//! so the next pass only processes *new* bytes / rows.
//!
//! The [`FileScanContext`] travels alongside the raw byte offset because some
//! session formats (notably Codex) declare metadata once at the top of the
//! file and reference it implicitly for the rest — without snapshotting that
//! context, an incremental resume would mis-attribute sessions.

use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use rusqlite::OptionalExtension;
use rusqlite::params;
use serde::Deserialize;
use serde::Serialize;

use super::Db;

/// Parser state persisted alongside the scan offset.
///
/// Fields correspond to values that appear at the top of a session file and
/// need to stay in scope for the rest of it. Collectors populate whichever
/// fields their format emits; absent ones stay empty.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct FileScanContext {
    /// Session identifier (Codex `session_meta`, Claude filename, ...).
    pub session_id: String,
    /// Working directory in which the session ran.
    pub cwd: String,
    /// Client version (e.g. Codex CLI `v0.42.0`).
    pub version: String,
    /// Model identifier currently active (Codex `turn_context.model`).
    pub model: String,
}

impl Db {
    /// Read the checkpoint for `path`.
    ///
    /// Returns `(size, last_offset, context)` where `size` is the file size as
    /// of the previous scan (used to detect truncation), `last_offset` is the
    /// byte offset to resume from, and `context` is the serialized parser state.
    ///
    /// When `path` has never been scanned the tuple is `(0, 0, None)`.
    pub fn get_file_state(&self, path: &Path) -> Result<(i64, i64, Option<FileScanContext>)> {
        let conn = self.lock();
        let row: Option<(i64, i64, Option<String>)> = conn
            .query_row(
                "SELECT size, last_offset, scan_context FROM file_state WHERE path = ?1",
                params![path.to_string_lossy().as_ref()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?)),
            )
            .optional()
            .context("query file_state")?;

        let (size, offset, raw) = row.unwrap_or_default();
        let ctx = raw
            .filter(|s| !s.is_empty())
            .and_then(|s| serde_json::from_str(&s).ok());
        Ok((size, offset, ctx))
    }

    /// Persist the checkpoint for `path`.
    ///
    /// Upserts on `path`; pass the updated `size` (file length after the scan)
    /// and `offset` (typically equal to `size` once the tail has been consumed).
    /// `ctx` is serialized as JSON; pass `None` when the collector has no
    /// running parser state worth preserving.
    pub fn set_file_state(
        &self,
        path: &Path,
        size: i64,
        offset: i64,
        ctx: Option<&FileScanContext>,
    ) -> Result<()> {
        let raw = match ctx {
            Some(c) => serde_json::to_string(c).context("serialize scan_context")?,
            None => String::new(),
        };
        let conn = self.lock();
        conn.execute(
            "INSERT INTO file_state(path, size, last_offset, scan_context)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET
               size = excluded.size,
               last_offset = excluded.last_offset,
               scan_context = excluded.scan_context",
            params![path.to_string_lossy().as_ref(), size, offset, raw],
        )
        .context("upsert file_state")?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "file_state_tests.rs"]
mod tests;
