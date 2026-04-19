-- 001_init: Initial schema for agent-token-usage-tui.
--
-- Tables:
--   * usage_records  — one row per LLM API call (token counts + cost)
--   * sessions       — slow-moving metadata per coding-agent session
--   * prompt_events  — one row per real user prompt (not tool result)
--   * file_state     — incremental scan checkpoint per source file
--   * pricing        — litellm model prices (synced at startup)
--   * meta           — key/value for migration tracking and future config
--
-- Mirrors agent-usage's schema so analysis SQL can port between the two.

CREATE TABLE IF NOT EXISTS usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    session_id TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_creation_input_tokens INTEGER DEFAULT 0,
    cache_read_input_tokens INTEGER DEFAULT 0,
    reasoning_output_tokens INTEGER DEFAULT 0,
    cost_usd REAL DEFAULT 0,
    timestamp DATETIME NOT NULL,
    project TEXT DEFAULT '',
    git_branch TEXT DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_records(timestamp);
CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_records(session_id);
CREATE INDEX IF NOT EXISTS idx_usage_source ON usage_records(source);
CREATE UNIQUE INDEX IF NOT EXISTS idx_usage_dedup
    ON usage_records(session_id, model, timestamp, input_tokens, output_tokens);

CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    session_id TEXT NOT NULL UNIQUE,
    project TEXT DEFAULT '',
    cwd TEXT DEFAULT '',
    version TEXT DEFAULT '',
    git_branch TEXT DEFAULT '',
    start_time DATETIME,
    prompts INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS prompt_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    session_id TEXT NOT NULL,
    timestamp DATETIME NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_prompt_timestamp ON prompt_events(timestamp);
CREATE UNIQUE INDEX IF NOT EXISTS idx_prompt_dedup
    ON prompt_events(session_id, timestamp);

CREATE TABLE IF NOT EXISTS file_state (
    path TEXT PRIMARY KEY,
    size INTEGER DEFAULT 0,
    last_offset INTEGER DEFAULT 0,
    scan_context TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS pricing (
    model TEXT PRIMARY KEY,
    input_cost_per_token REAL DEFAULT 0,
    output_cost_per_token REAL DEFAULT 0,
    cache_read_input_token_cost REAL DEFAULT 0,
    cache_creation_input_token_cost REAL DEFAULT 0,
    updated_at DATETIME
);
