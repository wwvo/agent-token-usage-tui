-- 003_windsurf_cost_diffs: per-checkpoint server-side cost snapshot.
--
-- Rationale.
-- Windsurf's Language Server emits `CORTEX_STEP_TYPE_CHECKPOINT` steps
-- that carry the server's own USD cost estimate plus its own token
-- accounting. The companion VSCode extension writes these to JSONL as
-- `checkpoint_cost` lines (see `tools/windsurf-exporter/src/writer.ts`).
-- This table is where the collector lands them.
--
-- Design notes.
-- * `step_id` is the primary key — checkpoint steps carry a server-side
--   `executionId` UUID, so we dedup on that alone. Missing id ⇒ the
--   exporter refuses to write the line, so we never see rows with
--   empty `step_id` here.
-- * `cascade_id` mirrors `windsurf_sessions.cascade_id` + `usage_records.
--   session_id` so the TUI can join against either table without a
--   translation step. We index it for the "show me every checkpoint
--   for this cascade" query.
-- * `server_cost_usd` is intentionally **not** accumulated into
--   `usage_records.cost_usd` — that would double-count against atut's
--   own `pricing::cost::calc_cost`. The cross-check view computes the
--   delta on the fly.
-- * We don't store `our_cost_usd` here. It's derivable from
--   `usage_records` joined on `(cascade_id, timestamp range)` and we
--   don't want a cached column to drift when pricing gets re-synced.
-- * `timestamp DESC` index: the Trend-adjacent "has drift grown
--   recently?" query pulls the most recent N rows.

CREATE TABLE IF NOT EXISTS windsurf_cost_diffs (
    step_id TEXT PRIMARY KEY,
    cascade_id TEXT NOT NULL,
    timestamp DATETIME NOT NULL,
    model TEXT DEFAULT '',
    server_cost_usd REAL NOT NULL,
    server_input_tokens INTEGER DEFAULT 0,
    server_output_tokens INTEGER DEFAULT 0,
    server_cache_read_tokens INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_windsurf_cost_diffs_cascade
    ON windsurf_cost_diffs(cascade_id);

CREATE INDEX IF NOT EXISTS idx_windsurf_cost_diffs_timestamp
    ON windsurf_cost_diffs(timestamp DESC);
