//! Sidecar tests for the Windsurf JSONL collector.
//!
//! Fixtures are created inline in `tempdir()` rather than committed under
//! `tests/fixtures/` because (a) the wire format is tiny and the fixture
//! *is* the test assertion, and (b) we need to be able to mutate + re-read
//! the same file across sub-tests to exercise the resume + idempotency
//! paths end-to-end.

use std::fs;
use std::io::Write;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::WindsurfCollector;
use crate::collector::Collector;
use crate::collector::NoopReporter;
use crate::domain::Source;
use crate::storage::Db;

// ---- Fixture builders -----------------------------------------------------

/// Append a single JSON object line (plus trailing `\n`) to `path`.
///
/// Mirrors how `tools/windsurf-exporter/src/writer.ts::writeSession`
/// appends each line; keeping the test-side writer identical avoids
/// drift between what we assert and what the real exporter will produce.
fn append_jsonl(path: &std::path::Path, line: &serde_json::Value) {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open fixture file");
    writeln!(f, "{line}").expect("write fixture line");
}

fn session_meta(
    cascade_id: &str,
    created_time: &str,
    summary: &str,
    last_model: &str,
    workspace: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "session_meta",
        "cascade_id": cascade_id,
        "created_time": created_time,
        "summary": summary,
        "last_model": last_model,
        "workspace": workspace,
    })
}

fn turn_usage(
    step_id: &str,
    timestamp: &str,
    model: &str,
    input: i64,
    output: i64,
    cached: i64,
) -> serde_json::Value {
    serde_json::json!({
        "type": "turn_usage",
        "step_id": step_id,
        "timestamp": timestamp,
        "model": model,
        "input_tokens": input,
        "output_tokens": output,
        "cached_input_tokens": cached,
    })
}

// ---- Basic sanity ---------------------------------------------------------

#[test]
fn source_is_windsurf() {
    assert_eq!(
        WindsurfCollector::new(Vec::new()).source(),
        Source::Windsurf
    );
}

#[tokio::test]
async fn scan_empty_dir_returns_empty_summary() {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("t.db")).expect("open");

    // No .jsonl files under the base → files_scanned = 0.
    let c = WindsurfCollector::new(vec![tmp.path().to_path_buf()]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(s.source, Source::Windsurf);
    assert_eq!(s.files_scanned, 0);
    assert_eq!(s.records_inserted, 0);
    assert_eq!(s.prompts_inserted, 0);
    assert_eq!(s.sessions_touched, 0);
    assert!(s.errors.is_empty());
}

#[tokio::test]
async fn scan_nonexistent_base_dir_is_silent_noop() {
    // A missing base directory (user hasn't installed the extension yet)
    // must not error — the TUI scan should run cleanly out of the box.
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("t.db")).expect("open");

    let missing = tmp.path().join("nope");
    let c = WindsurfCollector::new(vec![missing]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(s.files_scanned, 0);
    assert!(s.errors.is_empty());
}

// ---- Two cascades, fresh scan --------------------------------------------

#[tokio::test]
async fn scan_two_cascades_produces_two_sessions_and_n_usage_rows() {
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");

    let a = sessions_dir.join("cascade-a.jsonl");
    append_jsonl(
        &a,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "hello world",
            "sonnet-4-6",
            "file:///home/alice/code/a",
        ),
    );
    append_jsonl(
        &a,
        &turn_usage(
            "step-a1",
            "2026-04-19T10:01:00Z",
            "sonnet-4-6",
            1000,
            200,
            50,
        ),
    );
    append_jsonl(
        &a,
        &turn_usage(
            "step-a2",
            "2026-04-19T10:02:00Z",
            "sonnet-4-6",
            1500,
            300,
            75,
        ),
    );

    let b = sessions_dir.join("cascade-b.jsonl");
    append_jsonl(
        &b,
        &session_meta(
            "cascade-b",
            "2026-04-19T11:00:00Z",
            "other conversation",
            "opus-4-7",
            "file:///home/alice/code/b",
        ),
    );
    append_jsonl(
        &b,
        &turn_usage("step-b1", "2026-04-19T11:01:00Z", "opus-4-7", 500, 100, 25),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(s.files_scanned, 2);
    assert_eq!(s.records_inserted, 3, "2 + 1 turn_usage rows across files");
    assert_eq!(s.sessions_touched, 2);
    assert_eq!(s.prompts_inserted, 0, "Windsurf never emits prompt_events");
    assert!(s.errors.is_empty(), "errors: {:?}", s.errors);

    // DB side: two sessions with distinct session_ids and start_times.
    let sessions = db
        .fetch_recent_sessions(Some(Source::Windsurf), 10)
        .unwrap();
    assert_eq!(sessions.len(), 2);
    // Newest first (cascade-b at 11:00, cascade-a at 10:00).
    assert_eq!(sessions[0].session_id, "cascade-b");
    assert_eq!(sessions[0].records, 1);
    assert_eq!(
        sessions[0].total_tokens,
        500 + 100 + 25,
        "b has one turn: 500+100+25",
    );
    assert_eq!(sessions[1].session_id, "cascade-a");
    assert_eq!(sessions[1].records, 2);
    assert_eq!(
        sessions[1].total_tokens,
        1000 + 200 + 50 + 1500 + 300 + 75,
        "a has two turns accumulated",
    );
}

