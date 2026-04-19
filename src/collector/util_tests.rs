//! Sidecar tests for [`has_tool_result_block`], [`is_real_user_prompt`], and
//! [`read_jsonl_from_offset`].

use std::io::Write;

use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::NamedTempFile;

use super::has_tool_result_block;
use super::is_real_user_prompt;
use super::read_jsonl_from_offset;

// ---- has_tool_result_block ------------------------------------------------

#[test]
fn string_content_is_not_tool_result() {
    assert!(!has_tool_result_block(&json!("hello")));
}

#[test]
fn empty_array_is_not_tool_result() {
    assert!(!has_tool_result_block(&json!([])));
}

#[test]
fn array_of_text_blocks_is_not_tool_result() {
    let content = json!([
        { "type": "text", "text": "hi" },
        { "type": "text", "text": "there" }
    ]);
    assert!(!has_tool_result_block(&content));
}

#[test]
fn array_with_tool_result_block_matches() {
    let content = json!([
        { "type": "text", "text": "pre" },
        { "type": "tool_result", "content": "pong", "tool_use_id": "t1" }
    ]);
    assert!(has_tool_result_block(&content));
}

#[test]
fn object_content_without_type_field_is_not_tool_result() {
    assert!(!has_tool_result_block(&json!({"foo": "bar"})));
}

// ---- is_real_user_prompt --------------------------------------------------

#[test]
fn real_user_prompt_is_accepted() {
    let msg = json!({ "role": "user", "content": "hello there" });
    assert!(is_real_user_prompt(&msg));
}

#[test]
fn real_user_prompt_with_text_blocks_is_accepted() {
    let msg = json!({
        "role": "user",
        "content": [{ "type": "text", "text": "hello" }],
    });
    assert!(is_real_user_prompt(&msg));
}

#[test]
fn user_role_with_tool_result_is_rejected() {
    let msg = json!({
        "role": "user",
        "content": [{ "type": "tool_result", "content": "ok" }],
    });
    assert!(!is_real_user_prompt(&msg));
}

#[test]
fn assistant_role_is_rejected() {
    let msg = json!({ "role": "assistant", "content": "hi" });
    assert!(!is_real_user_prompt(&msg));
}

#[test]
fn missing_role_is_rejected() {
    let msg = json!({ "content": "orphan" });
    assert!(!is_real_user_prompt(&msg));
}

#[test]
fn missing_content_is_rejected() {
    let msg = json!({ "role": "user" });
    assert!(!is_real_user_prompt(&msg));
}

// ---- read_jsonl_from_offset ----------------------------------------------

fn tmpfile_with_lines(lines: &[&str]) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("temp file");
    for line in lines {
        writeln!(f, "{line}").expect("write line");
    }
    f
}

#[test]
fn read_jsonl_parses_all_lines_from_zero_offset() {
    let f = tmpfile_with_lines(&[r#"{"a": 1}"#, r#"{"b": 2}"#, r#"{"c": 3}"#]);
    let rows: Vec<_> = read_jsonl_from_offset(f.path(), 0)
        .expect("open")
        .collect::<Result<Vec<_>, _>>()
        .expect("parse");

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["a"], 1);
    assert_eq!(rows[2]["c"], 3);
}

#[test]
fn read_jsonl_skips_blank_lines() {
    let f = tmpfile_with_lines(&[r#"{"x": 1}"#, "", r#"{"y": 2}"#, "   "]);
    let rows: Vec<_> = read_jsonl_from_offset(f.path(), 0)
        .expect("open")
        .collect::<Result<Vec<_>, _>>()
        .expect("parse");

    assert_eq!(rows.len(), 2);
}

#[test]
fn read_jsonl_resumes_from_offset_correctly() {
    // "{"a":1}\n" is 8 bytes; starting from offset 8 yields lines 2–3.
    let f = tmpfile_with_lines(&[r#"{"a":1}"#, r#"{"b":2}"#, r#"{"c":3}"#]);

    let first_line_len = std::fs::metadata(f.path()).expect("meta").len();
    assert!(
        first_line_len >= 24,
        "fixture should have at least 3 lines worth"
    );

    let rows: Vec<_> = read_jsonl_from_offset(f.path(), 8)
        .expect("open")
        .collect::<Result<Vec<_>, _>>()
        .expect("parse");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["b"], 2);
    assert_eq!(rows[1]["c"], 3);
}

#[test]
fn read_jsonl_returns_per_line_parse_errors_not_aborts() {
    let f = tmpfile_with_lines(&[r#"{"ok": 1}"#, r#"{not valid json"#, r#"{"ok": 2}"#]);

    let rows: Vec<_> = read_jsonl_from_offset(f.path(), 0).expect("open").collect();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].is_ok());
    assert!(rows[1].is_err(), "middle line is malformed");
    assert!(rows[2].is_ok(), "later lines must still parse");
}
