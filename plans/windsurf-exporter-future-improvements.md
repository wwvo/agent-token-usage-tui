# Windsurf exporter — future improvements (post-v0.2.9)

*Status: notes only. Nothing in here is on the short-term roadmap.*

v0.2.9 ships a working exporter, but at three points the design makes
deliberate compromises because we didn't know the ground truth of the
Cascade step payload. A live probe on Windsurf's DevTools console
(see "How we learned this" below) now shows the real shape, and it's
richer than v0.2.9 assumes. We document the opportunity here so a
future v0.2.10 / v0.3 can pick it up without re-discovering it.

## The real step shape

```jsonc
{
  "type": "CORTEX_STEP_TYPE_USER_INPUT",
  "status": "CORTEX_STEP_STATUS_DONE",
  "metadata": {
    "createdAt": "2025-01-11T13:23:18.978469600Z",
    "source": "CORTEX_STEP_SOURCE_USER_EXPLICIT",
    "executionId": "3e5d8214-d106-4407-b6e5-39d34bccbd79"
  },
  "userInput": {
    "userResponse": "…",
    "activeUserState": {
      "activeDocument": {
        "absoluteUri": "file:///c:/Users/.../sanitation/staged_changes.diff",
        "workspaceUri": "file:///c:/Users/.../sanitation",
        "editorLanguage": "diff",
        "lineEnding": "\n"
      }
    }
  }
}
```

And the real trajectory summary:

```jsonc
{
  "summary": "Generating Git Commit Message",
  "stepCount": 5,
  "lastModifiedTime": "2025-01-11T13:23:29.076577700Z",
  "trajectoryId": "ea2dc248-68eb-4d87-8b9d-1e9cc215ea95",
  "status": "CASCADE_RUN_STATUS_IDLE",
  "lastUserInputTime": "2025-01-11T13:23:18.978469600Z",
  "trajectoryType": "CORTEX_TRAJECTORY_TYPE_CASCADE",
  "referencedFiles": ["…"]
}
```

Note what's **not** there: the summary carries no `workspaces` array, no `workspaceFolderAbsoluteUri`. That's why v0.2.9's `writer.ts` reads `summary.workspaces?.[0]?.workspaceFolderAbsoluteUri ?? ""` and lands on `""` for ~32% of sessions (16 out of 50 on the dev machine).

## Three opportunities

### 1. `timestamp` — use `step.metadata.createdAt` directly

**Today (v0.2.9)**. `extractTurnUsage` assembles `timestamp` via a fallback chain:

```ts
step.timestamp
  || summary.lastUserInputTime
  || summary.lastModifiedTime
  || summary.createdTime
  || ""
```

`step.timestamp` is always `""` in practice, so every turn inherits the per-cascade `lastUserInputTime`. All N turns in a cascade share one timestamp — the Trend view can't distinguish them.

**Upgrade**. `step.metadata.createdAt` is a per-step ISO-8601 with nanosecond precision. Read it first, fall back to the existing chain only if missing. Payoff: per-turn resolution in the Trend / Activity views.

### 2. `step_id` — use `step.metadata.executionId`

**Today**. `writeSession` synthesizes `turn-<N>` where N is the index of valid `user_input` turns in the cascade. Dedup across refreshes works because cascade trajectories are server-side append-only, so "the Nth valid turn" is stable.

**Upgrade**. `step.metadata.executionId` is a UUID assigned server-side per step. It's stable by construction and doesn't depend on the assumption that the exporter walks steps in the same order every time. Swap in as `step_id`; keep `turn-<N>` as a fallback when `executionId` is missing (older Windsurf builds?).

### 3. `workspace` — promote to per-turn, pull from `activeDocument.workspaceUri`

**Today**. `session_meta.workspace` is a per-cascade field whose source (`summary.workspaces`) doesn't exist. 32% empty on the dev machine.

**Upgrade**. Every `USER_INPUT` step carries `userInput.activeUserState.activeDocument.workspaceUri`. That's *per-turn* workspace resolution — a single cascade can span multiple workspaces if the user switches mid-conversation, and the per-turn data captures it.

