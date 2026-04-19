// Agent Token Usage — Windsurf Exporter
//
// Thin companion extension that ships alongside the `agent-token-usage-tui`
// Rust CLI. Its only job is to pull Cascade trajectories out of the
// Windsurf in-process Language Server (the data isn't on disk otherwise)
// and write them to JSONL files the Rust collector can ingest.

import * as vscode from "vscode";

import {
    clearCredentials,
    fetchCascadeSteps,
    getCredentials,
    listCascades,
} from "./api";
import { defaultSessionsDir, writeSession } from "./writer";

/** Delay before the first refresh after activate(). */
const INITIAL_DELAY_MS = 8_000;

/** Interval between subsequent refreshes. */
const POLL_INTERVAL_MS = 60_000;

/** Module-level guard so two refreshes never race. */
let refreshInFlight = false;

/** The status bar item; held in module scope to be updated across refresh ticks. */
let statusBar: vscode.StatusBarItem | undefined;

/**
 * VSCode activation hook.
 *
 * Skips immediately unless running inside Windsurf itself — the Language
 * Server endpoints we rely on don't exist in stock VSCode, so activating
 * anywhere else would only surface confusing errors. Keeping the guard
 * here (instead of `activationEvents`) lets the same `.vsix` install in
 * both editors without needing a Windsurf-only marketplace.
 */
export function activate(context: vscode.ExtensionContext): void {
    if (!vscode.env.appName.toLowerCase().includes("windsurf")) {
        // Silent no-op for stock VSCode / forks that don't ship the
        // `codeium.windsurf` extension.
        return;
    }

    statusBar = vscode.window.createStatusBarItem(
        vscode.StatusBarAlignment.Left,
        100,
    );
    statusBar.text = "$(pulse) atut: idle";
    statusBar.tooltip = "agent-token-usage-tui — Windsurf exporter";
    statusBar.command = "atut-windsurf-exporter.export-now";
    statusBar.show();
    context.subscriptions.push(statusBar);

    const manualTrigger = vscode.commands.registerCommand(
        "atut-windsurf-exporter.export-now",
        () => runOnce(),
    );
    context.subscriptions.push(manualTrigger);

    // Schedule the first run after the LS has had time to wake up. 8s
    // sidesteps the common cold-start window where `devClient()` is
    // still null. The handle is disposable so deactivate() clears it.
    const initialTimer = setTimeout(() => runOnce(), INITIAL_DELAY_MS);
    context.subscriptions.push({ dispose: () => clearTimeout(initialTimer) });

    const intervalTimer = setInterval(() => runOnce(), POLL_INTERVAL_MS);
    context.subscriptions.push({ dispose: () => clearInterval(intervalTimer) });
}

/**
 * VSCode deactivation hook.
 *
 * VSCode calls every `context.subscriptions` disposable before invoking
 * this function, which already tears down the status bar + timers.
 * Defined for symmetry and to keep lint rules happy about the exported
 * API surface.
 */
export function deactivate(): void {
    statusBar = undefined;
}

// ---- Internal: one refresh tick ------------------------------------------

/**
 * Drive one full refresh: creds → list cascades → per-cascade step fetch
 * → writer → status bar update.
 *
 * Guarded by `refreshInFlight` so the periodic timer + the manual
 * command + the initial kick can't pile up. The guard also keeps the
 * monkey-patch window in `api.ts::extractCsrf` from being installed
 * twice concurrently — a correctness issue, not just an efficiency one.
 */
async function runOnce(): Promise<void> {
    if (refreshInFlight) {
        return;
    }
    refreshInFlight = true;
    setStatus("$(sync~spin) atut: syncing", "Refreshing Cascade trajectories…");
    try {
        const creds = await getCredentials();
        if (!creds) {
            setStatus(
                "$(warning) atut: no creds",
                "Could not capture CSRF credentials; is Windsurf's Language Server up?",
            );
            return;
        }
        const summaries = await listCascades(creds);
        const ids = Object.keys(summaries);
        const dir = defaultSessionsDir();

        let totalInserted = 0;
        let totalSkipped = 0;
        let totalIgnored = 0;
        let totalCheckpoints = 0;
        let failedCascades = 0;

        for (const id of ids) {
            const summary = summaries[id];
            if (!summary) {
                continue;
            }
            let stepsResp;
            try {
                stepsResp = await fetchCascadeSteps(creds, id);
            } catch (err) {
                failedCascades++;
                console.warn(
                    `[atut-exporter] cascade ${id} step fetch failed:`,
                    err,
                );
                continue;
            }
            const steps = stepsResp.steps ?? [];
            const stats = writeSession(dir, id, summary, steps);
            totalInserted += stats.turnsInserted;
            totalSkipped += stats.turnsSkipped;
            totalIgnored += stats.turnsIgnored;
            totalCheckpoints += stats.checkpointsInserted;
        }

        setStatus(
            `$(pulse) atut: ${ids.length} cascades (+${totalInserted})`,
            [
                `Last refresh: ${new Date().toISOString()}`,
                `  cascades:    ${ids.length}`,
                `  inserted:    ${totalInserted}`,
                `  skipped:     ${totalSkipped} (already on disk)`,
                `  ignored:     ${totalIgnored} (non-usage steps)`,
                `  checkpoints: ${totalCheckpoints} (server cost cross-check)`,
                `  fetch err:   ${failedCascades}`,
                `  output:      ${dir}`,
            ].join("\n"),
        );
    } catch (err) {
        // Rotate creds on unrecoverable failures so the next tick tries a
        // fresh CSRF extraction instead of hitting a stale cache.
        clearCredentials();
        setStatus(
            "$(warning) atut: error",
            `Refresh failed: ${err instanceof Error ? err.message : String(err)}`,
        );
        console.error("[atut-exporter] refresh failed:", err);
    } finally {
        refreshInFlight = false;
    }
}

/** Update the status bar text + tooltip, no-op if deactivated. */
function setStatus(text: string, tooltip: string): void {
    if (!statusBar) {
        return;
    }
    statusBar.text = text;
    statusBar.tooltip = tooltip;
}
