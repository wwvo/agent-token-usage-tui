//! Codex CLI session collector.
//!
//! # Source format
//!
//! Codex stores one JSONL file per session under `~/.codex/sessions/**/*.jsonl`.
//! Each line is a wrapped entry:
//!
//! ```jsonc
//! { "timestamp": "2026-04-19T10:00:00.000Z",
//!   "type":      "session_meta" | "turn_context" | "response_item" | "event_msg",
//!   "payload":   { /* shape depends on `type` */ } }
//! ```
//!
//! # Extraction rules (1:1 with agent-usage)
//!
//! * `session_meta.payload.id` / `.cwd` / `.cli_version` seed the session.
//! * `turn_context.payload.model` updates the current model (sticks until the
//!   next `turn_context`).
//! * `response_item.payload.role == "user"` and `.type != "function_call_output"`
//!   counts as a real user prompt.
//! * `event_msg.payload.type == "token_count"` carries the usage numbers:
//!   `info.last_token_usage.{input,cached_input,output,reasoning_output}_tokens`.
//!
//! # Non-overlapping token correction
//!
//! Upstream Codex emits `input_tokens` **including** `cached_input_tokens`.
//! Our storage schema uses the non-overlapping breakdown, so we normalize:
//!
//! ```text
//! stored_input        = raw_input - cached_input
//! stored_cache_read   = cached_input
//! stored_output       = raw_output
//! stored_reasoning    = raw_reasoning_output
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

/// Collector for `~/.codex/sessions/**/*.jsonl`.
pub struct CodexCollector {
    base_paths: Vec<PathBuf>,
}

impl CodexCollector {
    /// Collect from the given list of base directories.
    #[must_use]
    pub fn new(base_paths: Vec<PathBuf>) -> Self {
        Self { base_paths }
    }

    /// Use the default `~/.codex/sessions` path.
    #[must_use]
    pub fn with_default_paths() -> Self {
        let mut paths = Vec::new();
        if let Some(home) = home_dir() {
            paths.push(home.join(".codex").join("sessions"));
        }
        Self::new(paths)
    }

    /// Collect every `.jsonl` descendant under the configured bases.
    fn discover_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for base in &self.base_paths {
            if !base.exists() {
                continue;
            }
            if let Err(e) = walk_jsonl(base, &mut out) {
                tracing::warn!(
                    base = %base.display(),
                    error = %e,
                    "failed to walk Codex sessions directory; skipping"
                );
            }
        }
        out
    }
}

impl Collector for CodexCollector {
    fn source(&self) -> Source {
        Source::Codex
    }

    fn scan(
        &self,
        db: &Db,
        reporter: &dyn Reporter,
    ) -> impl std::future::Future<Output = Result<ScanSummary>> + Send {
        let files = self.discover_files();
        let files_total = files.len();
        let source = Source::Codex;

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

                match process_file(db, &path).await {
                    Ok(stats) => {
                        summary.records_inserted += stats.records_inserted;
                        summary.prompts_inserted += stats.prompts_inserted;
                        summary.sessions_touched += stats.sessions_touched;
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "codex: failed to process file; continuing"
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

fn walk_jsonl(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_jsonl(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[derive(Default)]
struct FileStats {
    records_inserted: usize,
    prompts_inserted: usize,
    sessions_touched: usize,
}

/// Running parser state; mirrors `FileScanContext` so it survives offset resumes.
#[derive(Default)]
struct CodexState {
    session_id: String,
    cwd: String,
    version: String,
    model: String,
    first_ts: Option<DateTime<Utc>>,
}

impl CodexState {
    fn from_context(ctx: Option<&FileScanContext>) -> Self {
        ctx.map(|c| Self {
            session_id: c.session_id.clone(),
            cwd: c.cwd.clone(),
            version: c.version.clone(),
            model: c.model.clone(),
            first_ts: None,
        })
        .unwrap_or_default()
    }

    fn note_ts(&mut self, ts: DateTime<Utc>) {
        self.first_ts = Some(self.first_ts.map_or(ts, |existing| existing.min(ts)));
    }
}

async fn process_file(db: &Db, path: &Path) -> Result<FileStats> {
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

    // Resume with whatever parser context we had; on a fresh scan this stays empty.
    let mut state = if resume_offset > 0 {
        CodexState::from_context(prev_ctx.as_ref())
    } else {
        CodexState::default()
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
                    "codex: skipping malformed JSONL line"
                );
                continue;
            }
        };
        parse_entry(&value, &mut records, &mut prompts, &mut state);
    }

    // Fallback session id: file stem.
    let fallback_session_id = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_owned();
    if state.session_id.is_empty() {
        state.session_id = fallback_session_id;
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
            source: Source::Codex,
            session_id: state.session_id.clone(),
            project: state.cwd.clone(),
            cwd: state.cwd.clone(),
            version: state.version.clone(),
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
            version: state.version,
            model: state.model,
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
    records: &mut Vec<UsageRecord>,
    prompts: &mut Vec<PromptEvent>,
    state: &mut CodexState,
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
    let Some(payload) = entry.get("payload") else {
        return;
    };

    match kind {
        "session_meta" => {
            if let Some(id) = payload.get("id").and_then(Value::as_str) {
                if !id.is_empty() {
                    state.session_id = id.to_owned();
                }
            }
            if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
                if !cwd.is_empty() {
                    state.cwd = cwd.to_owned();
                }
            }
            if let Some(v) = payload.get("cli_version").and_then(Value::as_str) {
                if !v.is_empty() {
                    state.version = v.to_owned();
                }
            }
        }
        "turn_context" => {
            if let Some(model) = payload.get("model").and_then(Value::as_str) {
                if !model.is_empty() {
                    state.model = model.to_owned();
                }
            }
        }
        "response_item" => {
            // role=user + type != "function_call_output" ⇒ real prompt
            let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
            let item_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
            if role == "user" && item_type != "function_call_output" {
                if let Some(ts) = ts {
                    prompts.push(PromptEvent {
                        source: Source::Codex,
                        session_id: String::new(), // patched later
                        timestamp: ts,
                    });
                }
            }
        }
        "event_msg" => {
            let msg_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
            if msg_type != "token_count" {
                return;
            }
            let Some(info) = payload.get("info") else {
                return;
            };
            let Some(usage) = info.get("last_token_usage") else {
                return;
            };
            let Some(ts) = ts else { return };
            if state.model.is_empty() {
                return; // token_count with no model context we trust; skip
            }

            let get = |key: &str| -> i64 { usage.get(key).and_then(Value::as_i64).unwrap_or(0) };
            let raw_input = get("input_tokens");
            let cached = get("cached_input_tokens");
            let output = get("output_tokens");
            let reasoning = get("reasoning_output_tokens");

            // Non-overlapping correction: upstream input includes cache.
            // Pathological upstream where cached > raw_input would otherwise
            // produce a negative count; clamp to zero defensively.
            let adjusted_input = (raw_input - cached).max(0);

            records.push(UsageRecord {
                source: Source::Codex,
                session_id: String::new(), // patched later
                model: state.model.clone(),
                input_tokens: adjusted_input,
                output_tokens: output,
                cache_creation_input_tokens: 0, // Codex doesn't distinguish creation
                cache_read_input_tokens: cached,
                reasoning_output_tokens: reasoning,
                cost_usd: 0.0,
                timestamp: ts,
                project: state.cwd.clone(),
                git_branch: String::new(),
            });
        }
        _ => { /* unknown entry; ignore */ }
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
