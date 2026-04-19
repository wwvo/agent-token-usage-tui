// Agent Token Usage — Windsurf Exporter
//
// Thin companion extension that ships alongside the `agent-token-usage-tui`
// Rust CLI. Its only job is to pull Cascade trajectories out of the
// Windsurf in-process Language Server (the data isn't on disk otherwise)
// and write them to JSONL files the Rust collector can ingest.
//
// This scaffold lands empty on purpose — D1 creates the shape, D2 wires
// CSRF + RPC, D3 wires the writer, D4 wires activate + polling. Keeping
// each commit narrow makes reviews tractable.

import * as vscode from "vscode";

/**
 * VSCode activation hook.
 *
 * Skips immediately unless running inside Windsurf itself — the Language
 * Server endpoints we rely on don't exist in stock VSCode, so activating
 * anywhere else would only surface confusing errors. Keeping the guard
 * here (instead of `activationEvents`) lets the same `.vsix` install in
 * both editors without needing a Windsurf-only marketplace.
 */
export function activate(_context: vscode.ExtensionContext): void {
  if (!vscode.env.appName.toLowerCase().includes("windsurf")) {
    // Silent no-op: most users will install this alongside `atut`
    // without knowing it's Windsurf-specific.
    return;
  }
  // D4 will wire polling + status bar here.
}

/**
 * VSCode deactivation hook.
 *
 * Intentionally empty until D4 introduces an interval timer we need to
 * tear down cleanly; the scaffold owns no handles to release.
 */
export function deactivate(): void {
  // placeholder
}
