//! Batch insert + upsert for usage records, prompt events, and session metadata.
//!
//! Each batch call wraps a single `BEGIN; ... COMMIT;` transaction and uses
//! `prepare_cached` so that N rows cost N executes, not N prepares. Duplicates
//! are quietly ignored by the schema's unique indices:
//!
//! * `idx_usage_dedup` on `(session_id, model, timestamp, input_tokens, output_tokens)`
//! * `idx_prompt_dedup` on `(session_id, timestamp)`
//!
//! `upsert_session` follows agent-usage semantics: non-empty string fields
//! overwrite stale values, `start_time` keeps the earliest observation, and
//! `prompts` accumulates as a delta across repeated scans.

use anyhow::Context;
use anyhow::Result;
use rusqlite::params;

use super::Db;
use crate::domain::PromptEvent;
use crate::domain::SessionRecord;
use crate::domain::UsageRecord;

const INSERT_USAGE_SQL: &str = "\
INSERT OR IGNORE INTO usage_records(
    source, session_id, model,
    input_tokens, output_tokens,
    cache_creation_input_tokens, cache_read_input_tokens,
    reasoning_output_tokens, cost_usd,
    timestamp, project, git_branch
) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

const INSERT_PROMPT_SQL: &str = "\
INSERT OR IGNORE INTO prompt_events(source, session_id, timestamp)
VALUES(?1, ?2, ?3)";

const UPSERT_SESSION_SQL: &str = "\
INSERT INTO sessions(
    source, session_id, project, cwd, version, git_branch, start_time, prompts
) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(session_id) DO UPDATE SET
    project    = CASE WHEN excluded.project    != '' THEN excluded.project    ELSE sessions.project    END,
    cwd        = CASE WHEN excluded.cwd        != '' THEN excluded.cwd        ELSE sessions.cwd        END,
    version    = CASE WHEN excluded.version    != '' THEN excluded.version    ELSE sessions.version    END,
    git_branch = CASE WHEN excluded.git_branch != '' THEN excluded.git_branch ELSE sessions.git_branch END,
    start_time = CASE WHEN excluded.start_time < sessions.start_time THEN excluded.start_time ELSE sessions.start_time END,
    prompts    = sessions.prompts + excluded.prompts";

impl Db {
    /// Insert a batch of usage records; duplicates (per `idx_usage_dedup`) are
    /// silently dropped.
    ///
    /// Returns the number of newly inserted rows.
    pub fn insert_usage_batch(&self, records: &[UsageRecord]) -> Result<usize> {
        if records.is_empty() {
            return Ok(0);
        }

        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .context("begin usage batch transaction")?;

        let mut inserted = 0usize;
        {
            let mut stmt = tx
                .prepare_cached(INSERT_USAGE_SQL)
                .context("prepare usage insert")?;
            for r in records {
                inserted += stmt
                    .execute(params![
                        r.source.as_str(),
                        &r.session_id,
                        &r.model,
                        r.input_tokens,
                        r.output_tokens,
                        r.cache_creation_input_tokens,
                        r.cache_read_input_tokens,
                        r.reasoning_output_tokens,
                        r.cost_usd,
                        r.timestamp,
                        &r.project,
                        &r.git_branch,
                    ])
                    .context("insert usage row")?;
            }
        }

        tx.commit().context("commit usage batch")?;
        Ok(inserted)
    }

    /// Insert a batch of prompt events; duplicates (per `idx_prompt_dedup`) are
    /// silently dropped.
    pub fn insert_prompt_batch(&self, events: &[PromptEvent]) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }

        let mut conn = self.lock();
        let tx = conn
            .transaction()
            .context("begin prompt batch transaction")?;

        let mut inserted = 0usize;
        {
            let mut stmt = tx
                .prepare_cached(INSERT_PROMPT_SQL)
                .context("prepare prompt insert")?;
            for e in events {
                inserted += stmt
                    .execute(params![e.source.as_str(), &e.session_id, e.timestamp])
                    .context("insert prompt row")?;
            }
        }

        tx.commit().context("commit prompt batch")?;
        Ok(inserted)
    }

    /// Upsert session metadata (delta-accumulated `prompts`).
    ///
    /// Merge semantics on conflict (`session_id` match):
    /// * Non-empty string fields (`project`, `cwd`, `version`, `git_branch`)
    ///   overwrite stale values; empty ones preserve what's on disk.
    /// * `start_time` keeps the earliest value ever seen.
    /// * `prompts` accumulates — callers MUST pass the **delta** of prompts
    ///   discovered in the current scan, not the session's cumulative count.
    pub fn upsert_session(&self, s: &SessionRecord) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            UPSERT_SESSION_SQL,
            params![
                s.source.as_str(),
                &s.session_id,
                &s.project,
                &s.cwd,
                &s.version,
                &s.git_branch,
                s.start_time,
                s.prompts,
            ],
        )
        .context("upsert session")?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
