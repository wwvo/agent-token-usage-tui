//! Sidecar tests for `logging`.

use super::*;

#[test]
fn stderr_init_is_idempotent() {
    // First call either installs the subscriber or is a no-op when cargo test
    // has already registered one for a previous case. Either way it must Ok.
    init(LogMode::Stderr).expect("init stderr");
    init(LogMode::Stderr).expect("second init must not error");
}

#[test]
fn file_mode_returns_todo_error_until_m7() {
    let err = init(LogMode::File).expect_err("File mode must error before M7 C1");
    let msg = err.to_string();
    assert!(
        msg.contains("M7 C1"),
        "error must reference M7 C1 landing point; got: {msg}",
    );
}

#[test]
fn log_mode_equality_is_structural() {
    assert_eq!(LogMode::Stderr, LogMode::Stderr);
    assert_ne!(LogMode::Stderr, LogMode::File);
}
