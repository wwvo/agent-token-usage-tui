//! Sidecar tests for `Db::get_file_state` / `Db::set_file_state`.

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::Db;
use super::FileScanContext;

fn new_db() -> (tempfile::TempDir, Db) {
    let tmp = tempdir().expect("tempdir");
    let db = Db::open(&tmp.path().join("test.db")).expect("open db");
    (tmp, db)
}

fn sample_path() -> PathBuf {
    PathBuf::from("/tmp/session.jsonl")
}

#[test]
fn unknown_path_returns_zero_defaults() {
    let (_tmp, db) = new_db();
    let (size, offset, ctx) = db.get_file_state(&sample_path()).expect("read");
    assert_eq!(size, 0);
    assert_eq!(offset, 0);
    assert_eq!(ctx, None);
}

#[test]
fn set_then_get_roundtrips_numeric_fields() {
    let (_tmp, db) = new_db();
    db.set_file_state(&sample_path(), 1_024, 512, None)
        .expect("set");

    let (size, offset, ctx) = db.get_file_state(&sample_path()).expect("read");
    assert_eq!(size, 1_024);
    assert_eq!(offset, 512);
    assert_eq!(ctx, None, "no context when saved with None");
}

#[test]
fn set_then_get_roundtrips_context_json() {
    let (_tmp, db) = new_db();
    let ctx = FileScanContext {
        session_id: "sess-abc".into(),
        cwd: "/home/user/proj".into(),
        version: "0.42.0".into(),
        model: "gpt-5-codex".into(),
    };
    db.set_file_state(&sample_path(), 2_000, 2_000, Some(&ctx))
        .expect("set");

    let (_size, _offset, got) = db.get_file_state(&sample_path()).expect("read");
    assert_eq!(got, Some(ctx));
}

#[test]
fn repeated_set_overwrites_previous_checkpoint() {
    let (_tmp, db) = new_db();
    let path = sample_path();
    db.set_file_state(&path, 100, 100, None).expect("v1");
    db.set_file_state(&path, 200, 200, None).expect("v2");
    db.set_file_state(&path, 300, 300, None).expect("v3");

    let (size, offset, _ctx) = db.get_file_state(&path).expect("read");
    assert_eq!(size, 300);
    assert_eq!(offset, 300);
}

#[test]
fn distinct_paths_have_independent_checkpoints() {
    let (_tmp, db) = new_db();
    let p1 = PathBuf::from("/tmp/a.jsonl");
    let p2 = PathBuf::from("/tmp/b.jsonl");
    db.set_file_state(&p1, 100, 100, None).expect("p1");
    db.set_file_state(&p2, 999, 500, None).expect("p2");

    assert_eq!(db.get_file_state(&p1).expect("r1").0, 100);
    assert_eq!(db.get_file_state(&p2).expect("r2").0, 999);
}

#[test]
fn corrupted_context_json_falls_back_to_none() {
    let (_tmp, db) = new_db();
    // Inject garbage directly so the JSON deserialization fails gracefully.
    {
        let conn = db.lock();
        conn.execute(
            "INSERT INTO file_state(path, size, last_offset, scan_context)
             VALUES('/tmp/x.jsonl', 10, 10, '{not-valid-json')",
            [],
        )
        .expect("seed garbage");
    }

    let (size, offset, ctx) = db
        .get_file_state(&PathBuf::from("/tmp/x.jsonl"))
        .expect("read");
    assert_eq!((size, offset), (10, 10));
    assert_eq!(ctx, None, "malformed context must not crash the collector");
}
