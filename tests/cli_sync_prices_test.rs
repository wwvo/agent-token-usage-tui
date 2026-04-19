//! E2E test for `atut sync-prices`.
//!
//! We don't block the test on network reachability: `pricing::sync_or_fallback`
//! always lands *some* pricing (network if reachable, embedded fallback
//! otherwise), and the CLI prints one of three known lines. We assert:
//!
//! * exit 0
//! * stdout starts with "Pricing " (the only prefix the three outcomes share)
//! * stdout includes a "Re-priced N usage rows." summary
//! * a second invocation short-circuits on freshness.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_agent-token-usage-tui");

fn run_sync(tmp: &TempDir) -> std::process::Output {
    Command::new(BIN)
        .env("HOME", tmp.path())
        .env("USERPROFILE", tmp.path())
        .arg("--data-dir")
        .arg(tmp.path())
        .arg("sync-prices")
        .output()
        .expect("spawn atut sync-prices")
}

#[test]
fn sync_prices_populates_and_prints_summary() {
    let tmp = TempDir::new().expect("tempdir");
    let output = run_sync(&tmp);

    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");

    assert!(
        output.status.success(),
        "sync-prices should exit 0.\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    assert!(
        stdout.starts_with("Pricing "),
        "unexpected first line. stdout:\n{stdout}",
    );
    assert!(
        stdout.contains("Re-priced "),
        "missing re-price line. stdout:\n{stdout}",
    );
    assert!(tmp.path().join("data.db").exists(), "db should be created");
}

#[test]
fn second_sync_short_circuits_on_freshness() {
    let tmp = TempDir::new().expect("tempdir");

    // First sync populates + stamps updated_at = now.
    let first = run_sync(&tmp);
    assert!(first.status.success(), "first sync must succeed");

    // Second run within the 24h window must take the "still fresh" path.
    let second = run_sync(&tmp);
    let stdout = String::from_utf8(second.stdout).expect("utf8");
    assert!(
        second.status.success(),
        "second sync should exit 0. stdout:\n{stdout}",
    );
    assert!(
        stdout.contains("cache is fresh"),
        "expected freshness short-circuit. stdout:\n{stdout}",
    );
}
