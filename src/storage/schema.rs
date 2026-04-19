//! SQL schema migrations.
//!
//! # Design
//!
//! Migrations are identified by short stable strings (`"001_init"`) and
//! tracked in the `meta` table under keys like `migration_<id> = "done"`. The
//! driver is intentionally minimal: each migration is a string of SQL that
//! runs exactly once per database, in the order listed in the private `MIGRATIONS` table.
//!
//! The bootstrap `meta` table is created eagerly before we read migration
//! state, so the very first migration (`001_init`) can also re-declare `meta`
//! with `CREATE TABLE IF NOT EXISTS` without collision.
//!
//! SQL lives in `migrations/*.sql` (Codex-inspired: prompts and SQL off-board
//! the `.rs` files so non-Rust contributors can read and diff them cleanly).

use anyhow::Context;
use anyhow::Result;
use rusqlite::Connection;
use rusqlite::OptionalExtension;

/// One migration step applied by [`migrate`].
struct Migration {
    /// Stable identifier stored as `migration_<id>` in the `meta` table.
    id: &'static str,
    /// SQL to run. May contain multiple statements — we use `execute_batch`.
    sql: &'static str,
}

/// Chronological migration list; append only.
const MIGRATIONS: &[Migration] = &[
    Migration {
        id: "001_init",
        sql: include_str!("../../migrations/001_init.sql"),
    },
    Migration {
        id: "002_windsurf_sessions",
        sql: include_str!("../../migrations/002_windsurf_sessions.sql"),
    },
];

/// Apply every migration that has not yet been recorded as `done` in the
/// current database.
///
/// This function is safe to call on every startup: already-applied migrations
/// are skipped, and the whole sequence is idempotent.
pub fn migrate(conn: &Connection) -> Result<()> {
    // Bootstrap `meta` so we have somewhere to record migration state. The
    // very first migration also declares this table via
    // `CREATE TABLE IF NOT EXISTS`, which is fine — the table then already
    // matches and the statement is a no-op.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
             key TEXT PRIMARY KEY,
             value TEXT DEFAULT ''
         );",
    )
    .context("bootstrap meta table")?;

    for m in MIGRATIONS {
        let key = format!("migration_{}", m.id);
        let existing: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [&key], |row| {
                row.get(0)
            })
            .optional()
            .context("read migration state from meta")?;

        if existing.as_deref() == Some("done") {
            continue;
        }

        conn.execute_batch(m.sql)
            .with_context(|| format!("apply migration {}", m.id))?;

        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key.as_str(), "done"],
        )
        .with_context(|| format!("mark migration {} as done", m.id))?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
