//! OpenCode collector — reads a foreign SQLite database.
//!
//! # Source format
//!
//! Unlike Claude / Codex / OpenClaw, OpenCode persists to a single SQLite
//! file (`opencode.db` or similar) with two relevant tables:
//!
//! ```sql
//! CREATE TABLE session (
//!   id TEXT PRIMARY KEY,
//!   directory TEXT,
//!   ...
//! );
//! CREATE TABLE message (
//!   session_id TEXT,
//!   role TEXT,              -- 'user' | 'assistant' | ...
//!   time_created INTEGER,   -- unix millis
//!   data TEXT               -- JSON blob
//! );
//! ```
//!
//! Assistant messages carry token accounting inside `data` as JSON:
//!
//! ```jsonc
//! { "role":"assistant",
//!   "modelID":"gpt-5-codex",
//!   "tokens": {
//!     "input":N, "output":N, "reasoning":N,
//!     "cache": { "read":N, "write":N }
//!   },
//!   "time":{"created":<ms>,"completed":<ms>}, ... }
//! ```
//!
//! User prompts are matched heuristically by `data LIKE '%"role":"user"%'`
//! (the upstream schema has no structured role column).
//!
//! # Watermark
//!
//! We reuse [`Db::file_state`] to remember the maximum `message.time_created`
//! we've already ingested — the `last_offset` column is repurposed as a
//! millisecond watermark. That keeps the storage schema unchanged at the
//! cost of a single abstraction bent.
//!
//! # Token semantics
//!
//! OpenCode's `tokens.input` is **non-overlapping** w.r.t. cache, matching
//! our DB schema exactly. We copy fields 1:1 plus the reasoning bucket, which
//! no other collector currently fills (GPT-5 reasoning tokens).

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use rusqlite::Connection;
use rusqlite::OpenFlags;
use serde::Deserialize;
use serde_json::from_str;

use crate::collector::Collector;
use crate::collector::Reporter;
use crate::collector::ScanProgress;
use crate::collector::ScanSummary;
use crate::domain::PromptEvent;
use crate::domain::SessionRecord;
use crate::domain::Source;
use crate::domain::UsageRecord;
use crate::storage::Db;

/// Collector for OpenCode's local SQLite store.
pub struct OpenCodeCollector {
    db_paths: Vec<PathBuf>,
}

impl OpenCodeCollector {
    /// Build a collector reading the given OpenCode DB files.
    #[must_use]
    pub fn new(db_paths: Vec<PathBuf>) -> Self {
        Self { db_paths }
    }

    /// Unix epoch in milliseconds → UTC `DateTime`.
    fn millis_to_ts(millis: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp_millis(millis).unwrap_or_else(Utc::now)
    }
}

impl Collector for OpenCodeCollector {
    fn source(&self) -> Source {
        Source::OpenCode
    }