Schema options:

- **Non-breaking**: add `workspace` to `turn_usage`; leave `session_meta.workspace` alone and optionally backfill it to the first `USER_INPUT` turn's workspace.
- **Breaking**: deprecate `session_meta.workspace` and let the Rust collector aggregate workspace from `turn_usage` rows.

The non-breaking path is preferred — keeps old `.jsonl` files readable by the new collector.

## LevelDB probe — archived, not pursued

A scratch repo at `C:\Users\Administrator\code\windsurf-leveldb-probe\` proves Chromium's `Local Storage/leveldb/` for Windsurf can be read out-of-process with `classic-level` (copy the directory to a tmp location to sidestep the LOCK file, then iterate). Three keys are useful there:

- `cascade-open-sessions-by-workspace` — workspace → cascade id reverse map, but only for *currently open* cascade tabs.
- `cascade-last-viewed-at` — per-cascade last-view timestamp.
- `cascade-tab-editor-state` — draft Lexical editor content (not useful for token tracking).

ROI is **lower than just reading the step fields above**:

- The three step fields are 100% coverage (every `USER_INPUT` step has them).
- LevelDB coverage is limited to open tabs (~6 / 50 on the dev machine).
- LevelDB requires copy-to-tmp + cross-platform path handling + a new dep (Rust) or a rewrite to TS.
- The reference user (`exposuresolutions/achill-island-market` in their `docs/ai_memories.md`) only reached LevelDB after *losing* their Windsurf install, i.e. it's a recovery tool, not a first-class datasource.

Keep the probe around as documentation of what's possible; don't wire it into the exporter.

## Server-side retention: DONE vs CLEARED

A second probe (see `fetch-cascade.js` in the scratch repo) fetched full step lists for two cascades and bucketed them by `status`:

| Cascade | Age when fetched | Steps returned | `DONE` | `CLEARED` | Content retention |
|---|---|---|---|---|---|
| "Refine Changelog and Release Notes" | 0 days | 591 | 578 | 0 | 100% |
| "Phase 9 D2D Migration" | 2 days | 2972 | 110 | 2862 | 3.7% |

Interpretation. Windsurf's Language Server keeps full step content (`plannerResponse`, `codeAction`, `runCommand`, `userInput`, …) for recent cascades and GC's the content on older steps, leaving only `metadata` + `responseDimensionGroups`. Token statistics survive the GC, which is why the exporter's usage accounting is unaffected. Conversation *replay*, on the other hand, is only possible for recent cascades.

The exact GC policy (time-based vs step-count-based vs mixed) isn't known — we only have two data points and they don't bracket the cutoff. Running `fetch-cascade.js` against all 50 cascades would give enough data to curve-fit, but it isn't required for the exporter's current scope.

Importantly the API reports a `stepCount` in the summary that includes cleared-and-pruned steps (Phase 9's summary says 4581; the RPC returns only 2972 live step objects). So the exporter's idea of "how many turns are in this cascade" can never be the summary's `stepCount` verbatim — it has to count the live USER_INPUT steps it actually sees.

## New fields worth knowing about

Everything below comes from DONE-status steps. CLEARED steps don't have these.

- `step.metadata.cumulativeTokensAtStep` — per-step running total. Currently we compute the running total ourselves by summing `turn_usage` rows; this is a server-provided ground truth we could cross-check.
- `step.metadata.modelCost` / `step.metadata.modelUsage` (on `CHECKPOINT` steps) — Windsurf's own USD estimate for the cascade. Would let atut sanity-check its `pricing.cost::calc_cost` against the server's answer and flag drift when Windsurf ships pricing changes ahead of litellm.
- `step.subtrajectory` (on some `RETRIEVE_MEMORY` / etc. steps) — a nested trajectory that runs inline inside a parent step (e.g. a memory-retrieval sub-agent). This explains the `stepCount` vs `steps.length` gap: sub-trajectory steps count in `stepCount` but arrive nested, not flat. The exporter doesn't descend into these — none of the Token Usage ends up in sub-trajectory steps, but turn counting for the TUI should eventually handle this shape.
- `step.codeAction` (on `CODE_ACTION|DONE`) — the full `edit` / `write_to_file` / `multi_edit` arguments including `file_path`, `old_string`, `new_string`. A 205 KB payload per large edit. Useful for a hypothetical "changes by agent" view; irrelevant to token accounting.
- `step.runCommand` (on `RUN_COMMAND|DONE`) — shell command + stdout + exit code. `RUN_COMMAND|ERROR` additionally carries an `error` field with the stderr stream.
- `step.askUserQuestion.requestedInteraction` — the full prompt the agent sent to the user plus the options. Useful when replaying why a cascade took a particular branch.
- `step.viewFile` / `step.grepSearch` / `step.listDirectory` / `step.find` — the tool's input + output, mirroring the CLI-side primitives.

Two non-DONE statuses also carry content:

- `CORTEX_STEP_STATUS_ERROR` — RUN_COMMAND / GREP_SEARCH / MCP_TOOL / CODE_ACTION can land here. Keeps the arguments plus an `error` string. Valuable for an "agent failure rate" metric.
- `CORTEX_STEP_STATUS_CANCELED` — user-interrupted tool invocations. Rare (1 on the today cascade); same payload as DONE minus the successful output.

## Verification against the reference UI

`aggregate.js` in the scratch repo reads only `~/.atut/windsurf-sessions/*.jsonl` (no Windsurf RPC) and rolls up per-cascade totals. Side-by-side with the Windsurf Token Usage extension's dashboard screenshot the user captured on 2026-04-20:

| Conversation | Reference dashboard | atut JSONL rollup |
|---|---|---|
| #1 Implement Settings Panel | 75 turns / 992.6M | 75 / 992.6M ✓ |
| #2 Phase 9 D2D Migration | 38 / 360.5M | 38 / 360.5M ✓ |
| #3 Refine Changelog and Release Notes | 10 / 76.2M | 10 / 76.2M ✓ |
| (grand total) | 50 conv. / 1447.8M | 50 / 1.45B ✓ |

Conclusion: the exporter loses no data vs the reference. The only gap is presentation — atut's current TUI aggregates everything into a single `windsurf` row in Overview; surfacing per-cascade rows would require a new TUI view, not more exporter work.

## How we learned this

The probe happened during a live troubleshooting session: after v0.2.8 shipped and the TUI still showed zero Windsurf records, we patched the installed `api.js` + `writer.js` to dump `Object.keys(devClient)`, sample steps, and sample summaries to `~/atut-debug.log`, then asked the user to restart Windsurf and paste the log. The "real step shape" section above is a verbatim excerpt from that dump. The later retention probe added a `writeFileSync` call so the captured CSRF + port land in `~/atut-debug.creds.json`; the scratch `fetch-cascade.js` uses that file to talk to the Language Server directly, outside the extension, which makes every future datastructure probe a one-node-run away. No scraping, no reverse-engineering beyond `console.log`.

The patches are **not** rolled back in this hand-off — they keep refreshing `~/atut-debug.creds.json` on every exporter tick, which is convenient for more probes but also means `~/atut-debug.log` grows without bound while they're in place. Before the next release either the probe work continues (keep them) or the installed extension is reinstalled from the official `.vsix` (resets cleanly).

## Why we're **not** doing this yet

- v0.2.9 works end-to-end: the TUI shows real Windsurf data, records are deduped, timestamps land in the right day bucket.
- A schema bump on `turn_usage` would force a coordinated change in the Rust collector (`src/collector/windsurf.rs`) and the tests. Worth doing, but not in a hot-fix cadence.
- Users had three versions in three hours during the step.id / timestamp shakeout. One more rapid bump for "nicer data" starts to feel like churn from their side.

When we come back to this, a reasonable v0.2.10 shape is:

1. Upgrade `timestamp` to `step.metadata.createdAt` (smallest change, immediate visible win).
2. Upgrade `step_id` to `step.metadata.executionId` (robustness, no visible change).
3. Defer the `workspace` schema promotion to v0.3 along with the TUI's eventual per-project breakdown.
