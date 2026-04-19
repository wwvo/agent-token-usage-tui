//! Windsurf collector — ingests JSONL files written by the VSCode exporter.
//!
//! # Source format
//!
//! The companion extension in `tools/windsurf-exporter/` pulls Cascade
//! trajectories out of the Windsurf Language Server's in-process memory
//! (the data never hits disk any other way) and writes one `.jsonl` file
//! per cascade under `~/.atut/windsurf-sessions/`.
//!
//! Two line types, documented in detail in `tools/windsurf-exporter/src/writer.ts`:
//!
//! ```jsonc
//! // first line, written exactly once per cascade
//! { "type": "session_meta",
//!   "cascade_id":   "uuid-…",
//!   "created_time": "2026-04-19T10:00:00Z",
//!   "summary":      "freeform title",
//!   "last_model":   "gpt-5-codex",
//!   "workspace":    "file:///home/alice/code/x" }
//!
//! // zero or more subsequent lines
//! { "type": "turn_usage",
//!   "step_id":             "step-uuid-…",
//!   "timestamp":           "2026-04-19T10:01:00Z",
//!   "model":               "gpt-5-codex",
//!   "input_tokens":        1234,
//!   "output_tokens":       567,
//!   "cached_input_tokens": 42 }
//! ```
//!
//! # Extraction rules
//!
//! * `session_meta.cascade_id` becomes the session id (plus the stored
//!   workspace / first-seen timestamp for the session row).
//! * Each `turn_usage` row becomes one [`UsageRecord`]; `cached_input_tokens`
//!   maps onto `cache_read_input_tokens` and there is no cache-creation
//!   bucket for Windsurf (it doesn't expose one).
//! * Unlike Codex, Windsurf's `input_tokens` is already **non-overlapping**
//!   with cached reads, so no subtraction is needed.
//! * There is no user-prompt stream; `prompt_events` stays empty for this
//!   source — Cascade's "turn" is already the same granularity.
//!
//! # Offset resume
//!
//! We track per-file offsets via [`FileScanContext`] exactly like Codex,
//! so repeated scans only process new lines. A rewritten file (size
//! shrank) resets the offset to zero to re-ingest from scratch.

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
use crate::domain::SessionRecord;
use crate::domain::Source;
use crate::domain::UsageRecord;
use crate::domain::WindsurfSessionRecord;
use crate::storage::Db;
use crate::storage::FileScanContext;

/// Environment variable override for the sessions directory. Mirrors the
/// VSCode exporter's knob (`tools/windsurf-exporter/src/writer.ts`) so the
/// two sides stay in sync when redirected to an isolated sandbox.
const ENV_SESSIONS_DIR: &str = "ATUT_WINDSURF_SESSIONS_DIR";

/// Collector for Windsurf exporter JSONL files.
pub struct WindsurfCollector {
    base_paths: Vec<PathBuf>,
}

impl WindsurfCollector {
    /// Collect from the given list of base directories.
    #[must_use]
    pub fn new(base_paths: Vec<PathBuf>) -> Self {
        Self { base_paths }
    }

    /// Default paths, honoring `ATUT_WINDSURF_SESSIONS_DIR` when set.
    ///
    /// Falls back to `~/.atut/windsurf-sessions/` — the same default the
    /// exporter picks when the env var is unset.
    #[must_use]
    pub fn with_default_paths() -> Self {
        if let Some(dir) = std::env::var_os(ENV_SESSIONS_DIR) {
            return Self::new(vec![PathBuf::from(dir)]);
        }
        let mut paths = Vec::new();
        if let Some(home) = home_dir() {
            paths.push(home.join(".atut").join("windsurf-sessions"));
        }
        Self::new(paths)
    }

    /// Recursively collect every `.jsonl` descendant under the configured bases.
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
                    "failed to walk Windsurf sessions directory; skipping"
                );
            }
        }
        out
    }
}

impl Collector for WindsurfCollector {
    fn source(&self) -> Source {
        Source::Windsurf
    }

