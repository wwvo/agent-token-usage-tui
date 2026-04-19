# agent-token-usage-tui

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)

> Cross-platform terminal UI that aggregates **Claude Code / Codex / OpenClaw / OpenCode** sessions and reports token usage and cost.

A pure TUI — no web dashboard, no background daemon. Data, config, logs, and
pricing cache all live **next to the executable**, so you can drop it on a
thumb drive and it runs from there. Pricing comes from the litellm catalog,
refreshed at build time and embedded as a compile-time fallback so
everything works offline.

## Features

- k9s-style keyboard-driven UI with four views: **Overview** (by source) /
  **Sessions** (recent activity) / **Models** (cost per model) /
  **Trend** (7-day cost + tokens sparkline).
- Incremental scans (file-state checkpoint in SQLite) — rescans don't
  re-process millions of lines.
- Token normalization: Codex's overlapping input/cache semantics is
  corrected; OpenClaw's `delivery-mirror` double-billing filtered;
  `<synthetic>` Claude rows excluded; zero-token OpenCode failures
  filtered.
- Offline-first pricing cascade: freshness check → litellm GitHub sync →
  embedded fallback snapshot (build.rs refreshes the snapshot on every
  `cargo build`).
- Portable storage: `data.db` + `config.toml` + `log/` + `pricing.json`
  all live next to the binary.

## Supported agents

| Agent              | Data source                       | Notes                                                     |
| ------------------ | --------------------------------- | --------------------------------------------------------- |
| Claude Code        | `~/.claude/projects/**/*.jsonl`   | auto-detected                                             |
| Codex CLI          | `~/.codex/sessions/**/*.jsonl`    | auto-detected; `input_tokens` corrected for cache overlap |
| OpenClaw           | `<base>/<agent>/sessions/*.jsonl` | configure `openclaw_bases` in `config.toml`               |
| OpenCode           | local SQLite                      | configure `opencode_dbs` in `config.toml`                 |
| Windsurf / Cascade | _not persisted to disk_           | placeholder; real exporter ships in Phase 2 (see below)   |

## Install / build

Requires Rust 1.85+ (`edition = "2024"`), stable toolchain.

```bash
git clone https://github.com/briqt/agent-token-usage-tui
cd agent-token-usage-tui
cargo build --release
# binary at target/release/agent-token-usage-tui
```

The release profile strips symbols + LTO for a single-binary distribution.

## Quick start

```text
agent-token-usage-tui                # launch the TUI (default)
agent-token-usage-tui scan           # one-shot scan for cron / CI
agent-token-usage-tui sync-prices    # refresh pricing cache + recompute costs
agent-token-usage-tui version        # print commit, build date, MSRV
```

Common flags (all commands):

| Flag               | Meaning                                                       |
| ------------------ | ------------------------------------------------------------- |
| `--config <path>`  | Override `config.toml` location                               |
| `--data-dir <dir>` | Override portable directory (tests / sandboxed runs)          |
| `--no-scan`        | Skip the startup scan (TUI only)                              |
| `--no-prices`      | Skip the startup pricing sync (TUI only)                      |
| `-v` / `-vv`       | Raise log verbosity (`debug` / `trace`); overrides `RUST_LOG` |

### TUI key bindings

| Key               | Action                                                                 |
| ----------------- | ---------------------------------------------------------------------- |
| `q` / `Esc` / `4` | Quit / Trend                                                           |
| `1` / `2` / `3`   | Switch to Overview / Sessions / Models                                 |
| `j` / `↓`         | Move selection down                                                    |
| `k` / `↑`         | Move selection up                                                      |
| `g` / `Home`      | Jump to top                                                            |
| `G` / `End`       | Jump to bottom                                                         |
| `Enter`           | Drill into Sessions filtered by the highlighted source (Overview only) |
| `r`               | Refresh data from the DB                                               |

## Configuration

`config.toml` is optional; when missing, Claude / Codex still work via
`$HOME`-derived defaults. Only agents whose storage paths can't be
auto-detected appear in the schema:

```toml
# config.toml — every field optional.
openclaw_bases = ["/home/u/.local/share/openclaw"]
opencode_dbs   = ["/home/u/.local/share/opencode/opencode.db"]
windsurf_bases = []  # reserved for the Phase 2 VSCode exporter
```

Unknown keys are rejected loudly (typos in key names won't be silently
ignored).

## Portable layout

Everything lives next to the executable:

```
agent-token-usage-tui.exe
config.toml                 # optional — defaults work if absent
data.db                     # SQLite (WAL mode)
log/
  atut.log.2026-04-19       # daily rolling (TUI mode)
pricing.json                # last successful litellm sync
```

No writes to `%APPDATA%` / `~/.config` / `~/.local/share`.

## Phase 2: Windsurf support

Windsurf doesn't persist Cascade trajectories to disk — they live in the
Language Server's memory and are only reachable through a CSRF-gated local
gRPC endpoint. That's incompatible with a pure Rust CLI.

The Phase 2 plan is a **thin companion VSCode extension** that polls
`GetAllCascadeTrajectories` every ~60s and writes JSONL files to
`<exe-dir>/windsurf-sessions/*.jsonl`. `collector::windsurf` (placeholder
todahangelog

See [`CHANGELOG.md`](./CHANGELOG.md) for the release history. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Cy) will then read those files like any other agent. See

`plans/agent-token-usage-tui-architecture-77d40b.md` §13 for the design.

## Contributing

Start with [`AGENTS.md`](./AGENTS.md) — it covers code style, commit
conventions, file-size limits, Clippy policy, and testing rules used
throughout the code base.

## License

Apache-2.0
