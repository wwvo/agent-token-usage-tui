// Windsurf Cascade Language Server RPC client.
//
// The LS only accepts requests bearing an `x-codeium-csrf-token` header
// plus a matching `host` port. Neither is exposed through a public API;
// we extract them by monkey-patching `http.ClientRequest.prototype.end`
// / `.write` just long enough to intercept whatever the LS fires off
// through its own `devClient()` hook, then restore the prototype.
//
// The trick is load-bearing and slightly ugly. Justifications inline.

import * as http from "http";
import * as vscode from "vscode";

import type {
  CascadeStepsResponse,
  CascadeTrajectoriesResponse,
  TrajectorySummary,
  WindsurfCredentials,
} from "./types";

/** Per-process credential cache. Cleared on RPC failure (`clearCredentials`). */
let cachedCreds: WindsurfCredentials | null = null;

/**
 * Obtain credentials, preferring the cache and re-extracting on expiry.
 *
 * Returns `null` when the Windsurf extension isn't loaded, the LS hasn't
 * finished booting, or the CSRF capture dance didn't produce a header
 * within its budget. Callers should treat `null` as "try again later".
 */
export async function getCredentials(): Promise<WindsurfCredentials | null> {
  if (cachedCreds) {
    // Cheap validation ping: hitting `GetProcesses` with the cached
    // creds either returns 200 (still valid) or anything else (LS
    // restarted, rotate). Keeps the extraction cost off the hot path.
    try {
      const resp = await httpPost(
        `http://127.0.0.1:${cachedCreds.port}/exa.language_server_pb.LanguageServerService/GetProcesses`,
        { "x-codeium-csrf-token": cachedCreds.csrf },
        "{}",
      );
      if (resp.status === 200) {
        return cachedCreds;
      }
    } catch {
      /* fall through to re-extract */
    }
    cachedCreds = null;
  }
  cachedCreds = await extractCsrf();
  return cachedCreds;
}

/** Force the next `getCredentials()` call to re-extract from the LS. */
export function clearCredentials(): void {
  cachedCreds = null;
}

// ---- Credential extraction -----------------------------------------------

/**
 * Extract CSRF + port by intercepting an LS-initiated HTTP request.
 *
 * Strategy:
 * 1. Look up the `codeium.windsurf` extension and grab its `devClient()`.
 *    The extension exports a grab-bag of RPC stubs intended for internal
 *    debugging; calling any of them triggers an HTTP request through
 *    Node's `http` module with the CSRF + port headers we need.
 * 2. Monkey-patch `http.ClientRequest.prototype.{end, write}` to read
 *    headers off every outgoing request. We catch the token and host
 *    on the first request that carries both.
 * 3. Iterate the devClient's stub methods, calling each with `{}`; every
 *    call will throw (wrong args), but the headers are captured before
 *    the body is serialized. We cap each call at 5 s so a hung stub
 *    can't freeze the whole extraction.
 * 4. Always restore the original prototype in `finally`.
 *
 * The patch-and-restore is global for the process; while it's active,
 * *all* `http` requests see our shim. This is fine: the shim only reads
 * headers, never mutates, and completes in a few hundred ms in practice.
 */
async function extractCsrf(): Promise<WindsurfCredentials | null> {
  const ext = vscode.extensions.getExtension("codeium.windsurf");
  if (!ext?.isActive) {
    return null;
  }
  const exp = ext.exports as { devClient?: () => unknown };
  if (typeof exp?.devClient !== "function") {
    return null;
  }

  // Retry until the LS has finished booting its devClient; new windows
  // hit this on cold start. 10 × 2 s = 20 s budget.
  let devClient: Record<string, unknown> | null = null;
  for (let attempt = 0; attempt < 10; attempt++) {
    const candidate = exp.devClient() as Record<string, unknown> | null;
    if (candidate) {
      devClient = candidate;
      break;
    }
    await sleep(2000);
  }
  if (!devClient) {
    return null;
  }

  let capturedCsrf = "";
  let capturedPort = 0;

  const origEnd = http.ClientRequest.prototype.end;
  const origWrite = http.ClientRequest.prototype.write;

  const captureHeaders = function capture(this: http.ClientRequest): void {
    if (capturedCsrf) {
      return;
    }
    try {
      const csrf = this.getHeader("x-codeium-csrf-token");
      if (!csrf) {
        return;
      }
      capturedCsrf = String(csrf);
      const host = this.getHeader("host");
      if (host) {
        const match = String(host).match(/:(\d+)/);
        if (match) {
          capturedPort = Number(match[1]);
        }
      }
    } catch {
      /* best-effort; any throw during capture should not break the request */
    }
  };

  try {
    // `as any` is unavoidable: the prototype signatures use overloaded
    // tuples Node doesn't expose to TS. We keep the shim minimal and
    // delegate to the original on every path.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    http.ClientRequest.prototype.end = function patchedEnd(this: http.ClientRequest, ...args: any[]) {
      captureHeaders.call(this);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return origEnd.apply(this, args as any);
    };
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    http.ClientRequest.prototype.write = function patchedWrite(this: http.ClientRequest, ...args: any[]) {
      captureHeaders.call(this);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return origWrite.apply(this, args as any);
    };

    for (const methodName of Object.keys(devClient)) {
      const maybeFn = devClient[methodName];
      if (typeof maybeFn !== "function") {
        continue;
      }
      try {
        await Promise.race([
          (maybeFn as (arg: Record<string, never>) => Promise<unknown>)({}),
          new Promise((_resolve, reject) => {
            setTimeout(() => reject(new Error("devClient timeout")), 5000);
          }),
        ]);
      } catch {
        /* expected — we only need the intercepted headers */
      }
      if (capturedCsrf) {
        break;
      }
    }
  } finally {
    http.ClientRequest.prototype.end = origEnd;
    http.ClientRequest.prototype.write = origWrite;
  }

  if (capturedCsrf && capturedPort > 0) {
    return { csrf: capturedCsrf, port: capturedPort };
  }
  return null;
}