    // `async fn` sugar would work, but we match the desugared signature
    // every other collector in this module uses so the pipeline's glue
    // code stays uniform.
    #[allow(clippy::manual_async_fn)]
    fn scan(
        &self,
        db: &Db,
        reporter: &dyn Reporter,
    ) -> impl std::future::Future<Output = Result<ScanSummary>> + Send {
        let files = self.discover_files();
        let files_total = files.len();
        let source = Source::Windsurf;

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
                        summary.sessions_touched += stats.sessions_touched;
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "windsurf: failed to process file; continuing"
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
    sessions_touched: usize,
}

/// Running parser state; survives offset resumes via `FileScanContext`.
///
/// We only need `session_id` / `workspace` / `last_model` to survive a
/// resume — the timestamps are re-derived from the new lines we're about
/// to ingest on this pass.
///
/// `summary` is the Cascade's human-readable title from `session_meta`.
/// We don't round-trip it through `FileScanContext` (no slot for it there
/// today) — this is fine because the `windsurf_sessions` upsert semantics
/// treat empty-string inputs as "keep old value", so a resume that didn't
/// re-read the first line leaves the existing row untouched.
#[derive(Default)]
struct WindsurfState {
    session_id: String,
    workspace: String,
    last_model: String,
    /// Cascade's human-readable title, read off `session_meta.summary`.
    summary: String,
    /// The `created_time` from `session_meta`, parsed once when we see it.
    /// Takes precedence over `first_ts` for the session row's `start_time`.
    created_time: Option<DateTime<Utc>>,
    /// Minimum `timestamp` seen on any `turn_usage` line in this scan.
    /// Used only if `created_time` is absent.
    first_ts: Option<DateTime<Utc>>,
}

impl WindsurfState {
    fn from_context(ctx: Option<&FileScanContext>) -> Self {
        ctx.map(|c| Self {
            session_id: c.session_id.clone(),
            // `cwd` doubles as `workspace` on resume; context doesn't
            // have a dedicated slot and we already store the URI there.
            workspace: c.cwd.clone(),
            last_model: c.model.clone(),
            // `FileScanContext` has no summary slot; we intentionally
            // leave it empty on resume and rely on upsert's "empty
            // preserves old value" semantics to not clobber the row.
            summary: String::new(),
            created_time: None,
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

    // Truncation detection: if the file shrank we re-ingest from scratch.
    let resume_offset = if current_size < prev_size {
        0
    } else {
        prev_offset
    };
    if current_size == resume_offset {
        return Ok(FileStats::default());
    }

    let mut state = if resume_offset > 0 {
        WindsurfState::from_context(prev_ctx.as_ref())
    } else {
        WindsurfState::default()
    };

    let mut records: Vec<UsageRecord> = Vec::new();

    let resume_u64 = u64::try_from(resume_offset)
        .with_context(|| format!("negative offset {resume_offset} for {}", path.display()))?;

    for line_result in util::read_jsonl_from_offset(path, resume_u64)? {
        let value = match line_result {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "windsurf: skipping malformed JSONL line"
                );
                continue;
            }
        };
        parse_entry(&value, &mut records, &mut state);
    }

    // Fallback session id: derive from the file stem when the file has
    // no `session_meta` line yet (e.g. exporter crashed before first flush).
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

    let records_inserted = db.insert_usage_batch(&records)?;

    // Upsert session whenever we have any usage rows or a `session_meta`
    // line (non-empty session id plus at least one state field). Empty
    // files that contained neither leave sessions_touched = 0.
    let has_meta = !state.session_id.is_empty()
        && (state.created_time.is_some()
            || !state.last_model.is_empty()
            || !state.summary.is_empty());
    let sessions_touched = if records.is_empty() && !has_meta {
        0
    } else {
        let start_time = state
            .created_time
            .or(state.first_ts)
            .unwrap_or_else(Utc::now);
        db.upsert_session(&SessionRecord {
            source: Source::Windsurf,
            session_id: state.session_id.clone(),
            project: state.workspace.clone(),
            cwd: state.workspace.clone(),
            version: String::new(),
            git_branch: String::new(),
            start_time,
            // Windsurf's Cascade doesn't emit a discrete prompt event; the
            // user-input step *is* the turn, so counting turns here would
            // double-count against the usage records. We leave prompts at 0.
            prompts: 0,
        })?;

        // Mirror the cascade's Windsurf-specific presentation fields
        // (summary / workspace / last_model / created_time) into the
        // dedicated `windsurf_sessions` table so the per-cascade TUI
        // drill-down view can read them without re-parsing the JSONL.
        // Gated on `has_meta` so an orphan file (turn_usage without a
        // preceding session_meta, e.g. a crashed exporter) doesn't
        // produce a blank row that would look broken in the drill-down
        // view; `last_seen = now` is the upsert's max-wins signal —
        // even when `state.first_ts` is older (because this scan only
        // saw old rows) we still want to record that we *observed* the
        // cascade on this pass.
        if has_meta {
            db.upsert_windsurf_session(&WindsurfSessionRecord {
                cascade_id: state.session_id.clone(),
                summary: state.summary.clone(),
                workspace: state.workspace.clone(),
                last_model: state.last_model.clone(),
                created_time: state.created_time,
                last_seen: Utc::now(),
            })?;
        }
        1
    };

    db.set_file_state(
        path,
        current_size,
        current_size,
        Some(&FileScanContext {
            session_id: state.session_id,
            cwd: state.workspace,
            version: String::new(),
            model: state.last_model,
        }),
    )?;

    Ok(FileStats {
        records_inserted,
        sessions_touched,
    })
}

fn parse_entry(entry: &Value, records: &mut Vec<UsageRecord>, state: &mut WindsurfState) {
    let Some(kind) = entry.get("type").and_then(Value::as_str) else {
        return;
    };

    match kind {
        "session_meta" => {
            if let Some(id) = entry.get("cascade_id").and_then(Value::as_str) {
                if !id.is_empty() {
                    state.session_id = id.to_owned();
                }
            }
            if let Some(summary_ws) = entry.get("workspace").and_then(Value::as_str) {
                if !summary_ws.is_empty() {
                    state.workspace = summary_ws.to_owned();
                }
            }
            if let Some(model) = entry.get("last_model").and_then(Value::as_str) {
                if !model.is_empty() {
                    state.last_model = model.to_owned();
                }
            }
            if let Some(title) = entry.get("summary").and_then(Value::as_str) {
                if !title.is_empty() {
                    state.summary = title.to_owned();
                }
            }
            if let Some(ct) = entry.get("created_time").and_then(Value::as_str) {
                if let Ok(parsed) = DateTime::parse_from_rfc3339(ct) {
                    state.created_time = Some(parsed.with_timezone(&Utc));
                }
            }
        }
        "turn_usage" => {
            // Skip rows without a usable timestamp: Cascade without
            // `timestamp` means we can't plot them in the Trend view and
            // they'd show up as 1970-01-01 everywhere; drop instead.
            let Some(ts_str) = entry.get("timestamp").and_then(Value::as_str) else {
                return;
            };
            let Ok(ts_fixed) = DateTime::parse_from_rfc3339(ts_str) else {
                return;
            };
            let ts = ts_fixed.with_timezone(&Utc);
            state.note_ts(ts);

            let model = entry
                .get("model")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map_or_else(|| state.last_model.clone(), str::to_owned);

            let get = |key: &str| -> i64 { entry.get(key).and_then(Value::as_i64).unwrap_or(0) };
            let input = get("input_tokens");
            let output = get("output_tokens");
            let cached = get("cached_input_tokens");

            records.push(UsageRecord {
                source: Source::Windsurf,
                session_id: String::new(), // patched later from state.session_id
                model,
                input_tokens: input,
                output_tokens: output,
                // Windsurf's exporter doesn't distinguish creation vs read,
                // so everything cache-related lands in `cache_read`.
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: cached,
                reasoning_output_tokens: 0,
                cost_usd: 0.0,
                timestamp: ts,
                project: state.workspace.clone(),
                git_branch: String::new(),
            });
        }
        _ => { /* unknown entry; ignore */ }
    }
}

#[cfg(test)]
#[path = "windsurf_tests.rs"]
mod tests;
