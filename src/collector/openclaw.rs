//! OpenClaw session collector.
//!
//! # Source format
//!
//! OpenClaw stores sessions under a two-level directory tree:
//!
//! ```text
//! <base>/
//!   <agent-id>/
//!     sessions/
//!       <session-id>.jsonl
//! ```
//!
//! The immediate child directory of `<base>` is the **agent id** (e.g. a
//! project slug) and feeds the `project` column on both `usage_records` and
//! `sessions`. Everything lower is recursively scanned for `*.jsonl`.
//!
//! # Entry schema
//!
//! Each JSONL line is an envelope:
//!
//! ```jsonc
//! { "type": "session" | "message",
//!   "id": "...", "parentId": "...", "timestamp": "...",
//!   "cwd": "...", "message": { /* only for type="message" */ } }
//! ```
//!
//! Inside a `type=message`, the nested `message` object carries role, model,
//! provider and a usage block:
//!
//! ```jsonc
//! { "role": "user" | "assistant",
//!   "content": ..., "model": "...", "provider": "...",
//!   "usage": { "input": N, "output": N, "cacheRead": N, "cacheWrite": N } }
//! ```
//!
//! # Extraction rules
//!
//! * `type=session`: seed `session_id = id`, `cwd = cwd`.
//! * `type=message`, `role=user`: real prompt **iff** the content has no
//!   `tool_result` block (shares `util::is_real_user_prompt` with Claude).
//! * `type=message`, `role=assistant`, with `usage` non-null: usage record —
//!   but filter out the internal `model = "delivery-mirror"` rows (OpenClaw
//!   emits these as a transport-layer echo and double-counts them).
//!
//! # Token semantics (already non-overlapping upstream)
//!
//! Unlike Codex, OpenClaw's `input` **does not** include `cacheRead` or
//! `cacheWrite`. We store:
//!
//! ```text
//! input_tokens                 = usage.input
//! output_tokens                = usage.output
//! cache_read_input_tokens      = usage.cacheRead
//! cache_creation_input_tokens  = usage.cacheWrite
//! ```

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;
use serde_json::Value;

use crate::collector::Collector;
use crate::collector::Reporter;
use crate::collector::ScanProgress;
use crate::collector::ScanSummary;
use crate::collector::util;
use crate::domain::PromptEvent;
use crate::domain::SessionRecord;
use crate::domain::Source;
use crate::domain::UsageRecord;
use crate::storage::Db;
use crate::storage::FileScanContext;

/// Rows with this model are OpenClaw's internal transport echoes and must not
/// be double-counted against real agents.
const DELIVERY_MIRROR_MODEL: &str = "delivery-mirror";

/// Collector for `<base>/<agent-id>/sessions/**/*.jsonl`.
pub struct OpenClawCollector {
    base_paths: Vec<PathBuf>,
}

impl OpenClawCollector {
    /// Build a collector that scans every file under every configured base.
    #[must_use]
    pub fn new(base_paths: Vec<PathBuf>) -> Self {
        Self { base_paths }
    }

    /// Enumerate `(path, agent_id)` pairs for every `.jsonl` descendant.
    ///
    /// The `agent_id` is the first subdirectory under the base — the same one
    /// that feeds the `project` column downstream.
    fn discover_files(&self) -> Vec<(PathBuf, String)> {
        let mut out = Vec::new();
        for base in &self.base_paths {
            if !base.exists() {
                continue;
            }
            let entries = match fs::read_dir(base) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        base = %base.display(),
                        error = %e,
                        "openclaw: cannot read base directory; skipping",
                    );
                    continue;
                }
            };
            for agent in entries.flatten() {
                if !agent.path().is_dir() {
                    continue;
                }
                let Some(agent_id) = agent.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let sessions = agent.path().join("sessions");
                if !sessions.exists() {
                    continue;
                }
                if let Err(e) = walk_jsonl(&sessions, &agent_id, &mut out) {
                    tracing::warn!(
                        dir = %sessions.display(),
                        agent = %agent_id,
                        error = %e,
                        "openclaw: failed to walk sessions; skipping",
                    );
                }
            }
        }
        out
    }
}

impl Collector for OpenClawCollector {
    fn source(&self) -> Source {
        Source::OpenClaw
    }

