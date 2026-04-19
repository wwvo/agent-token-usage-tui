//! `agent-token-usage-tui` library surface.
//!
//! `main.rs` is a thin binary entry point; all logic lives here so it can be
//! exercised by integration tests and reused in future tooling (e.g. the
//! companion Windsurf exporter VSCode extension planned for Phase 2).

pub mod app_dir;
pub mod cli;
pub mod collector;
pub mod config;
pub mod domain;
pub mod logging;
pub mod pricing;
pub mod storage;
pub mod tui;
