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
// 2. Every subsequent line is a `turn_usage` object keyed by `step_id`.
//    Re-ingesting the same step is a no-op: we read the file, gather
//    the existing `step_id` set, and skip duplicates before appending.
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
    } else if (obj.type === "turn_usage" && typeof obj.step_id === "string") {
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
      workspace: summary.workspaces?.[0]?.workspaceFolderAbsoluteUri ?? "",
    };
    fs.appendFileSync(filePath, `${JSON.stringify(meta)}\n`, "utf8");
    stats.sessionMetaWritten = true;
  }

  for (const step of steps) {
    const usage = extractTurnUsage(step, summary);
    if (!usage) {
      stats.turnsIgnored++;
      continue;
    }
    if (stepIds.has(usage.step_id)) {
      stats.turnsSkipped++;
      continue;
    }
    fs.appendFileSync(filePath, `${JSON.stringify(usage)}\n`, "utf8");
    stepIds.add(usage.step_id);
    stats.turnsInserted++;
  }

  return stats;
}

// ---- Internal: step → TurnUsageLine --------------------------------------

/**
 * Pull a `turn_usage` row out of a Cascade step, or `null` if the step
 * isn't a user-input turn with usable metrics.
 *
 * The filter matches the reference implementation in
 * `windsurf-token-usage/src/api.ts`:
 *   - `step.type === "CORTEX_STEP_TYPE_USER_INPUT"`
 *   - A `responseDimensionGroups` entry titled exactly `"Token Usage"`
 *     with `uid` ∈ { input_tokens, output_tokens, cached_input_tokens }
 *
 * Missing `step.id` is fatal for our dedup story; we return `null` so
 * the caller counts it as "ignored" rather than silently appending a
 * row that will later duplicate.
 */
function extractTurnUsage(
  step: CascadeStep,
  summary: TrajectorySummary,
): TurnUsageLine | null {
  if (step.type !== "CORTEX_STEP_TYPE_USER_INPUT") {
    return null;
  }
  if (!step.id) {
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
    type: "turn_usage",
    step_id: step.id,
    timestamp: step.timestamp ?? "",
    model: step.metadata?.requestedModelUid ?? summary.lastGeneratorModelUid ?? "",
    input_tokens: input,
    output_tokens: output,
    cached_input_tokens: cached,
  };
}