    fn scan(
        &self,
        db: &Db,
        reporter: &dyn Reporter,
    ) -> impl std::future::Future<Output = Result<ScanSummary>> + Send {
        // Clone up front so we don't borrow `self` across the async move.
        let files: Vec<PathBuf> = self
            .db_paths
            .iter()
            .filter(|p| p.exists())
            .cloned()
            .collect();
        let files_total = files.len();
        let source = Source::OpenCode;

        async move {
            let mut summary = ScanSummary::new(source);
            summary.files_scanned = files_total;

            for (idx, path) in files.into_iter().enumerate() {
                reporter.on_progress(ScanProgress {
                    source,
                    files_done: idx,
                    files_total,
                    current_file: Some(path.clone()),
                });

                match process_db(db, &path).await {
                    Ok(stats) => {
                        summary.records_inserted += stats.records_inserted;
                        summary.prompts_inserted += stats.prompts_inserted;
                        summary.sessions_touched += stats.sessions_touched;
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "opencode: failed to process db; continuing",
                        );
                        summary.errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }

            reporter.on_progress(ScanProgress {
                source,
                files_done: files_total,
                files_total,
                current_file: None,
            });
            Ok(summary)
        }
    }
}

// ---- Data shapes mirroring OpenCode's `message.data` JSON ----

#[derive(Debug, Deserialize)]
struct OpenCodeMessageData {
    #[serde(default)]
    role: String,
    #[serde(default, rename = "modelID")]
    model_id: String,
    #[serde(default)]
    tokens: OpenCodeTokens,
    #[serde(default)]
    time: OpenCodeTime,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeTokens {
    #[serde(default)]
    input: i64,
    #[serde(default)]
    output: i64,
    #[serde(default)]
    reasoning: i64,
    #[serde(default)]
    cache: OpenCodeCache,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeCache {
    #[serde(default)]
    read: i64,
    #[serde(default)]
    write: i64,
}

#[derive(Debug, Default, Deserialize)]
struct OpenCodeTime {
    #[serde(default)]
    created: i64,
}

#[derive(Default)]
struct FileStats {
    records_inserted: usize,
    prompts_inserted: usize,
    sessions_touched: usize,
}

/// Process one OpenCode DB file end-to-end.
async fn process_db(db: &Db, path: &Path) -> Result<FileStats> {
    let (_prev_size, last_watermark, _prev_ctx) = db.get_file_state(path)?;

    let src = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open opencode db {}", path.display()))?;

    // Assistant rows past the watermark.
    let mut stmt = src
        .prepare(
            "SELECT m.data, m.session_id, m.time_created, s.directory \
             FROM message m \
             JOIN session s ON m.session_id = s.id \
             WHERE m.time_created > ?1 \
             ORDER BY m.time_created",
        )
        .context("prepare opencode message select")?;

    let mut records: Vec<UsageRecord> = Vec::new();
    let mut sessions: std::collections::BTreeMap<String, SessionRecord> =
        std::collections::BTreeMap::new();
    let mut max_watermark: i64 = last_watermark;

    let rows = stmt
        .query_map([last_watermark], |row| {
            Ok((
                row.get::<_, String>(0)?,         // data (json)
                row.get::<_, String>(1)?,         // session_id
                row.get::<_, i64>(2)?,            // time_created (ms)
                row.get::<_, Option<String>>(3)?, // session.directory
            ))
        })
        .context("query opencode messages")?;

    for row in rows {
        let (data_json, session_id, time_created, directory) = match row {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error=%e, "opencode: row read failed; skipping");
                continue;
            }
        };
        let Ok(msg): Result<OpenCodeMessageData, _> = from_str::<OpenCodeMessageData>(&data_json)
        else {
            continue;
        };

        // Filter: assistant role, non-empty model, and at least one non-zero
        // token bucket (failed calls record 0/0 and would look like free usage).
        if msg.role != "assistant" || msg.model_id.is_empty() {
            continue;
        }
        if msg.tokens.input == 0 && msg.tokens.output == 0 {
            continue;
        }

        let ts = if msg.time.created > 0 {
            OpenCodeCollector::millis_to_ts(msg.time.created)
        } else {
            OpenCodeCollector::millis_to_ts(time_created)
        };
        let project = directory.clone().unwrap_or_default();

        records.push(UsageRecord {
            source: Source::OpenCode,
            session_id: session_id.clone(),
            model: msg.model_id,
            input_tokens: msg.tokens.input,
            output_tokens: msg.tokens.output,
            cache_creation_input_tokens: msg.tokens.cache.write,
            cache_read_input_tokens: msg.tokens.cache.read,
            reasoning_output_tokens: msg.tokens.reasoning,
            cost_usd: 0.0,
            timestamp: ts,
            project: project.clone(),
            git_branch: String::new(),
        });

        max_watermark = max_watermark.max(time_created);

        sessions.entry(session_id.clone()).or_insert(SessionRecord {
            source: Source::OpenCode,
            session_id: session_id.clone(),
            project: project.clone(),
            cwd: project,
            version: String::new(),
            git_branch: String::new(),
            start_time: ts,
            prompts: 0,
        });
    }
    drop(stmt);

    // Prompt events: user messages. We collect all of them (not just
    // past-watermark) so session prompt tallies stay correct even when a new
    // assistant row lands with the same session as an old user prompt; the
    // DB's dedup index keeps this idempotent.
    let mut prompts: Vec<PromptEvent> = Vec::new();
    if !sessions.is_empty() {
        let mut pstmt = src
            .prepare(
                "SELECT session_id, time_created FROM message \
                 WHERE data LIKE '%\"role\":\"user\"%' \
                 ORDER BY time_created",
            )
            .context("prepare opencode user-prompt select")?;
        let prows = pstmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .context("query opencode user prompts")?;
        for row in prows {
            let (sid, time_created) = match row {
                Ok(r) => r,
                Err(_) => continue,
            };
            if let Some(s) = sessions.get_mut(&sid) {
                s.prompts += 1;
            }
            prompts.push(PromptEvent {
                source: Source::OpenCode,
                session_id: sid,
                timestamp: OpenCodeCollector::millis_to_ts(time_created),
            });
        }
    }
    drop(src);

    let records_inserted = db.insert_usage_batch(&records)?;
    let prompts_inserted = db.insert_prompt_batch(&prompts)?;

    let mut sessions_touched = 0;
    for session in sessions.values() {
        db.upsert_session(session)?;
        sessions_touched += 1;
    }

    // Watermark only moves forward.
    if max_watermark > last_watermark {
        db.set_file_state(path, max_watermark, max_watermark, None)?;
    }

    Ok(FileStats {
        records_inserted,
        prompts_inserted,
        sessions_touched,
    })
}

#[cfg(test)]
#[path = "opencode_tests.rs"]
mod tests;
