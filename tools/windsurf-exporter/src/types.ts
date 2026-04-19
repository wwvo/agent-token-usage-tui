// Type declarations for the subset of the Windsurf Cascade RPC surface
// this exporter actually consumes. Deliberately narrower than the full
// protobuf schema — if a field isn't persisted to JSONL downstream, we
// don't type it here; `unknown` + runtime guards work better than
// duplicating a moving target.

/**
 * Credentials captured from the in-process Windsurf Language Server.
 *
 * `csrf` is the `x-codeium-csrf-token` the LS emits on every authenticated
 * request. `port` is the localhost HTTP port it listens on. Both are
 * per-process and rotate on every Windsurf restart.
 */
export interface WindsurfCredentials {
    readonly csrf: string;
    readonly port: number;
}

/** Subset of `GetAllCascadeTrajectories` response summary we care about. */
export interface TrajectorySummary {
    /** Free-form title the Cascade UI shows for this conversation. */
    summary?: string;
    /** Step count as reported by the server; used as a cheap freshness probe. */
    stepCount?: number;
    /** ISO-8601 string; we pass it through untouched to the JSONL. */
    createdTime?: string;
    /** ISO-8601 string. */
    lastModifiedTime?: string;
    /**
     * ISO-8601 string. Closest cascade-level proxy for "when did the last
     * user_input turn happen?"; used as a per-turn `timestamp` fallback
     * when the step itself doesn't expose one (always the case in
     * production Windsurf builds).
     */
    lastUserInputTime?: string;
    /** Model UID the Cascade last generated a response with. */
    lastGeneratorModelUid?: string;
    /**
     * Workspace roots this Cascade touched, in the server's order. We pick
     * the first entry's `workspaceFolderAbsoluteUri` as `project` in the
     * JSONL session meta.
     */
    workspaces?: ReadonlyArray<{
        workspaceFolderAbsoluteUri?: string;
        branchName?: string;
        repository?: { computedName?: string };
    }>;
}

/** Subset of a single step we need to compute per-turn token usage. */
export interface CascadeStep {
    /** Enum name; we only keep rows where this equals `CORTEX_STEP_TYPE_USER_INPUT`. */
    type?: string;
    /** ISO-8601 timestamp. */
    timestamp?: string;
    /** Per-step metadata carrying dimensions + requested model. */
    metadata?: CascadeStepMetadata;
}

export interface CascadeStepMetadata {
    /** Model UID the user asked for (may differ from `lastGeneratorModelUid`). */
    requestedModelUid?: string;
    /**
     * ISO-8601 step creation time, nanosecond precision in practice
     * (`2026-04-20T01:23:45.678901234Z`). Present on every step we've
     * ever seen in a production Windsurf build; still typed optional
     * because we don't want a single missing field to take the whole
     * refresh down.
     */
    createdAt?: string;
    /**
     * Server-assigned UUID per step. Stable across refreshes, so we
     * prefer it over our synthetic `turn-<N>` counter for new files.
     * Existing files stick with the counter to stay backward compatible
     * with the on-disk rows the collector has already ingested.
     */
    executionId?: string;
    /** Grouped dimension list; we only read the `Token Usage` group. */
    responseDimensionGroups?: ReadonlyArray<ResponseDimensionGroup>;
    /**
     * Windsurf's own running cost estimate on `CORTEX_STEP_TYPE_CHECKPOINT`
     * steps. Shape is left intentionally loose (`Record<string, unknown>`)
     * because the field isn't documented and the v0.2.9 retention probe
     * only confirmed its *existence*, not the exact field names inside.
     * The exporter's extractor in `writer.ts` probes for several
     * plausible layouts at runtime and bails gracefully if nothing
     * matches — so a wrong guess degrades to "no cross-check data"
     * rather than crashing the refresh.
     */
    modelCost?: Record<string, unknown>;
    /**
     * Windsurf's per-checkpoint token accounting (input / output / cache
     * breakdown) on `CORTEX_STEP_TYPE_CHECKPOINT` steps. Same loose
     * typing + defensive extraction as `modelCost`.
     */
    modelUsage?: Record<string, unknown>;
}

export interface ResponseDimensionGroup {
    /** e.g. `"Token Usage"` — English-only today, but we match by equality. */
    title?: string;
    dimensions?: ReadonlyArray<ResponseDimension>;
}

export interface ResponseDimension {
    /** `input_tokens` / `output_tokens` / `cached_input_tokens` / other. */
    uid?: string;
    cumulativeMetric?: { value?: number };
}

/** Response shape of `GetCascadeTrajectorySteps`. */
export interface CascadeStepsResponse {
    steps?: ReadonlyArray<CascadeStep>;
}

/** Response shape of `GetAllCascadeTrajectories`. */
export interface CascadeTrajectoriesResponse {
    /** Map of cascade id → summary. */
    trajectorySummaries?: Readonly<Record<string, TrajectorySummary>>;
}
