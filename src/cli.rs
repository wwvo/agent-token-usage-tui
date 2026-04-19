//! Command-line surface and top-level dispatcher.
//!
//! This module owns `clap` parsing and routes each subcommand to the relevant
//! subsystem. `main.rs` is a thin wrapper around [`run`] so the dispatch logic
//! stays library-testable and reusable from integration tests or future tools
//! (e.g. the Phase 2 Windsurf exporter).

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::ArgAction;
use clap::Parser;
use clap::Subcommand;

use crate::logging;
use crate::logging::LogMode;

/// Top-level CLI schema.
///
/// Global flags (`--config`, `--data-dir`, `-v`, `--no-scan`, `--no-prices`)
/// are defined here so every subcommand — and the default TUI entry point —
/// sees the same surface. Several fields are currently `#[allow(dead_code)]`:
/// they are parsed now to stabilize the UX while the subsystems that consume
/// them land in later phases (M2 C7, M4 C5, M5 C6, M7 C3).
#[derive(Debug, Parser)]
#[command(
    name = env!("CARGO_PKG_NAME"),
    version,
    about,
    long_about = None,
)]
#[allow(dead_code)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Override path to `config.toml` (defaults to `<exe-dir>/config.toml`).
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,

    /// Override the portable runtime directory (defaults to `<exe-dir>`).
    ///
    /// Useful for integration tests that want an isolated sandbox.
    #[arg(long, value_name = "DIR", global = true)]
    data_dir: Option<PathBuf>,

    /// Increase verbosity (`-v` = debug, `-vv` = trace). Overrides `RUST_LOG`.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    /// Skip the initial scan when entering the TUI.
    #[arg(long, global = true)]
    no_scan: bool,

    /// Skip the pricing sync on startup; use whatever is already cached.
    #[arg(long, global = true)]
    no_prices: bool,
}

/// Subcommands exposed to users.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scan known agent session directories, update the database, and exit.
    ///
    /// Intended for cron / scheduled invocation where a TUI is inappropriate.
    Scan,

    /// Refresh the litellm model pricing cache and recompute costs, then exit.
    SyncPrices,

    /// Print version information and exit.
    Version,
}

/// CLI entry point used by `main.rs`.
///
/// Errors bubble up from subsystems; `main.rs` is responsible for rendering
/// them and choosing an exit code.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    logging::init(LogMode::Stderr)?;

    match cli.command {
        None => {
            // M5 C6 replaces this with `tui::run(...).await` under a tokio runtime.
            todo!("TUI entry point lands in M5 C6; use `version` / `scan` subcommands for now")
        }
        Some(Commands::Scan) => {
            // M4 C5 wires up `pipeline::run_scan` here.
            todo!("scan pipeline lands in M4 C5")
        }
        Some(Commands::SyncPrices) => {
            // M2 C7 implements `pricing::sync_or_fallback`; M4 C5 calls it here.
            todo!("pricing sync lands in M2 C7 + M4 C5")
        }
        Some(Commands::Version) => print_version(),
    }
}

/// Write the short version line to stdout.
///
/// We use `writeln!` on an acquired `stdout` handle instead of `println!` so
/// the workspace `clippy::print_stdout = "deny"` stays enforced — `println!`
/// everywhere else in the code base is a bug.
fn print_version() -> Result<()> {
    // TODO(M7 C3): embed short git hash + build timestamp via build.rs envs.
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(())
}
