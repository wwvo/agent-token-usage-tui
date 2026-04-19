//! Terminal UI — ratatui + crossterm driven k9s-style interface.
//!
//! Entry point is [`run`], which owns the full lifecycle:
//!
//! 1. Switch stdout into raw alt-screen mode (crossterm).
//! 2. Build an [`App`] bound to the shared SQLite [`Db`] and populate it.
//! 3. Loop: draw → poll events → apply keypresses → optionally refresh data.
//! 4. On quit, restore the terminal regardless of whether the draw loop
//!    errored — no stuck raw-mode sessions.

pub mod app;
pub mod render;

pub use app::App;
pub use app::View;

use std::io::Stdout;
use std::io::stdout;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use crossterm::event::DisableMouseCapture;
use crossterm::event::EnableMouseCapture;
use crossterm::event::Event;
use crossterm::event::poll as event_poll;
use crossterm::event::read as event_read;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::storage::Db;

/// How often the draw loop wakes up even when the user is idle.
///
/// Short enough that `r`efresh + footer transitions feel snappy, long enough
/// that an idle terminal doesn't burn CPU.
const TICK: Duration = Duration::from_millis(200);

/// Launch the k9s-style TUI against the given database.
///
/// Runs on the caller's async runtime (we stay `async` even though the loop
/// is synchronous; the Tokio runtime is in the picture because future
/// refreshes will want `tokio::spawn` for background rescans).
pub async fn run(db: Db) -> Result<()> {
    let mut terminal = enter_terminal().context("enter terminal raw mode")?;
    let result = run_loop(&mut terminal, db).await;
    // Restore the terminal *before* propagating any error so stale raw mode
    // doesn't leave the user's shell unusable.
    let leave = leave_terminal(&mut terminal);
    result.and(leave)
}

/// Inner loop extracted so `run` can always restore the terminal, even on error.
async fn run_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, db: Db) -> Result<()> {
    let mut app = App::new(db);
    app.refresh();

    while !app.should_quit {
        terminal
            .draw(|f| render::draw(f, &app))
            .context("ratatui draw")?;

        if event_poll(TICK).context("crossterm poll")? {
            // Only key events drive state; resize / mouse / focus etc. are
            // handled implicitly by the next `terminal.draw` call.
            if let Event::Key(k) = event_read().context("crossterm read")? {
                app.on_key(k);
            }
        }
    }
    Ok(())
}

fn enter_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable_raw_mode")?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture).context("enter alt screen")?;
    Terminal::new(CrosstermBackend::new(out)).context("new Terminal")
}

fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable_raw_mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
    )
    .context("leave alt screen")?;
    terminal.show_cursor().context("show cursor")?;
    Ok(())
}