// ---- HTTP helper ----------------------------------------------------------

/**
 * POST a JSON body and resolve with the raw status + text.
 *
 * Two independent timeouts:
 * - 5 s `req.timeout` — triggered by Node when the socket stalls.
 * - 10 s hard wall-clock — backstop for cases where the stream is
 *   actively streaming but never ends. Both converge on `req.destroy()`
 *   so callers never leak file descriptors.
 */
function httpPost(
  url: string,
  extraHeaders: Readonly<Record<string, string>>,
  body: string,
): Promise<{ status: number; body: string }> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const parsed = new URL(url);

    const hardTimer = setTimeout(() => {
      if (!settled) {
        settled = true;
        req.destroy();
        reject(new Error("timeout (hard)"));
      }
    }, 10_000);

    const req = http.request(
      {
        hostname: parsed.hostname,
        port: parsed.port,
        path: parsed.pathname,
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Connect-Protocol-Version": "1",
          ...extraHeaders,
        },
        timeout: 5000,
      },
      (res) => {
        let data = "";
        res.on("data", (chunk) => {
          data += chunk;
        });
        res.on("end", () => {
          if (!settled) {
            settled = true;
            clearTimeout(hardTimer);
            resolve({ status: res.statusCode ?? 0, body: data });
          }
        });
      },
    );
    req.on("error", (err) => {
      if (!settled) {
        settled = true;
        clearTimeout(hardTimer);
        reject(err);
      }
    });
    req.on("timeout", () => {
      if (!settled) {
        settled = true;
        clearTimeout(hardTimer);
        req.destroy();
        reject(new Error("timeout"));
      }
    });
    req.write(body);
    req.end();
  });
}

// ---- Public RPC surface ---------------------------------------------------

/**
 * Type-narrowed RPC call. `T` is the expected JSON shape; we parse once
 * and let the caller assert narrower types if needed.
 */
async function apiCall<T>(
  creds: WindsurfCredentials,
  method: string,
  body: object,
): Promise<T> {
  const resp = await httpPost(
    `http://127.0.0.1:${creds.port}/exa.language_server_pb.LanguageServerService/${method}`,
    { "x-codeium-csrf-token": creds.csrf },
    JSON.stringify(body),
  );
  if (resp.status !== 200) {
    throw new Error(`API ${method}: HTTP ${resp.status} — ${resp.body.slice(0, 200)}`);
  }
  return JSON.parse(resp.body) as T;
}

/**
 * List every Cascade trajectory the LS remembers (summaries only).
 *
 * `include_user_inputs: false` keeps the payload small; we only need
 * the metadata map + the `stepCount` staleness heuristic.
 */
export async function listCascades(
  creds: WindsurfCredentials,
): Promise<Readonly<Record<string, TrajectorySummary>>> {
  const resp = await apiCall<CascadeTrajectoriesResponse>(
    creds,
    "GetAllCascadeTrajectories",
    { include_user_inputs: false },
  );
  return resp.trajectorySummaries ?? {};
}

/** Fetch every step of a single Cascade trajectory. */
export async function fetchCascadeSteps(
  creds: WindsurfCredentials,
  cascadeId: string,
): Promise<CascadeStepsResponse> {
  return apiCall<CascadeStepsResponse>(creds, "GetCascadeTrajectorySteps", {
    cascade_id: cascadeId,
  });
}

// ---- internal utils -------------------------------------------------------

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
