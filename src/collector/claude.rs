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

use crate::collector::Collector;
use crate::collector::Reporter;
use crate::collector::ScanProgress;
use crate::collector::ScanSummary;
use crate::domain::Source;
use crate::storage::Db;

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
                        summary
                            .errors
                            .push(format!("{}: {}", path.display(), e));
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

/// Per-file counters returned by [`process_file`]. Filled in by M3 C3b.
#[derive(Default)]
struct FileStats {
    records_inserted: usize,
    prompts_inserted: usize,
    sessions_touched: usize,
}

/// Process a single Claude JSONL file: read the tail, parse, insert.
///
/// M3 C3a only wires the `file_state` checkpoint so repeated scans don't
/// re-read the same bytes. The actual message parsing + inserts arrive in
/// M3 C3b and will flesh out `FileStats`.
async fn process_file(db: &Db, path: &Path) -> Result<FileStats> {
    let (prev_size, prev_offset, prev_ctx) = db.get_file_state(path)?;
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let current_size = metadata.len() as i64;

    // File shrank or was replaced: restart from 0 (Claude doesn't truncate
    // sessions in practice, but be defensive).
    let resume_offset = if current_size < prev_size {
        0
    } else {
        prev_offset
    };

    if current_size == resume_offset {
        // Nothing new; keep the checkpoint as-is.
        return Ok(FileStats::default());
    }

    // M3 C3b plugs parsing + DB writes in here. For now we only advance the
    // offset so a second scan doesn't re-examine the same bytes — this keeps
    // the skeleton end-to-end correct even before messages turn into DB rows.
    db.set_file_state(path, current_size, current_size, prev_ctx.as_ref())?;

    Ok(FileStats::default())
}

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
