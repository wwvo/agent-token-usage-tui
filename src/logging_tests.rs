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
fn init_file_into_creates_log_directory_and_returns_guard() {
    use tempfile::tempdir;
    let tmp = tempdir().expect("tempdir");
    let dir = tmp.path().join("logs");
    // Dir doesn't exist yet; init_file_into must create it.
    assert!(!dir.exists(), "precondition: log dir does not pre-exist");

    let _guard = init_file_into(dir.clone()).expect("file logging init");
    assert!(dir.exists(), "log dir must be created");
    assert!(dir.is_dir());
    // Guard dropping here is fine for the test; this asserts the API shape,
    // not the actual file writes (those happen asynchronously in a worker).
}

#[test]
fn log_mode_equality_is_structural() {
    assert_eq!(LogMode::Stderr, LogMode::Stderr);
    assert_ne!(LogMode::Stderr, LogMode::File);
}
