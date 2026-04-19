//! End-to-end test for the `atut scan` subcommand.
//!
//! Spawns the real compiled binary (path courtesy of cargo's
//! `CARGO_BIN_EXE_<name>` env var) with a scratch `--data-dir` and both
//! `HOME`/`USERPROFILE` overridden so it cannot accidentally touch the
//! developer's real `~/.claude/` or `~/.codex/`. We then assert the summary
//! is emitted on stdout and the process exits cleanly.
//!
//! This locks the CLI contract (`Scan complete:` line + per-source counters)
//! so refactors can't silently change the output format, and double-checks
//! that the scan is a valid no-op against an empty environment.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::process::Command;

use tempfile::TempDir;

/// Cargo exposes every compiled binary via this env var at test-build time.
const BIN: &str = env!("CARGO_BIN_EXE_agent-token-usage-tui");

#[test]
fn scan_against_empty_home_exits_cleanly_and_prints_summary() {
    let tmp = TempDir::new().expect("tempdir");

    let output = Command::new(BIN)
        // Overriding both keeps the test correct on every OS we target.
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .arg("--data-dir")
        .arg(tmp.path())
        .arg("scan")
        .output()
        .expect("spawn atut scan");

    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");

    assert!(
        output.status.success(),
        "scan should exit 0. stdout:\n{stdout}\nstderr:\n{stderr}",
    );

    assert!(
        stdout.contains("Scan complete:"),
        "missing header. stdout:\n{stdout}",
    );
    assert!(
        stdout.contains("claude:"),
        "missing claude line. stdout:\n{stdout}",
    );
    assert!(
        stdout.contains("codex:"),
        "missing codex line. stdout:\n{stdout}",
    );
    // Empty HOME means zero files to scan — lock this to catch regressions
    // where the binary accidentally uses the real user's sessions dir.
    assert!(
        stdout.contains("files=0"),
        "expected files=0 on empty HOME. stdout:\n{stdout}",
    );
    assert!(
        stdout.contains("costs:"),
        "missing costs summary. stdout:\n{stdout}",
    );

    // Also verify the DB file got created so a follow-up `sync-prices` or
    // TUI launch can find it at the documented portable location.
    let db_path = tmp.path().join("data.db");
    assert!(
        db_path.exists(),
        "database should be created at {}",
        db_path.display(),
    );
}

#[test]
fn scan_prints_help_entry_listing_subcommand() {
    // Quick smoke test: `--help` should mention the scan subcommand so the UX
    // stays discoverable after every refactor.
    let output = Command::new(BIN).arg("--help").output().expect("spawn");

    let stdout = String::from_utf8(output.stdout).expect("help utf8");
    assert!(output.status.success(), "--help should exit 0");
    assert!(stdout.contains("scan"), "--help missing scan subcommand");
}
