//! Claude Code session collector.
//!
//! # Source format
//!
//! Claude writes one JSONL file per session under
//! `~/.claude/projects/<projectHash>/<sessionId>.jsonl`. Each line is an
//! envelope of the form:
//!
//! ```jsonc
//! { "type": "user" | "assistant",
//!   "uuid": "...",
//!   "timestamp": "2026-04-19T12:34:56.789Z",
//!   "sessionId": "…",
//!   "cwd": "/path",
//!   "version": "1.0.3",
//!   "gitBranch": "main",
//!   "message": { /* role / content / model / usage / ... */ } }
//! ```
//!
//! # Extraction rules
//!
//! * Real user prompts (`type: "user"` and `message.content` is not a
//!   `tool_result` block) become [`PromptEvent`]s and bump `prompts`.
//! * Assistant turns carrying `message.usage` with a real model name become
//!   [`UsageRecord`]s. Streaming chunks without a `usage` field and entries
//!   whose `message.model` is the synthetic `"<synthetic>"` marker are
//!   skipped.
//! * The envelope's top-level `sessionId` / `cwd` / `version` / `gitBranch`
//!   populate the [`SessionRecord`]; when absent, the file stem doubles as
//!   the session id so orphaned files still aggregate.
//!
//! # Incremental scans
//!
//! `Db::get_file_state` / `set_file_state` carry a byte offset so repeat
//! scans only process the tail. The [`FileScanContext`] alongside the offset
//! remembers the envelope fields we've already resolved, which Claude files
//! don't strictly need (fields repeat on every line), but keeps the interface
//! uniform with Codex which does.

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

/// Collector for `~/.claude/projects/**/*.jsonl`.
pub struct ClaudeCollector {
    base_paths: Vec<PathBuf>,
}

impl ClaudeCollector {
    /// Collect from the given list of base directories.
    ///
    /// Each base is scanned recursively for `.jsonl` files. Non-existent
    /// directories are skipped silently (they're a normal state when a user
    /// only uses one of the two supported layouts).
    #[must_use]
    pub fn new(base_paths: Vec<PathBuf>) -> Self {
        Self { base_paths }
    }

    /// Use default paths: `~/.claude/projects` and `~/.config/claude/projects`.
    ///
    /// Both are checked because Linux users occasionally configure Claude to
    /// honor the XDG Base Directory spec via symlink or explicit config.
    #[must_use]
    pub fn with_default_paths() -> Self {
        let mut paths = Vec::new();
        if let Some(home) = home_dir() {
            paths.push(home.join(".claude").join("projects"));
            paths.push(home.join(".config").join("claude").join("projects"));
        }
        Self::new(paths)
    }

    /// Collect every `.jsonl` descendant under the configured base paths.
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
                    "failed to walk Claude session directory; skipping"
                );
            }
        }
        out
    }
}

impl Collector for ClaudeCollector {
    fn source(&self) -> Source {
        Source::Claude
    }

    fn scan(
        &self,
        db: &Db,
        reporter: &dyn Reporter,
    ) -> impl std::future::Future<Output = Result<ScanSummary>> + Send {
        // Materialize the file list up front so we can report progress with an
        // accurate total before we start hitting disk.
        let files = self.discover_files();
        let files_total = files.len();
        let source = Source::Claude;

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
                    Ok(file_stats) => {
                        summary.records_inserted += file_stats.records_inserted;
                        summary.prompts_inserted += file_stats.prompts_inserted;
                        summary.sessions_touched += file_stats.sessions_touched;
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "claude: failed to process file; continuing"
                        );
                        summary.errors.push(format!("{}: {}", path.display(), e));
                    }
                }
            }

            // Final progress tick so UI can show "done".
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

// ---- Helpers ---------------------------------------------------------------

/// Walk `dir` recursively, pushing every `.jsonl` into `out`.
///
/// Symlink loops are not handled (Claude's tree doesn't have them in practice);
/// hidden files are included so users who tweak paths don't lose data.
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

/// Cross-platform best-effort home directory lookup.
///
/// Avoids the `dirs` crate dependency for a one-call use-site.
fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Per-file counters returned by [`process_file`].
#[derive(Default)]
struct FileStats {
    records_inserted: usize,
    prompts_inserted: usize,
    sessions_touched: usize,
}

/// Accumulated session-level fields while we walk a file.
///
/// Claude repeats session metadata on every line, so we just keep the latest
/// non-empty values we've seen.
#[derive(Default)]
struct SessionMeta {
    session_id: Option<String>,
    cwd: Option<String>,
    version: Option<String>,
    git_branch: Option<String>,
    first_ts: Option<DateTime<Utc>>,
}

impl SessionMeta {
    fn observe_envelope(&mut self, env: &Value, ts: Option<DateTime<Utc>>) {
        if let Some(s) = env.get("sessionId").and_then(Value::as_str) {
            if !s.is_empty() {
                self.session_id = Some(s.to_owned());
            }
        }
        if let Some(s) = env.get("cwd").and_then(Value::as_str) {
            if !s.is_empty() {
                self.cwd = Some(s.to_owned());
            }
        }
        if let Some(s) = env.get("version").and_then(Value::as_str) {
            if !s.is_empty() {
                self.version = Some(s.to_owned());
            }
        }
        if let Some(s) = env.get("gitBranch").and_then(Value::as_str) {
            if !s.is_empty() {
                self.git_branch = Some(s.to_owned());
            }
        }
        if let Some(t) = ts {
            self.first_ts = Some(self.first_ts.map_or(t, |existing| existing.min(t)));
        }
    }
}

