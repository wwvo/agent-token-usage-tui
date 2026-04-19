-- 002_windsurf_sessions: per-cascade metadata store for Windsurf.
--
-- Rationale.
-- The Windsurf collector (`src/collector/windsurf.rs`) ingests append-only
-- JSONL files emitted by the companion VSCode extension. The very first
-- line of each file is a `session_meta` object carrying the cascade's
-- human-readable title (`summary`), workspace URI, and server-recorded
-- `created_time`. Until this migration those fields never left the
-- JSONL — the `sessions` table stores *usage-oriented* fields shared
-- across every source, while `session_meta.summary` is Windsurf-specific
-- presentation data that would only clutter the cross-source row shape.
--
-- A dedicated table keeps the cross-source `sessions` schema stable and
-- gives the TUI a cheap `SELECT` target for the planned per-cascade
-- drill-down view (see plans/windsurf-exporter-future-improvements.md
-- "View B — Per-cascade 'Sessions' drill-down").
--
-- Design notes.
-- * `cascade_id` doubles as the primary key AND the join key against
--   `usage_records.session_id` / `sessions.session_id`. Using TEXT
--   (not INTEGER) mirrors the `sessions` schema so JOINs stay direct.
-- * `created_time` is nullable: older JSONL files (or exporter crashes
--   before the first `session_meta` flush) can lack it. We fall back to
--   `last_seen` for display ordering when null.
-- * `last_seen` captures the most recent time we saw this cascade during
--   a scan. Indexing it descending is the primary access pattern for
--   the drill-down ("newest cascade first").

CREATE TABLE IF NOT EXISTS windsurf_sessions (
    cascade_id TEXT PRIMARY KEY,
    summary TEXT DEFAULT '',
    workspace TEXT DEFAULT '',
    last_model TEXT DEFAULT '',
    created_time DATETIME,
    last_seen DATETIME NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_windsurf_sessions_last_seen
    ON windsurf_sessions(last_seen DESC);
