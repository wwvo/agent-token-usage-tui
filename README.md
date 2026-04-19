# agent-token-usage-tui

> Cross-platform terminal UI that aggregates **Claude Code / Codex / OpenClaw / OpenCode** sessions and reports token usage and cost.
>
> **Status:** `0.1.0-alpha` — M1 skeleton only. Tracking full milestones in `plans/`.

A pure TUI (no web UI, no dashboard server). Data, config, logs, and pricing cache all live **next to the executable** — drop it anywhere and it works. Pricing comes from the litellm catalog and is embedded as a compile-time fallback so everything works offline.

## Why this exists

Most AI coding agents write session data locally but none of them talk to each other. If you bounce between Claude Code, Codex, OpenClaw, and OpenCode across projects, the only way to answer "how much did I burn this week?" is to tally four separate logs by hand.

This tool reads all of them, calculates cost against the litellm model catalog, and puts a k9s-style overview one terminal away:

* totals (cost / tokens / sessions / API calls)
* cost broken down by model
* 7-day trend sparkline
* drill-down by session, model, or time window

## Supported agents

| Agent | Data source | Phase |
|---|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` | M3 (MVP) |
| Codex CLI | `~/.codex/sessions/**/*.jsonl` | M3 (MVP) |
| OpenClaw | `<base>/<agent>/sessions/*.jsonl` | M4 (MVP) |
| OpenCode | local SQLite (read-only) | M4 (MVP) |
| Windsurf / Cascade | *not persisted to disk* — see Phase 2 below | Phase 2 |

## Install / build

Requires Rust 1.85+ (`edition = "2024"`). Stable toolchain.

```bash
git clone https://github.com/briqt/agent-token-usage-tui
cd agent-token-usage-tui
cargo build --release
# binary at target/release/agent-token-usage-tui
```

The release profile strips symbols + LTO => single `~15 MB` executable.

## Quick start

Currently (M1 skeleton) only `version` is implemented; other subcommands print a descriptive `todo!` panic pointing at the commit that will implement them:

```text
agent-token-usage-tui                # M5 C6 → enter TUI
agent-token-usage-tui scan           # M4 C5 → one-shot scan for cron / CI
agent-token-usage-tui sync-prices    # M4 C5 → refresh pricing cache only
agent-token-usage-tui version        # works today
```

## Portable layout

Everything is written next to the executable:

```
agent-token-usage-tui.exe
config.toml                 # optional — defaults work if absent
data.db                     # SQLite (WAL mode)
log/
  2026-04-19.log            # daily rolling (M7 C1)
pricing.json                # last successful litellm sync
```

No writes to `%APPDATA%` / `~/.config` / `~/.local/share`. Drop the exe on a thumb drive and it runs from there.

## Phase 2: Windsurf support

Windsurf doesn't persist Cascade trajectories to disk — they live in the Language Server's memory and are only reachable through a CSRF-gated local gRPC endpoint. That's incompatible with a pure Rust CLI.

The Phase 2 plan is a **thin companion VSCode extension** that polls `GetAllCascadeTrajectories` every ~60s and writes JSONL files to `<exe-dir>/windsurf-sessions/*.jsonl`. `collector::windsurf` then reads those JSONL files like any other agent. See `plans/agent-token-usage-tui-architecture-77d40b.md` §13 for the design.

## Contributing

Start with [`AGENTS.md`](./AGENTS.md) — it covers the code style, commit conventions, file-size limits, Clippy policy, and testing rules used throughout the code base.

## License

Apache-2.0
