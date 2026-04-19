// Incremental JSONL writer for Windsurf Cascade trajectories.
//
// One file per cascade, one object per line. The Rust collector reads
// these files append-only with an offset marker so rewriting / reordering
// is not allowed. The invariants:
//
// 1. The *first* line of a non-empty file is a `session_meta` object; it
//    is written exactly once per cascade and never mutated afterward.
//    Summaries change over time (users rename, LS updates `stepCount`),
//    but the Rust side treats the first-seen values as canonical — any
//    drift is absorbed into the dashboard's best-effort UX, not the
//    schema.
// 2. Every subsequent line is a `turn_usage` object. Its `step_id` is
//    either the server-assigned UUID from `step.metadata.executionId`
//    (preferred for files newly created on ≥ v0.2.10) or the synthetic
//    `"turn-<N>"` counter used by v0.2.9 and earlier (N = position
//    among successfully-extracted user_input turns within this cascade,
//    starting at 0). `writeSession` picks one style *per file*: if the
//    file already has synthetic ids on disk we keep appending synthetic
//    ones so a mid-life exporter upgrade doesn't double-write rows the
//    Rust collector has already ingested. Either id is stable across
//    refreshes, so the collector's `(cascade_id, step_id)` dedup
//    behaves identically.
// 3. Writes are append-only with a trailing `\n`. Partial-write recovery
//    is left to the OS — a crash mid-write leaves at most one mangled
//    trailing line that the collector ignores (`serde_json::from_str`
//    errors on that line only).
//
// The shapes here are the *wire* format. If we change them, Rust's
// `collector::windsurf` needs to change in lockstep — the doc comments
// below are the contract.

import * as fs from "fs";
import * as os from "os";
import * as path from "path";

import type { CascadeStep, TrajectorySummary } from "./types";

/** Environment variable override for the output directory. */
const ENV_DIR = "ATUT_WINDSURF_SESSIONS_DIR";

// ---- Wire format ----------------------------------------------------------

/**
 * First line of each file. Exactly one per cascade; never mutated.
 *
 * Field order is preserved deliberately: JSON object ordering isn't
 * semantic per the spec, but the Rust side logs raw lines on parse error
 * and a stable shape is friendlier to grep / diff.
 */
export interface SessionMetaLine {
    type: "session_meta";
    cascade_id: string;
    created_time: string;
    summary: string;
    last_model: string;
    workspace: string;
}

/** Subsequent lines. One per Cascade step of `type=CORTEX_STEP_TYPE_USER_INPUT`. */
export interface TurnUsageLine {
    type: "turn_usage";
    step_id: string;
    timestamp: string;
    model: string;
    input_tokens: number;
    output_tokens: number;
    cached_input_tokens: number;
}

/** Bookkeeping returned from `writeSession` so the extension can log progress. */
export interface WriteStats {
    /** `true` iff we appended the `session_meta` line in this call. */
    sessionMetaWritten: boolean;
    /** Count of `turn_usage` lines appended this call (new `step_id`s). */
    turnsInserted: number;
    /** Count of input steps that were already on disk (by `step_id`). */
    turnsSkipped: number;
    /** Count of input steps that had no usage / wrong type; never written. */
    turnsIgnored: number;
}

// ---- Directory resolution ------------------------------------------------

/**
 * Resolve the sessions directory, honoring `ATUT_WINDSURF_SESSIONS_DIR`.
 *
 * Falls back to `~/.atut/windsurf-sessions/`. We intentionally don't
 * land next to the Windsurf extension itself because its install path
 * is unstable (`.vscode-server` hashes, per-version directories).
 * `~/.atut` is predictable and the Rust TUI already treats it as a
 * default scan root via `WindsurfConfig.bases`.
 */
export function defaultSessionsDir(): string {
    const override = process.env[ENV_DIR];
    if (override && override.trim()) {
        return override;
    }
    return path.join(os.homedir(), ".atut", "windsurf-sessions");
}

/** `<dir>/<cascadeId>.jsonl`, with `cascadeId` lightly sanitized. */
export function sessionFilePath(dir: string, cascadeId: string): string {
    // Defensive: cascadeIds are UUIDs in practice, but we still strip
    // anything that could escape the target directory. Windows + POSIX
    // agree that path separators + null byte are forbidden.
    const safe = cascadeId.replace(/[\\/\0]/g, "_");
    return path.join(dir, `${safe}.jsonl`);
}

