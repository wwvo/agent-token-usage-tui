# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1] — 2026-04-19

CI hotfix release. No functional changes beyond the pipeline repairs
needed to actually ship `v0.2.0`'s artifacts.

### Fixed

- **cnb release package stage**: `set -euo pipefail` is a bash-ism; the
  cnb runner's default shell is dash, which rejected it with
  `sh: 1: set: Illegal option -o pipefail`. Both arch legs now use
  `set -eu` (no pipes are involved, so `pipefail` was pure dead weight).
- **clippy `collapsible_match`**: two nested `match ... => { if ... }`
  sites in `src/collector/claude.rs` and `src/collector/openclaw.rs`
  now use match arm guards. The lint is older than `v0.1.0` but the
  cnb runner's stable clippy floor raised past where we'd been lint-
  testing locally, so this is the same code surface, new lint version.

### Known limitations

- The `v0.2.0` tag remains on `origin` as a historical no-artifact
  release. `v0.2.1` is the first usable tarball release of the
  `v0.2.x` line.

## [0.2.0] — 2026-04-19

Windsurf support + TUI polish release.

### Added

- **Windsurf VSCode exporter** under `tools/windsurf-exporter/`:
  a thin companion extension that captures Cascade trajectories from the
  in-process Language Server (CSRF + `devClient()` interception) and
  writes append-only JSONL to `~/.atut/windsurf-sessions/`. Status bar
  shows per-tick counters; manual refresh via
  `ATUT: Export Windsurf Cascade trajectories now`.
- **Windsurf collector**: the Rust side now ingests the exporter's JSONL
  files into SQLite like any other agent. Source-scoped offset resume,
  malformed-line skip, missing-timestamp drop, per-turn model fallback
  to the session's `last_model` — all covered by 7 new fixture tests.
- **Sessions scrollbar + PageUp/PageDown** navigation; Sessions view cap
  raised from 200 to 2,000 rows (still safely under 1 ms per query).
- **Models → Sessions drill-down**: press `Enter` on Models to filter
  Sessions to every session that ever used the highlighted model, with
  totals scoped to that model.
- **`NO_COLOR` support**: honors the [no-color.org](https://no-color.org/)
  convention; `Modifier::BOLD` / reverse stay intact so selection is
  still visible on monochrome terminals.
- **Panic hook**: disables raw mode and leaves the alt screen before
  the default panic hook prints a backtrace — crashes inside the TUI
  no longer leave the user's shell broken.
- **`justfile`** with common recipes (`fmt`, `clippy`, `test`, `run`,
  `release`, `ci`). README now has a **Development** section pointing
  at it.
- **CLI `--help`**: top-level `long_about` plus per-subcommand
  `long_about` with copy-pasteable example blocks.
- **cnb.cool CI** (`.cnb.yml`): push/PR pipelines run
  `fmt-check + clippy + test` on amd64 + arm64; tag pushes produce
  stripped Linux tarballs (`atut-<tag>-<arch>-linux.tar.gz`) plus
  SHA-256 checksums.
- **Apache-2.0 `LICENSE`** file at the repo root (the Cargo manifest
  already declared this license; the file itself was missing).

### Changed

- `on_key` now takes a `page_size: usize` parameter so PageUp/PageDown
  can respect the actual terminal height. No behavior change for other
  keys.
- `run_tui` wires the stderr-based `StartupReporter` (progress checklist
  replaces the previous blank pre-TUI screen). Other CLI subcommands
  still use `NoopReporter`.

### Fixed

- README `Phase 2` / `CHANGELOG` paragraph had residual text corruption
  from an earlier edit (`todahangelog`, a truncated `## Cy)` heading).
  Cleaned up and `CHANGELOG` link moved to its own `## Changelog` section.

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

- **Windsurf** sessions required a companion VSCode extension to capture;
  the initial `v0.1.0` release shipped a stub collector. See the
  `[0.2.0]` Windsurf entries above for the follow-up that closes this gap.
- No scrolling / pagination yet on the Sessions view — it shows the 200
  most recent only. (Addressed in `[0.2.0]`: scrollbar + 2,000-row cap +
  PageUp/PageDown.)
- No per-model drill-down from the Models view; only Overview → Sessions
  filtering is wired up. (Addressed in `[0.2.0]`.)

[Unreleased]: https://cnb.cool/prevailna/agent-token-usage-tui/-/compare/v0.2.1...HEAD
[0.2.1]: https://cnb.cool/prevailna/agent-token-usage-tui/-/compare/v0.2.0...v0.2.1
[0.2.0]: https://cnb.cool/prevailna/agent-token-usage-tui/-/compare/v0.1.0...v0.2.0
[0.1.0]: https://cnb.cool/prevailna/agent-token-usage-tui/-/tags/v0.1.0
