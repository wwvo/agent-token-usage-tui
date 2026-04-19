# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — 2026-04-19

First public release. Everything below is new.

### Added

- **Collectors** for four AI coding agents with deterministic scan order and
  incremental checkpointing:
  - Claude Code (`~/.claude/projects/**/*.jsonl`), auto-detected.
  - Codex CLI (`~/.codex/sessions/**/*.jsonl`), auto-detected, with
    non-overlapping `input_tokens` correction (cache read/creation already
    counted in Codex's raw `input_tokens`).
  - OpenClaw (`<base>/<agent>/sessions/*.jsonl`), two-level directory walk,
    `delivery-mirror` double-billing filter.
  - OpenCode (local SQLite, read-only), watermark-column incremental scan.
  - Windsurf / Cascade — placeholder collector; real exporter lands in
    Phase 2 (companion VSCode extension).
- **Pricing cascade** against the
  [litellm catalog](https://github.com/BerriAI/litellm): freshness check →
  GitHub sync → embedded fallback snapshot. `build.rs` refreshes the
  fallback on every `cargo build`; `AGENT_TUI_DISABLE_LITELLM_DOWNLOAD=1`
  forces offline-only builds.
- **SQLite storage** with WAL journaling, idempotent migrations, fuzzy model
  matching for cost recalculation, and dedup indices that make "rescan
  everything" idempotent.
- **Config** via an optional `config.toml` next to the executable:
  `openclaw_bases`, `opencode_dbs`, `windsurf_bases`. Unknown keys are
  rejected loudly (`deny_unknown_fields`); missing file uses defaults.
- **CLI subcommands**: `scan`, `sync-prices`, `version`.
  `version` emits commit SHA, build date, and MSRV for bug reports.
- **TUI** (default `atut` entry point), four views:
  - **Overview** — per-source totals (records, prompts, sessions, tokens,
    cost, last activity).
  - **Sessions** — 200 most recent sessions, newest first.
  - **Models** — per-model rollup sorted by cost descending.
  - **Trend** — 7-day sparkline + daily detail table.
- **Key bindings** (k9s-style): `q/Esc`, `1/2/3/4`, `j/k/g/G`, `Home/End`,
  `Enter` (drill into Sessions from Overview), `r` (refresh).
- **Startup progress UI** — stderr-based `[  ]` / `[OK]` / `[!!]` checklist
  for pricing sync + each collector, replacing the pre-TUI blank screen.
- **Portable layout** — `data.db`, `config.toml`, `log/`, `pricing.json`
  all live next to the executable. No writes to `%APPDATA%`, `~/.config`,
  or `~/.local/share`.
- **Logging** — daily rolling files at `log/atut.log.YYYY-MM-DD` for TUI
  mode; direct stderr for CLI subcommands. Non-blocking background writer.
- **Release profile** — fat LTO + `codegen-units = 1` + `panic = abort`,
  plus `strip = symbols`. Opt-in `dist` profile inherits from `release`
  and turns off debug-assertions / overflow-checks for final binary shrink.

### Infrastructure

- **CI** (`.github/workflows/ci.yml`) — rustfmt / clippy (`-D warnings`) /
  test on Ubuntu + Windows + macOS, plus a `cargo doc` gate on broken
  intra-doc links.
- **188 tests** (167 unit + 21 integration) covering every collector with
  real JSONL / SQLite fixtures and full CLI-to-DB round trips.
- **Strict clippy lint floor**: `unwrap_used`, `expect_used`,
  `print_stdout`, `print_stderr`, `clone_on_ref_ptr`, `dbg_macro`, and
  style denials all turned on project-wide.

### Known limitations

- **Windsurf** sessions are not yet captured; the Phase 2 VSCode exporter
  is the design landing point (see
  `plans/agent-token-usage-tui-architecture-77d40b.md` §13).
- No scrolling / pagination yet on the Sessions view — it shows the 200
  most recent only.
- No per-model drill-down from the Models view; only Overview → Sessions
  filtering is wired up.

[Unreleased]: https://cnb.cool/prevailna/agent-token-usage-tui/-/compare/v0.1.0...HEAD
[0.1.0]: https://cnb.cool/prevailna/agent-token-usage-tui/-/tags/v0.1.0
