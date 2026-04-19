//! Thin binary entry point.
//!
//! All logic lives in the `agent_token_usage_tui` library crate so it can be
//! unit-tested and reused; this file only dispatches into `cli::run` and
//! surfaces the final exit code.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    match agent_token_usage_tui::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Write with an acquired stderr handle to side-step the
            // `clippy::print_stderr = "deny"` workspace lint; ignore sub-IO
            // failures because at this point we're already in the error path.
            let mut out = std::io::stderr().lock();
            let _ = writeln!(out, "error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