/// Process a single Claude JSONL file: read the tail, parse, insert, upsert.
async fn process_file(db: &Db, path: &Path) -> Result<FileStats> {
    let (prev_size, prev_offset, _prev_ctx) = db.get_file_state(path)?;
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let current_size = i64::try_from(metadata.len())
        .with_context(|| format!("file too large for i64 offset: {}", path.display()))?;

    // File shrank or was replaced: restart from 0 (Claude doesn't truncate
    // sessions in practice, but be defensive).
    let resume_offset = if current_size < prev_size {
        0
    } else {
        prev_offset
    };

    if current_size == resume_offset {
        return Ok(FileStats::default());
    }

    // Derive project from the parent directory name and a session fallback
    // from the file stem. Both are used only when the envelope doesn't carry
    // explicit values.
    let project = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_owned();
    let fallback_session_id = path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_owned();

    let mut records: Vec<UsageRecord> = Vec::new();
    let mut prompts: Vec<PromptEvent> = Vec::new();
    let mut meta = SessionMeta::default();

    let resume_u64 = u64::try_from(resume_offset)
        .with_context(|| format!("negative offset {resume_offset} for {}", path.display()))?;

    for line_result in util::read_jsonl_from_offset(path, resume_u64)? {
        let value = match line_result {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "claude: skipping malformed JSONL line"
                );
                continue;
            }
        };
        parse_envelope(&value, &mut records, &mut prompts, &mut meta);
    }

    // Resolve session id used by both the SessionRecord and the FileScanContext.
    let session_id = meta
        .session_id
        .clone()
        .unwrap_or_else(|| fallback_session_id.clone());

    // Patch in the resolved session id for any records / prompts that couldn't
    // read it from their own envelope.
    for r in &mut records {
        if r.session_id.is_empty() {
            r.session_id.clone_from(&session_id);
        }
        if r.project.is_empty() {
            r.project.clone_from(&project);
        }
    }
    for p in &mut prompts {
        if p.session_id.is_empty() {
            p.session_id.clone_from(&session_id);
        }
    }

    let records_inserted = db.insert_usage_batch(&records)?;
    let prompts_inserted = db.insert_prompt_batch(&prompts)?;

    let sessions_touched = if records.is_empty() && prompts.is_empty() {
        0
    } else {
        let session_record = SessionRecord {
            source: Source::Claude,
            session_id: session_id.clone(),
            project: project.clone(),
            cwd: meta.cwd.clone().unwrap_or_default(),
            version: meta.version.clone().unwrap_or_default(),
            git_branch: meta.git_branch.clone().unwrap_or_default(),
            start_time: meta.first_ts.unwrap_or_else(Utc::now),
            prompts: i64::try_from(prompts.len()).unwrap_or(i64::MAX),
        };
        db.upsert_session(&session_record)?;
        1
    };

    let new_ctx = FileScanContext {
        session_id,
        cwd: meta.cwd.unwrap_or_default(),
        version: meta.version.unwrap_or_default(),
        model: String::new(), // Claude models are attached per-turn, not per-session.
    };
    db.set_file_state(path, current_size, current_size, Some(&new_ctx))?;

    Ok(FileStats {
        records_inserted,
        prompts_inserted,
        sessions_touched,
    })
}

/// Parse one envelope line, pushing zero or one record/prompt into the
/// accumulators and keeping session metadata current.
fn parse_envelope(
    env: &Value,
    records: &mut Vec<UsageRecord>,
    prompts: &mut Vec<PromptEvent>,
    meta: &mut SessionMeta,
) {
    let ts = env
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc));

    meta.observe_envelope(env, ts);

    let Some(kind) = env.get("type").and_then(Value::as_str) else {
        return;
    };
    let Some(message) = env.get("message") else {
        return;
    };

    match kind {
        "user" if util::is_real_user_prompt(message) => {
            let Some(ts) = ts else { return };
            prompts.push(PromptEvent {
                source: Source::Claude,
                session_id: String::new(), // filled later from meta/fallback
                timestamp: ts,
            });
        }
        "assistant" => {
            let Some(model) = message.get("model").and_then(Value::as_str) else {
                return;
            };
            if model == "<synthetic>" {
                return;
            }
            let Some(usage) = message.get("usage") else {
                return; // streaming chunk w/o usage — skip
            };
            let Some(ts) = ts else { return };

            let get_i64 =
                |key: &str| -> i64 { usage.get(key).and_then(Value::as_i64).unwrap_or(0) };

            records.push(UsageRecord {
                source: Source::Claude,
                session_id: String::new(), // filled later
                model: model.to_owned(),
                input_tokens: get_i64("input_tokens"),
                output_tokens: get_i64("output_tokens"),
                cache_creation_input_tokens: get_i64("cache_creation_input_tokens"),
                cache_read_input_tokens: get_i64("cache_read_input_tokens"),
                reasoning_output_tokens: 0, // Claude has no separate reasoning channel
                cost_usd: 0.0,              // filled by recalc_costs
                timestamp: ts,
                project: String::new(), // filled later
                git_branch: env
                    .get("gitBranch")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        _ => { /* unknown envelope type; ignore silently */ }
    }
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
