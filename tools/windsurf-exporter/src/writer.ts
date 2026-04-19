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
// 2. Every subsequent line is a `turn_usage` object whose `step_id` is a
//    *synthetic* `"turn-<N>"` counter (N = position among successfully-
//    extracted user_input turns within this cascade, starting at 0).
//    Windsurf's Cascade step objects don't expose a stable per-step id
//    (we learned this the hard way; see commit log for `step.id`), so we
//    dedup off the cascade-local turn index instead. Cascade trajectories
//    are append-only histories — the Nth user_input step in this refresh
//    is always the same physical turn as the Nth on every future refresh
//    — so synthetic ids align perfectly with on-disk rows.
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

    // `stepIds.size` = the number of `turn_usage` lines already on disk
    // for this cascade. Each line carries a synthetic `turn-<N>` id
    // assigned in strictly increasing order, so the on-disk set is
    // exactly `{turn-0, turn-1, …, turn-(N-1)}`. We use the size as
    // the "how many valid turns have we already flushed?" counter and
    // skip that many before appending.
    let validTurnsSoFar = 0;
    for (const step of steps) {
        const partial = extractTurnUsage(step, summary);
        if (!partial) {
            stats.turnsIgnored++;
            continue;
        }
        // Previous refreshes already wrote the first `stepIds.size` valid
        // turns; walk past them. `validTurnsSoFar` bumps only on non-null
        // partials so ignored steps don't shift the index.
        if (validTurnsSoFar < stepIds.size) {
            validTurnsSoFar++;
            stats.turnsSkipped++;
            continue;
        }
        const syntheticId = `turn-${validTurnsSoFar}`;
        const line: TurnUsageLine = {
            type: "turn_usage",
            step_id: syntheticId,
            ...partial,
        };
        fs.appendFileSync(filePath, `${JSON.stringify(line)}\n`, "utf8");
        stepIds.add(syntheticId);
        stats.turnsInserted++;
        validTurnsSoFar++;
    }

    return stats;
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

    return {
        timestamp: step.timestamp ?? "",
        model:
            step.metadata?.requestedModelUid ??
            summary.lastGeneratorModelUid ??
            "",
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cached,
    };
}
