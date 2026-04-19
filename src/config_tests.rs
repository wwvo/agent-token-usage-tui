//! Sidecar tests for `Config::load_or_default` and `to_pipeline`.

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Config;

#[test]
fn missing_file_returns_default() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("nope.toml");
    assert!(!path.exists());

    let cfg = Config::load_or_default(&path).expect("missing file is fine");
    assert!(cfg.openclaw_bases.is_empty());
    assert!(cfg.opencode_dbs.is_empty());
    assert!(cfg.windsurf_bases.is_empty());
}

#[test]
fn empty_file_returns_default() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("empty.toml");
    std::fs::write(&path, "").expect("write empty");

    let cfg = Config::load_or_default(&path).expect("empty is fine");
    assert!(cfg.openclaw_bases.is_empty());
}

#[test]
fn populated_file_roundtrips_all_three_lists() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
openclaw_bases = ["/a/oc", "/b/oc"]
opencode_dbs   = ["/c/opencode.db"]
windsurf_bases = ["/d/ws"]
"#,
    )
    .unwrap();

    let cfg = Config::load_or_default(&path).expect("parse config");
    assert_eq!(
        cfg.openclaw_bases,
        vec![PathBuf::from("/a/oc"), PathBuf::from("/b/oc")]
    );
    assert_eq!(cfg.opencode_dbs, vec![PathBuf::from("/c/opencode.db")]);
    assert_eq!(cfg.windsurf_bases, vec![PathBuf::from("/d/ws")]);
}

#[test]
fn partial_file_keeps_missing_fields_default() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("partial.toml");
    std::fs::write(&path, r#"opencode_dbs = ["/only.db"]"#).unwrap();

    let cfg = Config::load_or_default(&path).expect("partial");
    assert!(cfg.openclaw_bases.is_empty());
    assert_eq!(cfg.opencode_dbs, vec![PathBuf::from("/only.db")]);
    assert!(cfg.windsurf_bases.is_empty());
}

#[test]
fn unknown_keys_fail_loudly_not_silently() {
    // A config typo should not be silently dropped — users would spend
    // hours wondering why their paths don't take effect.
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("typo.toml");
    std::fs::write(&path, r#"opencode_path = "/typoed""#).unwrap();

    let err = Config::load_or_default(&path).expect_err("typo must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("parse config"),
        "error should reference parsing; got: {msg}",
    );
}

#[test]
fn to_pipeline_copies_all_three_lists() {
    let cfg = Config {
        openclaw_bases: vec![PathBuf::from("/a")],
        opencode_dbs: vec![PathBuf::from("/b")],
        windsurf_bases: vec![PathBuf::from("/c")],
    };
    let pipe = cfg.to_pipeline();
    assert_eq!(pipe.openclaw_bases, cfg.openclaw_bases);
    assert_eq!(pipe.opencode_dbs, cfg.opencode_dbs);
    assert_eq!(pipe.windsurf_bases, cfg.windsurf_bases);
}
