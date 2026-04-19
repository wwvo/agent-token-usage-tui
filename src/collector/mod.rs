//! Session file collectors for each supported agent.
//!
//! Populated in M3–M4:
//!
//! | Phase | Collector |
//! |-------|-----------|
//! | M3 C3 | Claude Code (`~/.claude/projects/**/*.jsonl`) |
//! | M3 C5 | Codex (`~/.codex/sessions/**/*.jsonl`) |
//! | M4 C1 | OpenClaw (`<base>/<agent>/sessions/*.jsonl`) |
//! | M4 C2 | OpenCode (SQLite read-only) |
//! | M4 C3 | Windsurf (placeholder; real exporter lands in Phase 2) |
