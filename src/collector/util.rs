//! Shared JSONL parsing helpers used by the Claude / Codex / OpenClaw collectors.
//!
//! Why this lives outside each collector module: the `role: "user"` vs
//! `tool_result` distinction is a recurring trap — every agent that speaks
//! Anthropic-style messages stores tool call outputs as messages with
//! `role: "user"` and `content: [{ type: "tool_result", ... }]`. Without a
//! shared filter, collectors would each reimplement the check and drift.

use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use serde_json::Value;

/// Does this message's `content` field hold at least one `tool_result` block?
///
/// `content` comes in two shapes:
///
/// * `"a string"` — plain text prompt.
/// * `[ {"type": "tool_result", ...}, {"type": "text", ...}, ... ]` — array of
///   blocks.
///
/// Returns `true` only in the array case when at least one block has
/// `type == "tool_result"`. Plain strings and arrays of text/image blocks
/// return `false`.
#[must_use]
pub fn has_tool_result_block(content: &Value) -> bool {
    content
        .as_array()
        .is_some_and(|blocks| blocks.iter().any(is_tool_result_block))
}

fn is_tool_result_block(block: &Value) -> bool {
    block
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t == "tool_result")
}

/// Is this message a real user-typed prompt (as opposed to a tool-result
/// delivery message that happens to wear `role: "user"`)?
#[must_use]
pub fn is_real_user_prompt(msg: &Value) -> bool {
    let is_user = msg
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|r| r == "user");
    if !is_user {
        return false;
    }
    match msg.get("content") {
        Some(content) => !has_tool_result_block(content),
        None => false, // no content ⇒ not a real prompt
    }
}

/// Lazily read a JSONL file starting from `offset`, yielding one parsed value
/// per non-empty line.
///
/// Collectors prefer this over `read_to_string + split('\n')` because session
/// files can reach hundreds of megabytes; streaming keeps memory bounded.
///
/// # Errors
///
/// The outer `Result` surfaces file-open / seek failures. Per-line errors
/// (I/O read errors, malformed JSON) surface as `Err` variants in the
/// iterator so the caller can decide whether to skip, log, or abort.
pub fn read_jsonl_from_offset(
    path: &Path,
    offset: u64,
) -> Result<impl Iterator<Item = Result<Value>>> {
    let mut file =
        File::open(path).with_context(|| format!("open JSONL file {}", path.display()))?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("seek {} to offset {offset}", path.display()))?;
    }
    let reader = BufReader::new(file);
    let lines = reader.lines().filter_map(|line_result| match line_result {
        Ok(line) if line.trim().is_empty() => None,
        Ok(line) => Some(serde_json::from_str::<Value>(&line).map_err(anyhow::Error::from)),
        Err(e) => Some(Err(anyhow::Error::from(e))),
    });
    Ok(lines)
}

#[cfg(test)]
#[path = "util_tests.rs"]
mod tests;