// ---- Idempotency ---------------------------------------------------------

#[tokio::test]
async fn rescan_without_changes_inserts_zero_new_rows() {
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");

    let file = sessions_dir.join("cascade-a.jsonl");
    append_jsonl(
        &file,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "s",
            "sonnet-4-6",
            "file:///home/alice",
        ),
    );
    append_jsonl(
        &file,
        &turn_usage("step-1", "2026-04-19T10:01:00Z", "sonnet-4-6", 100, 50, 10),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);

    // First pass: 1 session + 1 usage row.
    let first = c.scan(&db, &NoopReporter).await.expect("first scan");
    assert_eq!(first.records_inserted, 1);
    assert_eq!(first.sessions_touched, 1);

    // Second pass over unchanged file: zero new rows, still counts as
    // a "scanned" file (we stat-ed it, saw offset unchanged).
    let second = c.scan(&db, &NoopReporter).await.expect("second scan");
    assert_eq!(second.files_scanned, 1);
    assert_eq!(
        second.records_inserted, 0,
        "offset resume should skip every line"
    );
    assert_eq!(
        second.sessions_touched, 0,
        "no new rows means no session upsert",
    );
}

// ---- Append-only incremental ---------------------------------------------

#[tokio::test]
async fn appended_turns_are_ingested_on_next_scan() {
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");

    let file = sessions_dir.join("cascade-a.jsonl");
    append_jsonl(
        &file,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "s",
            "sonnet-4-6",
            "file:///home/alice",
        ),
    );
    append_jsonl(
        &file,
        &turn_usage("step-1", "2026-04-19T10:01:00Z", "sonnet-4-6", 100, 50, 10),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);

    let first = c.scan(&db, &NoopReporter).await.expect("first scan");
    assert_eq!(first.records_inserted, 1);

    // Simulate the exporter flushing a new turn.
    append_jsonl(
        &file,
        &turn_usage("step-2", "2026-04-19T10:02:00Z", "sonnet-4-6", 200, 80, 20),
    );

    let second = c.scan(&db, &NoopReporter).await.expect("second scan");
    assert_eq!(
        second.records_inserted, 1,
        "only the newly-appended line is ingested",
    );
    assert_eq!(
        second.sessions_touched, 1,
        "session row is re-upserted once the new row lands",
    );
}

// ---- Skip rules ----------------------------------------------------------

#[tokio::test]
async fn malformed_line_is_skipped_not_fatal() {
    // A mid-file corruption (e.g. partial write) must not abort the whole
    // scan. The collector logs a warning and keeps going.
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");
    let file = sessions_dir.join("cascade-a.jsonl");

    append_jsonl(
        &file,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "s",
            "sonnet-4-6",
            "file:///home/alice",
        ),
    );
    // Corrupt line.
    {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .expect("open");
        writeln!(f, "{{not valid json").unwrap();
    }
    append_jsonl(
        &file,
        &turn_usage("step-1", "2026-04-19T10:01:00Z", "sonnet-4-6", 10, 5, 1),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(
        s.records_inserted, 1,
        "malformed middle line skipped; the real turn still ingested",
    );
    // The error isn't recorded at summary level — per-line parse failures
    // are logged via `tracing::warn!` to keep stdout clean for non-TUI runs.
    assert!(s.errors.is_empty(), "errors: {:?}", s.errors);
}

#[tokio::test]
async fn turn_without_timestamp_is_dropped() {
    // A step without a valid RFC3339 timestamp would land on 1970-01-01
    // in the Trend view; the collector drops it instead.
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");
    let file = sessions_dir.join("cascade-a.jsonl");

    append_jsonl(
        &file,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "s",
            "sonnet-4-6",
            "file:///home/alice",
        ),
    );
    // Bad timestamp: empty string.
    append_jsonl(
        &file,
        &turn_usage("step-bad", "", "sonnet-4-6", 100, 50, 10),
    );
    // Good row.
    append_jsonl(
        &file,
        &turn_usage("step-ok", "2026-04-19T10:01:00Z", "sonnet-4-6", 200, 80, 20),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);
    let s = c.scan(&db, &NoopReporter).await.expect("scan");

    assert_eq!(s.records_inserted, 1);
}

// ---- windsurf_sessions metadata landing ----------------------------------