// ---- Existing-file read --------------------------------------------------

/**
 * Read an existing file and collect every `step_id` plus whether the
 * file already has a `session_meta` line.
 *
 * Missing file → empty state (the writer will create everything).
 * Malformed JSON lines → treated as absent; we assume a future run will
 * overwrite the corrupt line with a correct append (best-effort).
 */
export function loadExistingState(filePath: string): {
    stepIds: Set<string>;
    hasSessionMeta: boolean;
} {
    const stepIds = new Set<string>();
    let hasSessionMeta = false;
    let raw: string;
    try {
        raw = fs.readFileSync(filePath, "utf8");
    } catch (err) {
        if ((err as NodeJS.ErrnoException).code === "ENOENT") {
            return { stepIds, hasSessionMeta };
        }
        throw err;
    }
    for (const line of raw.split("\n")) {
        const trimmed = line.trim();
        if (!trimmed) {
            continue;
        }
        let parsed: unknown;
        try {
            parsed = JSON.parse(trimmed);
        } catch {
            continue;
        }
        if (typeof parsed !== "object" || parsed === null) {
            continue;
        }
        const obj = parsed as Record<string, unknown>;
        if (obj.type === "session_meta") {
            hasSessionMeta = true;
        } else if (
            obj.type === "turn_usage" &&
            typeof obj.step_id === "string"
        ) {
            stepIds.add(obj.step_id);
        }
    }
    return { stepIds, hasSessionMeta };
}

// ---- Append ---------------------------------------------------------------

/**
 * Reconcile a cascade's current steps with its on-disk file.
 *
 * The function is idempotent: calling it twice with the same inputs
 * yields `{ sessionMetaWritten: false, turnsInserted: 0, ... }` on the
 * second call. Only genuinely new `step_id`s produce I/O.
 *
 * We synchronously `appendFileSync` so a crash mid-loop leaves the file
 * in a consistent state (each line is an atomic append, not a batch).
 * The cost is minor — we never process more than a few hundred steps
 * per cascade.
 */
export function writeSession(
    dir: string,
    cascadeId: string,
    summary: TrajectorySummary,
    steps: readonly CascadeStep[],
): WriteStats {
    fs.mkdirSync(dir, { recursive: true });
    const filePath = sessionFilePath(dir, cascadeId);
    const { stepIds, hasSessionMeta } = loadExistingState(filePath);

    const stats: WriteStats = {
        sessionMetaWritten: false,
        turnsInserted: 0,
        turnsSkipped: 0,
        turnsIgnored: 0,
    };

    if (!hasSessionMeta) {
        const meta: SessionMetaLine = {
            type: "session_meta",
            cascade_id: cascadeId,
            created_time: summary.createdTime ?? "",
            summary: summary.summary ?? "",
            last_model: summary.lastGeneratorModelUid ?? "",
            workspace:
                summary.workspaces?.[0]?.workspaceFolderAbsoluteUri ?? "",
        };
        fs.appendFileSync(filePath, `${JSON.stringify(meta)}\n`, "utf8");
        stats.sessionMetaWritten = true;
    }

    // Pick an id style per file, not per run:
    // - `synthetic` — the file already has `turn-<N>` rows from a
    //   pre-v0.2.10 exporter. Keep appending the same shape so the
    //   Rust collector's `(cascade_id, step_id)` dedup doesn't see
    //   every old row "migrate" into a duplicate new UUID row.
    // - `uuid` — file is brand new, or every existing row is already
    //   a UUID. Prefer `step.metadata.executionId`. If a particular
    //   step happens to be missing one (shouldn't happen in practice,
    //   but we don't crash on it), fall back to synthetic for that row
    //   only; mixing is safe since both ids are unique.
    const idStyle = detectIdStyle(stepIds);

    let validTurnsSoFar = 0;
    for (const step of steps) {
        const partial = extractTurnUsage(step, summary);
        if (!partial) {
            stats.turnsIgnored++;
            continue;
        }
        const syntheticId = `turn-${validTurnsSoFar}`;
        const uuidId = step.metadata?.executionId;
        const chosenId =
            idStyle === "synthetic" ? syntheticId : (uuidId ?? syntheticId);
        // Idempotent: if this id is already on disk, skip. Works whether
        // the id is synthetic (v0.2.9 carry-over) or UUID (v0.2.10+).
        if (stepIds.has(chosenId)) {
            stats.turnsSkipped++;
            validTurnsSoFar++;
            continue;
        }
        const line: TurnUsageLine = {
            type: "turn_usage",
            step_id: chosenId,
            ...partial,
        };
        fs.appendFileSync(filePath, `${JSON.stringify(line)}\n`, "utf8");
        stepIds.add(chosenId);
        stats.turnsInserted++;
        validTurnsSoFar++;
    }

    return stats;
}