    fn scan(
        &self,
        db: &Db,
        reporter: &dyn Reporter,
    ) -> impl std::future::Future<Output = Result<ScanSummary>> + Send {
        let files = self.discover_files();
        let files_total = files.len();
        let source = Source::OpenClaw;

        async move {
            let mut summary = ScanSummary::new(source);
            summary.files_scanned = files_total;

            for (idx, (path, agent_id)) in files.into_iter().enumerate() {
                reporter.on_progress(ScanProgress {
                    source,
                    files_done: idx,
                    files_total,
                    current_file: Some(path.clone()),
                });

                match process_file(db, &path, &agent_id).await {
                    Ok(stats) => {
                        summary.records_inserted += stats.records_inserted;
                        summary.prompts_inserted += stats.prompts_inserted;
                        summary.sessions_touched += stats.sessions_touched;
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            agent = %agent_id,
                            error = %e,
                            "openclaw: failed to process file; continuing",
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

// ---- Helpers --------------------------------------------------------------

fn walk_jsonl(dir: &Path, agent_id: &str, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_jsonl(&path, agent_id, out)?;
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push((path, agent_id.to_owned()));
        }
    }
    Ok(())
}

#[derive(Default)]
struct FileStats {
    records_inserted: usize,
    prompts_inserted: usize,
    sessions_touched: usize,
}

/// Running parser state; mirrors `FileScanContext` so it survives resumes.
#[derive(Default)]
struct OpenClawState {
    session_id: String,
    cwd: String,
    first_ts: Option<DateTime<Utc>>,
}

impl OpenClawState {
    fn from_context(ctx: Option<&FileScanContext>) -> Self {
        ctx.map(|c| Self {
            session_id: c.session_id.clone(),
            cwd: c.cwd.clone(),
            first_ts: None,
        })
        .unwrap_or_default()
    }

    fn note_ts(&mut self, ts: DateTime<Utc>) {
        self.first_ts = Some(self.first_ts.map_or(ts, |existing| existing.min(ts)));
    }
}

async fn process_file(db: &Db, path: &Path, agent_id: &str) -> Result<FileStats> {
    let (prev_size, prev_offset, prev_ctx) = db.get_file_state(path)?;
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let current_size = i64::try_from(metadata.len())
        .with_context(|| format!("file too large for i64 offset: {}", path.display()))?;

    let resume_offset = if current_size < prev_size {
        0
    } else {
        prev_offset
    };
    if current_size == resume_offset {
        return Ok(FileStats::default());
    }

    let mut state = if resume_offset > 0 {
        OpenClawState::from_context(prev_ctx.as_ref())
    } else {
        OpenClawState::default()
    };

    let mut records: Vec<UsageRecord> = Vec::new();
    let mut prompts: Vec<PromptEvent> = Vec::new();

    let resume_u64 = u64::try_from(resume_offset)
        .with_context(|| format!("negative offset {resume_offset} for {}", path.display()))?;

    for line_result in util::read_jsonl_from_offset(path, resume_u64)? {
        let value = match line_result {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "openclaw: skipping malformed JSONL line",
                );
                continue;
            }
        };
        parse_entry(&value, agent_id, &mut records, &mut prompts, &mut state);
    }

    // Fallback session id: file stem.
    if state.session_id.is_empty() {
        state.session_id = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_owned();
    }

    for r in &mut records {
        if r.session_id.is_empty() {
            r.session_id.clone_from(&state.session_id);
        }
    }
    for p in &mut prompts {
        if p.session_id.is_empty() {
            p.session_id.clone_from(&state.session_id);
        }
    }

    let records_inserted = db.insert_usage_batch(&records)?;
    let prompts_inserted = db.insert_prompt_batch(&prompts)?;

    let sessions_touched = if records.is_empty() && prompts.is_empty() {
        0
    } else {
        db.upsert_session(&SessionRecord {
            source: Source::OpenClaw,
            session_id: state.session_id.clone(),
            project: agent_id.to_owned(),
            cwd: state.cwd.clone(),
            version: String::new(),
            git_branch: String::new(),
            start_time: state.first_ts.unwrap_or_else(Utc::now),
            prompts: i64::try_from(prompts.len()).unwrap_or(i64::MAX),
        })?;
        1
    };

    db.set_file_state(
        path,
        current_size,
        current_size,
        Some(&FileScanContext {
            session_id: state.session_id,
            cwd: state.cwd,
            version: String::new(),
            model: String::new(),
        }),
    )?;

    Ok(FileStats {
        records_inserted,
        prompts_inserted,
        sessions_touched,
    })
}

fn parse_entry(
    entry: &Value,
    agent_id: &str,
    records: &mut Vec<UsageRecord>,
    prompts: &mut Vec<PromptEvent>,
    state: &mut OpenClawState,
) {
    let ts = entry
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc));
    if let Some(t) = ts {
        state.note_ts(t);
    }

    let Some(kind) = entry.get("type").and_then(Value::as_str) else {
        return;
    };

    match kind {
        "session" => {
            if let Some(id) = entry.get("id").and_then(Value::as_str) {
                if !id.is_empty() {
                    state.session_id = id.to_owned();
                }
            }
            if let Some(cwd) = entry.get("cwd").and_then(Value::as_str) {
                if !cwd.is_empty() {
                    state.cwd = cwd.to_owned();
                }
            }
        }
        "message" => {
            let Some(message) = entry.get("message") else {
                return;
            };
            let role = message.get("role").and_then(Value::as_str).unwrap_or("");

            match role {
                "user" if util::is_real_user_prompt(message) => {
                    if let Some(ts) = ts {
                        prompts.push(PromptEvent {
                            source: Source::OpenClaw,
                            session_id: String::new(),
                            timestamp: ts,
                        });
                    }
                }
                "assistant" => {
                    let Some(usage) = message.get("usage") else {
                        return;
                    };
                    let model = message
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    if model == DELIVERY_MIRROR_MODEL {
                        return;
                    }
                    let Some(ts) = ts else { return };

                    let get =
                        |key: &str| -> i64 { usage.get(key).and_then(Value::as_i64).unwrap_or(0) };

                    records.push(UsageRecord {
                        source: Source::OpenClaw,
                        session_id: String::new(),
                        model,
                        input_tokens: get("input"),
                        output_tokens: get("output"),
                        cache_creation_input_tokens: get("cacheWrite"),
                        cache_read_input_tokens: get("cacheRead"),
                        reasoning_output_tokens: 0,
                        cost_usd: 0.0,
                        timestamp: ts,
                        project: agent_id.to_owned(),
                        git_branch: String::new(),
                    });
                }
                _ => { /* unknown role */ }
            }
        }
        _ => { /* unknown entry type */ }
    }
}

#[cfg(test)]
#[path = "openclaw_tests.rs"]
mod tests;