#[tokio::test]
async fn scan_persists_session_meta_into_windsurf_sessions() {
    // First-pass contract: the collector must upsert the cascade's
    // presentation fields (summary / workspace / last_model /
    // created_time) into `windsurf_sessions` so the per-cascade TUI
    // drill-down view can read them without re-parsing the JSONL.
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");

    let file = sessions_dir.join("cascade-a.jsonl");
    append_jsonl(
        &file,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "Generating commit message",
            "gpt-5-codex",
            "file:///home/alice/code/a",
        ),
    );
    append_jsonl(
        &file,
        &turn_usage("step-1", "2026-04-19T10:01:00Z", "gpt-5-codex", 100, 50, 10),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);
    c.scan(&db, &NoopReporter).await.expect("scan");

    let rows = db
        .fetch_windsurf_sessions_summary(10)
        .expect("fetch summary");
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.cascade_id, "cascade-a");
    assert_eq!(r.summary, "Generating commit message");
    assert_eq!(r.workspace, "file:///home/alice/code/a");
    assert_eq!(r.last_model, "gpt-5-codex");
    assert!(r.created_time.is_some(), "created_time must be populated");
    // Aggregate counters flow through the JOIN against usage_records.
    assert_eq!(r.turns, 1);
    assert_eq!(r.total_tokens, 100 + 50 + 10);
}

#[tokio::test]
async fn resume_after_append_preserves_summary_via_empty_upsert() {
    // Resume path: the second scan replays from `resume_offset` and
    // never re-sees the file's first `session_meta` line. Because
    // WindsurfState::from_context has no summary slot, `state.summary`
    // is empty on this pass — which must NOT wipe out the row's title
    // thanks to the upsert's "empty preserves old value" rule.
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");

    let file = sessions_dir.join("cascade-a.jsonl");
    append_jsonl(
        &file,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "stable title",
            "gpt-5-codex",
            "file:///home/alice/code/a",
        ),
    );
    append_jsonl(
        &file,
        &turn_usage("step-1", "2026-04-19T10:01:00Z", "gpt-5-codex", 100, 50, 10),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);
    let _ = c.scan(&db, &NoopReporter).await.expect("first");

    // Append a new turn; the next scan resumes past the session_meta.
    append_jsonl(
        &file,
        &turn_usage("step-2", "2026-04-19T10:02:00Z", "gpt-5-codex", 200, 80, 20),
    );
    let _ = c.scan(&db, &NoopReporter).await.expect("second");

    let rows = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.summary, "stable title", "resume must not wipe summary");
    assert_eq!(r.turns, 2, "aggregate counts accumulate");
    assert_eq!(r.total_tokens, 100 + 50 + 10 + 200 + 80 + 20);
}

#[tokio::test]
async fn scan_without_session_meta_skips_windsurf_sessions_row() {
    // Edge case: exporter crashed before flushing the first
    // `session_meta` line. Such files still carry `turn_usage` rows
    // that we ingest into `usage_records` under a file-stem-derived
    // session_id, but we must NOT synthesize a `windsurf_sessions` row
    // with empty summary/workspace/last_model — that's presentation
    // junk from the TUI's point of view.
    //
    // With `has_meta` requiring at least one of created_time /
    // last_model / summary to be non-empty, this branch short-circuits.
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");

    let file = sessions_dir.join("orphan.jsonl");
    append_jsonl(
        &file,
        &turn_usage("step-1", "2026-04-19T10:01:00Z", "gpt-5-codex", 100, 50, 10),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);
    c.scan(&db, &NoopReporter).await.expect("scan");

    // usage_records got the row (under file-stem session_id).
    let sessions = db
        .fetch_recent_sessions(Some(Source::Windsurf), 10)
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "orphan");

    // But windsurf_sessions stays empty: we have nothing presentation-
    // worthy to show yet.
    let meta = db.fetch_windsurf_sessions_summary(10).expect("fetch");
    assert!(
        meta.is_empty(),
        "orphan cascade must not create a windsurf_sessions row; got {meta:?}",
    );
}

#[tokio::test]
async fn turn_missing_model_falls_back_to_session_meta_model() {
    // If a turn_usage row has an empty `model`, the parser should use the
    // last `session_meta.last_model` instead of silently writing "".
    let tmp = tempdir().expect("tempdir");
    let sessions_dir = tmp.path().join("windsurf-sessions");
    fs::create_dir_all(&sessions_dir).expect("mkdir sessions");
    let file = sessions_dir.join("cascade-a.jsonl");

    append_jsonl(
        &file,
        &session_meta(
            "cascade-a",
            "2026-04-19T10:00:00Z",
            "s",
            "sonnet-fallback",
            "file:///home/alice",
        ),
    );
    append_jsonl(
        &file,
        &turn_usage(
            "step-1",
            "2026-04-19T10:01:00Z",
            "", // empty model → fallback
            100,
            50,
            10,
        ),
    );

    let db = Db::open(&tmp.path().join("t.db")).expect("open");
    let c = WindsurfCollector::new(vec![sessions_dir]);
    c.scan(&db, &NoopReporter).await.expect("scan");

    let by_model = db.fetch_model_tallies(Some(Source::Windsurf)).unwrap();
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "sonnet-fallback");
}
