// shardlane-herdr-agent-state.ts — Shardlane-owned Command Code → Herdr agent bridge.
// Managed by Shardlane. Do not hand-edit:
// Shardlane atomically replaces this file on every Provider launch.
//
// Reports Command Code lifecycle into Herdr (the only runtime state authority)
// through the herdr CLI:
//   session_start            → idle presence (agent appears on launch)
//   run_start {sessionId}    → working + --agent-session-id (resume identity,
//                              same id space as `command-code --session <id>`)
//   run_end                  → idle
//   interrupted / run_error  → idle (defensive: hard stops never reach onStop;
//                              a stale "working" after an abort would be worse
//                              than a redundant idle report)
//   session_shutdown         → release-agent
//
// blocked is intentionally NOT reported: the current Mod API exposes no
// unambiguous "approval UI is waiting" event (tool_denied means denial+abort,
// tool_queued fires before permission checks). Fabricating it would lie.
//
// No-op outside a Herdr pane (missing HERDR_PANE_ID / HERDR_BIN_PATH).
// Subprocesses go through node:child_process directly — a mod is in-process
// Node code, and this avoids depending on any runtime-specific exec helper.

import { execFile as execFileCallback, execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";

const SOURCE = "shardlane:commandcode:v1";
const AGENT = "commandcode";
const REPORT_TIMEOUT_MS = 10_000;

// Optional diagnostics: set SHARDLANE_BRIDGE_DEBUG=<path> to trace the bridge
// (load, reports, release). Env-gated so normal sessions stay silent.
const debugLog = (message: string): void => {
  const path = process.env.SHARDLANE_BRIDGE_DEBUG;
  if (!path) return;
  try {
    appendFileSync(path, `${Date.now()} ${message}\n`);
  } catch {
    // Diagnostics must never disturb the session.
  }
};

function execFile(bin: string, args: string[]): Promise<void> {
  return new Promise((resolve, reject) => {
    execFileCallback(
      bin,
      args,
      { timeout: REPORT_TIMEOUT_MS },
      (error) => (error ? reject(error) : resolve()),
    );
  });
}

// A mod default-exports a factory that receives the harness API bound as `cmd`
// (installed v1.15.1: `on(event, handler) → unsubscribe`; jiti strips types).
export default function (cmd: {
  on(event: string, handler: (payload?: unknown) => void): unknown;
}): void {
  const paneId = process.env.HERDR_PANE_ID;
  const herdrBin = process.env.HERDR_BIN_PATH;
  if (!paneId || !herdrBin) return; // outside a Herdr pane: pure no-op
  debugLog(`load pane=${paneId}`);

  // Herdr ignores stale sequence values per source, so the counter must be
  // strictly increasing. The time-seeded base keeps ordering across process
  // restarts on the same pane; the counter itself never reads the clock again.
  let seq = Date.now();
  const nextSeq = (): string => String(++seq);

  // Resume identity, once learned, rides on EVERY report: a state-only report
  // would otherwise overwrite (clear) the pane's agent_session on Herdr side.
  let knownSessionId: string | undefined;
  // Set once any report is attempted — the exit hook only releases authority
  // this process actually established.
  let reported = false;

  // Serialize herdr reports so report order preserves event order; a failed
  // report must never disturb the session, so failures are swallowed.
  let chain: Promise<unknown> = Promise.resolve();
  const run = (args: string[]): void => {
    chain = chain.then(() => execFile(herdrBin, args)).catch(() => {});
  };

  const report = (state: "idle" | "working", sessionId?: string): void => {
    reported = true;
    if (sessionId) knownSessionId = sessionId;
    debugLog(`report ${state} session=${knownSessionId ?? "none"}`);
    const args = [
      "pane",
      "report-agent",
      paneId,
      "--source",
      SOURCE,
      "--agent",
      AGENT,
      "--state",
      state,
      "--seq",
      nextSeq(),
    ];
    if (knownSessionId) args.push("--agent-session-id", knownSessionId);
    run(args);
  };

  // Session identity has a DEDICATED Herdr method (plan §5.2): for a custom
  // label, report-agent's --agent-session-id is accepted but not projected,
  // while pane.report-agent-session establishes agent_session on the pane.
  // Idempotent: only reported when the id is new.
  let reportedSessionId: string | undefined;
  const reportSession = (sessionId: string): void => {
    if (sessionId === reportedSessionId) return;
    reportedSessionId = sessionId;
    run([
      "pane",
      "report-agent-session",
      paneId,
      "--source",
      SOURCE,
      "--agent",
      AGENT,
      "--agent-session-id",
      sessionId,
      "--seq",
      nextSeq(),
    ]);
  };

  // Release is guarded by the same per-source seq watermark as state reports:
  // a seq-less release is accepted but ignored (live-verified Herdr 0.8.2), so
  // every release must carry the next monotonic seq.
  const releaseArgs = (): string[] => [
    "pane",
    "release-agent",
    paneId,
    "--source",
    SOURCE,
    "--agent",
    AGENT,
    "--seq",
    nextSeq(),
  ];
  const release = (): void => {
    run(releaseArgs());
  };

  cmd.on("session_start", () => report("idle"));
  cmd.on("run_start", (payload?: { sessionId?: string }) => {
    report("working", payload?.sessionId);
    if (payload?.sessionId) reportSession(payload.sessionId);
  });
  // Defensive: some entry points may fire run_start before binding the id.
  cmd.on("run_end", (payload?: {
    result?: { nextState?: { sessionId?: string } };
    sessionId?: string;
  }) => {
    const id = payload?.result?.nextState?.sessionId ?? payload?.sessionId;
    report("idle", id);
    if (id) reportSession(id);
  });
  cmd.on("interrupted", () => report("idle"));
  cmd.on("run_error", () => report("idle"));
  // In-process session switch/dispose (v1.15.1 emits this only while running).
  cmd.on("session_shutdown", () => release());
  // Process exit: session_shutdown does NOT fire when the TUI quits, so the
  // release anchor for process teardown is Node's synchronous exit hook
  // (async execFile cannot complete during teardown; execFileSync can).
  process.once("exit", () => {
    debugLog("exit-hook fired");
    if (!reported) return;
    try {
      execFileSync(herdrBin, releaseArgs(), { timeout: REPORT_TIMEOUT_MS });
      debugLog("release ok");
    } catch (error) {
      debugLog(`release failed: ${String(error)}`);
      // Best-effort: Herdr's pane lifecycle owns the rest.
    }
  });
}
