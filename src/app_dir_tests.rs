//! Sidecar tests for `app_dir`.
//!
//! This file is included via `#[path = "app_dir_tests.rs"] mod tests;` at the
//! bottom of `app_dir.rs`, so it is a submodule of `app_dir` and can access
//! private items (constants like `CONFIG_FILENAME`).

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn exe_dir_returns_existing_absolute_path() {
    let dir = exe_dir().expect("exe_dir should succeed under cargo test");
    assert!(
        dir.is_absolute(),
        "exe_dir should be absolute: {}",
        dir.display()
    );
    assert!(
        dir.exists(),
        "exe_dir must exist on disk: {}",
        dir.display()
    );
}

#[test]
fn config_path_lives_under_exe_dir_with_expected_filename() {
    let dir = exe_dir().expect("exe_dir");
    let cfg = config_path().expect("config_path");

    assert_eq!(cfg.parent().expect("config has parent"), dir.as_path());
    assert_eq!(
        cfg.file_name().and_then(|n| n.to_str()),
        Some(CONFIG_FILENAME)
    );
}

#[test]
fn db_path_lives_under_exe_dir_with_expected_filename() {
    let dir = exe_dir().expect("exe_dir");
    let db = db_path().expect("db_path");

    assert_eq!(db.parent().expect("db has parent"), dir.as_path());
    assert_eq!(db.file_name().and_then(|n| n.to_str()), Some(DB_FILENAME));
}

#[test]
fn pricing_cache_path_lives_under_exe_dir_with_expected_filename() {
    let dir = exe_dir().expect("exe_dir");
    let cache = pricing_cache_path().expect("pricing_cache_path");

    assert_eq!(cache.parent().expect("cache has parent"), dir.as_path());
    assert_eq!(
        cache.file_name().and_then(|n| n.to_str()),
        Some(PRICING_CACHE_FILENAME),
    );
}

#[test]
fn log_dir_is_created_on_demand() {
    let dir = log_dir().expect("log_dir");

    assert!(
        dir.is_dir(),
        "log_dir must be a directory: {}",
        dir.display()
    );
    assert_eq!(dir.file_name().and_then(|n| n.to_str()), Some(LOG_DIRNAME));

    // Idempotent: calling again must not error.
    let again = log_dir().expect("log_dir second call");
    assert_eq!(dir, again);
}
