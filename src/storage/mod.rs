//! SQLite-backed storage layer.
//!
//! [`Db`] is the single public entry point; it wraps a `rusqlite::Connection`
//! behind `Arc<Mutex<...>>` so every caller serializes writes. The mutex is
//! intentional: SQLite itself supports concurrent reads under WAL, but a
//! single-writer model is the simplest correct behavior for a portable CLI
//! that only ever has one process holding the DB.
//!
//! # Modules
//!
//! * [`schema`] — migration driver + `migrations/*.sql` loader.
//! * Future: `file_state` (M2 C3), `records` (M2 C3), `pricing` (M2 C4),
//!   `costs` (M2 C5), `queries` (M5 C1).

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use anyhow::Context;
use anyhow::Result;
use rusqlite::Connection;

pub mod costs;
pub mod file_state;
pub mod pricing;
pub mod records;
pub mod schema;

pub use file_state::FileScanContext;

/// Owned handle to the portable SQLite database.
///
/// Cheap to `Clone` (reference-counted), so caller threads can each hold their
/// own copy without re-opening the connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open (or create) the SQLite file at `path`, enable WAL + busy_timeout,
    /// and run any pending schema migrations.
    ///
    /// # Errors
    ///
    /// Surfaces any rusqlite error from `open`, the `PRAGMA` updates, or
    /// migrations, annotated with file-path context.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("open sqlite database at {}", path.display()))?;

        // WAL journaling: Windows-friendly concurrent reads + small write amplification.
        conn.pragma_update(None, "journal_mode", "WAL")
            .context("set journal_mode = WAL")?;
        // Short wait on write contention; portable CLI rarely needs longer.
        conn.pragma_update(None, "busy_timeout", 5_000_i32)
            .context("set busy_timeout = 5000 ms")?;

        schema::migrate(&conn).context("run schema migrations")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Acquire the underlying connection guard.
    ///
    /// Higher-level modules (`records`, `pricing`, `queries`) build typed APIs
    /// on top of this; downstream consumers — including the CLI, TUI and
    /// integration tests — can also reach for this as an escape hatch when
    /// they need read-only SQL beyond what the typed wrappers cover.
    ///
    /// If the mutex is poisoned (another thread panicked while holding it),
    /// recover the inner guard rather than propagating the panic — at the
    /// SQLite layer poisoning is almost always benign (read-only corruption
    /// of cached statements), and killing the process is worse than continuing.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
