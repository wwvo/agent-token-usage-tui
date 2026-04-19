# Agent Token Usage · Windsurf Exporter

Companion VSCode extension for [`agent-token-usage-tui`](../../README.md).

## Why this exists

Windsurf's Cascade trajectories — the token usage history the TUI wants
to show — never touch disk. They live inside the Windsurf Language
Server's memory and are only reachable through a CSRF-gated local gRPC
endpoint exposed to extensions running in the same process.

A pure Rust CLI can't reach that endpoint. This extension does the
privileged read-side and writes JSONL files that the Rust collector
ingests like any other agent's on-disk sessions.

## Scope

- **Write-only.** This extension does not render any UI. All dashboards
  live in the Rust TUI.
- **Windsurf-only.** The extension no-ops immediately in stock VSCode
  (`vscode.env.appName` check).
- **Offline-safe.** No network calls; everything stays on `localhost`.

## Layout

```
tools/windsurf-exporter/
├── package.json        # extension manifest (publisher, activation events, commands)
├── tsconfig.json       # strict ES2022 + CommonJS (VSCode host loads via require)
├── .vscodeignore       # keep source + sourcemaps out of the .vsix
├── .gitignore          # node_modules/ out/ *.vsix
├── src/
│   └── extension.ts    # activate/deactivate (scaffold today; D4 adds polling)
└── README.md           # this file
```

## Build

```bash
cd tools/windsurf-exporter
npm install
npm run compile     # tsc → out/extension.js
npm run package     # .vsix via @vscode/vsce (no runtime deps to bundle)
```

Install the `.vsix` in Windsurf via **Extensions → … → Install from VSIX**.

## What lands in which commit

| Commit | Scope |
|--------|-------|
| D1 | Empty scaffold (this commit) |
| D2 | `src/api.ts` — CSRF + devClient + RPC helpers |
| D3 | `src/writer.ts` — incremental JSONL writer |
| D4 | `src/extension.ts` fills in activation polling + status bar |
| D5 | Rust side: `src/collector/windsurf.rs` reads the JSONL files |
| D6 | Fixture tests for the collector |
| D7 | User-facing README section + CHANGELOG |

## Licensing

Apache-2.0, same as the parent crate. See [`../../LICENSE`](../../LICENSE).