/**
 * Decide whether to keep writing synthetic `turn-<N>` ids (because the
 * file was started by a pre-v0.2.10 exporter) or switch to UUID ids.
 *
 * Iterates `stepIds` once, short-circuits on the first synthetic match.
 * An empty set — i.e. a brand-new file — is treated as "uuid" so new
 * files pick up the cleaner id style immediately.
 */
function detectIdStyle(stepIds: Set<string>): "synthetic" | "uuid" {
    for (const id of stepIds) {
        if (/^turn-\d+$/.test(id)) {
            return "synthetic";
        }
    }
    return "uuid";
}

// ---- Internal: step → TurnUsageLine --------------------------------------

/** Subset of `TurnUsageLine` that `extractTurnUsage` can fill in. */
type TurnUsagePayload = Omit<TurnUsageLine, "type" | "step_id">;

/**
 * Pull a `turn_usage` payload out of a Cascade step, or `null` if the
 * step isn't a user-input turn with usable metrics.
 *
 * The filter matches the reference implementation in
 * `windsurf-token-usage/src/api.ts`:
 *   - `step.type === "CORTEX_STEP_TYPE_USER_INPUT"`
 *   - A `responseDimensionGroups` entry titled exactly `"Token Usage"`
 *     with `uid` ∈ { input_tokens, output_tokens, cached_input_tokens }
 *
 * The returned payload omits `type` + `step_id`: the caller
 * (`writeSession`) synthesizes the `turn-<N>` id from the cascade-local
 * turn position, so we don't need anything stable-and-unique from the
 * step itself.
 */
function extractTurnUsage(
    step: CascadeStep,
    summary: TrajectorySummary,
): TurnUsagePayload | null {
    if (step.type !== "CORTEX_STEP_TYPE_USER_INPUT") {
        return null;
    }

    const groups = step.metadata?.responseDimensionGroups ?? [];
    let input = 0;
    let output = 0;
    let cached = 0;
    let found = false;
    for (const group of groups) {
        if (group.title !== "Token Usage") {
            continue;
        }
        for (const dim of group.dimensions ?? []) {
            const value = dim.cumulativeMetric?.value ?? 0;
            switch (dim.uid) {
                case "input_tokens":
                    input = value;
                    found = true;
                    break;
                case "output_tokens":
                    output = value;
                    found = true;
                    break;
                case "cached_input_tokens":
                    cached = value;
                    found = true;
                    break;
                default:
                    // Ignore unknown uids; their semantics aren't part of our wire
                    // contract and we don't want to accidentally surface e.g.
                    // `reasoning_tokens` until the Rust side is ready for it.
                    break;
            }
        }
    }

    if (!found) {
        return null;
    }

    // Timestamp resolution, preferred → fallback:
    // 1. `step.metadata.createdAt` — per-step, nanosecond precision,
    //    present on every production step observed during the v0.2.9
    //    retention probe. This is what lets the TUI's Trend view land
    //    each turn in its own hour bucket instead of bunching every turn
    //    in a cascade at `lastUserInputTime`.
    // 2. `step.timestamp` — unused in production (always `""`); kept
    //    because it's cheap and future-proofs against a field rename.
    // 3. cascade-level times — per-cascade granularity, good enough to
    //    keep the row alive through the Rust collector's RFC-3339 check
    //    (`windsurf.rs::parse_entry` drops rows with empty timestamps).
    //
    // `||` (not `??`) on purpose: these fields show up as `""` in
    // practice, not `undefined`. `??` would treat `""` as a real value
    // and skip the fallback.
    const timestamp =
        step.metadata?.createdAt ||
        step.timestamp ||
        summary.lastUserInputTime ||
        summary.lastModifiedTime ||
        summary.createdTime ||
        "";

    return {
        timestamp,
        model:
            step.metadata?.requestedModelUid ??
            summary.lastGeneratorModelUid ??
            "",
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cached,
    };
}
