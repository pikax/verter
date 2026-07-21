/**
 * One-route corpus benchmark session.
 *
 * Opens the deterministic sample read-only, fires the mined authored-position
 * requests with hard per-request bounds, detects wedges (a request whose
 * promise never settles, or a `$/verter/getStatistics` liveness check going
 * dark after a timeout), samples RSS for the server + provider processes, and
 * returns an exactly-accounted route report. The session NEVER hangs: every
 * await is bounded, and the route as a whole runs under a wall-clock budget
 * the caller additionally enforces from outside.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { GET_STATISTICS_METHOD } from "../core/startupGate.js";
import { extractQuiescenceCounters, pollUntilQuiesced } from "../core/quiescence.js";
import { summarizeKinds } from "./metrics.js";
import { mineCorpusProbes, type CorpusProbe } from "./probes.js";
import { ProcessTreeSampler } from "./processTree.js";
import { sampleManifestHash } from "./sample.js";
import { spawnCorpusGateLsp, type CorpusGateLspHandle } from "./spawn.js";
import { UNPROVEN_ISOLATION } from "./topology.js";
import {
  completionIsEmpty,
  definitionIsEmpty,
  hoverIsEmpty,
  referencesIsEmpty,
} from "./verdicts.js";
import type {
  CorpusGateConfig,
  CorpusGateRoute,
  CorpusRequestKind,
  CorpusRequestObservation,
  CorpusRouteEarlyStop,
  CorpusRouteReport,
  CorpusRouteStartup,
} from "./types.js";

/** The sentinel a promise-race resolves to when the raced promise never settled. */
const NEVER_SETTLED = Symbol("corpus-gate-never-settled");

/**
 * Race `promise` against a hard timer. Resolves to the promise's settlement
 * (value or rethrown error) or to {@link NEVER_SETTLED} when the timer wins.
 * This is the wedge-detection primitive: the underlying `LspClient` already
 * enforces a client-side timeout, so the raced timer only ever wins when that
 * timeout ITSELF failed to fire — the hang class the throwaway probe hit.
 */
async function raceSettlement<T>(
  promise: Promise<T>,
  boundMs: number,
): Promise<{ settled: true; value?: T; error?: unknown } | { settled: false }> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeoutPromise = new Promise<typeof NEVER_SETTLED>((resolve) => {
    timer = setTimeout(() => resolve(NEVER_SETTLED), boundMs);
    timer.unref?.();
  });
  try {
    const raced = await Promise.race([
      promise.then(
        (value) => ({ settled: true as const, value }),
        (error: unknown) => ({ settled: true as const, error }),
      ),
      timeoutPromise,
    ]);
    return raced === NEVER_SETTLED ? { settled: false } : raced;
  } finally {
    if (timer) clearTimeout(timer);
  }
}

interface MutableAccounting {
  requestsSent: number;
  requestsAnswered: number;
  requestsEmpty: number;
  requestsTimedOut: number;
  requestsErrored: number;
  requestsAbandoned: number;
  filesOpened: number;
  filesSkipped: number;
  probesMined: number;
}

export interface RunCorpusRouteOptions {
  /** Override for the spawner (hermetic tests inject a fake). */
  readonly spawn?: typeof spawnCorpusGateLsp;
  readonly log?: (message: string) => void;
}

function emptyStartup(): CorpusRouteStartup {
  return {
    initializeMs: 0,
    readyObserved: false,
    syncObserved: false,
    quiesced: false,
    settleMs: 0,
  };
}

/**
 * Run one route's benchmark session end-to-end. Never throws for in-session
 * defects (wedges, timeouts, provider death) — those are report content; it
 * reports spawn/initialize failure through `fatalError`.
 */
export async function runCorpusRoute(
  route: CorpusGateRoute,
  config: CorpusGateConfig,
  sampleRelativePaths: readonly string[],
  options: RunCorpusRouteOptions = {},
): Promise<CorpusRouteReport> {
  const log = options.log ?? (() => {});
  const spawn = options.spawn ?? spawnCorpusGateLsp;
  const startedAt = Date.now();
  const deadline = startedAt + config.routeBudgetMs;
  const observations: CorpusRequestObservation[] = [];
  const accounting: MutableAccounting = {
    requestsSent: 0,
    requestsAnswered: 0,
    requestsEmpty: 0,
    requestsTimedOut: 0,
    requestsErrored: 0,
    requestsAbandoned: 0,
    filesOpened: 0,
    filesSkipped: 0,
    probesMined: 0,
  };
  let livenessChecks = 0;
  let livenessFailures = 0;
  let wedged = false;
  let wedgeDetail: string | null = null;
  let completed = false;
  let fatalError: string | null = null;
  let startup = emptyStartup();
  const allowedEmpty = new Set(config.thresholds.allowedEmptyCategories);
  // ONE sampler over the WHOLE spawned tree: sampling a single advertised pid
  // let a multi-GB child sit outside the per-process ceiling entirely.
  const treeSampler = new ProcessTreeSampler(config.rssSampleIntervalMs);
  const earlyStop: { enabled: boolean; stopped: boolean; reason: string | null } = {
    enabled: config.fastMode,
    stopped: false,
    reason: null,
  };

  let handle: CorpusGateLspHandle | null = null;
  try {
    handle = await spawn(route, config.corpusDir, {
      readyCapMs: config.startupReadyCapMs,
      settleCapMs: config.startupSettleCapMs,
      statisticsTimeoutMs: config.wedgeLivenessTimeoutMs,
    });
  } catch (error) {
    fatalError = String((error as Error)?.message ?? error).slice(0, 500);
    log(`[corpus-gate:${route}] spawn failed: ${fatalError}`);
  }

  if (handle) {
    startup = handle.startup;
    const client = handle.client;
    const lspPid = client.process.pid;
    const relayPid = handle.relay?.process.pid ?? null;
    const refreshTreeRoots = (): void => {
      treeSampler.setRoots({
        serverPid: lspPid ?? null,
        relayPid,
        providerPid: handle?.providerPid() ?? null,
      });
    };
    refreshTreeRoots();
    treeSampler.start();

    /** Bounded liveness check; a dark server flips the route to wedged. */
    const checkLiveness = async (context: string): Promise<boolean> => {
      livenessChecks += 1;
      const result = await raceSettlement(
        client.sendRequest(GET_STATISTICS_METHOD, {}, config.wedgeLivenessTimeoutMs),
        config.wedgeLivenessTimeoutMs + 5_000,
      );
      if (!result.settled || result.error !== undefined) {
        livenessFailures += 1;
        wedged = true;
        wedgeDetail = `liveness check went dark (${context}): getStatistics ${
          result.settled
            ? `failed: ${String((result.error as Error)?.message ?? result.error).slice(0, 200)}`
            : "never settled"
        }`;
        log(`[corpus-gate:${route}] WEDGE — ${wedgeDetail}`);
        return false;
      }
      return true;
    };

    /**
     * Opt-in early stop (never the default). It fires ONLY on already-recorded
     * MONOTONE failures — an unexpected empty result or a breached per-process
     * RSS ceiling cannot be un-failed by measuring more requests — so the
     * verdict is already decided and the remaining census only costs time. A
     * passing route is never cut short.
     */
    const earlyStopReason = (): string | null => {
      if (!config.fastMode) return null;
      const empty = observations.find((observation) => observation.unexpectedEmpty);
      if (empty !== undefined) {
        return `unexpected empty ${empty.kind} result (${empty.category}) already failed this route`;
      }
      const maxRss = treeSampler.maxObservedRssBytes();
      if (maxRss !== null && maxRss > config.thresholds.rssMaxBytes) {
        return `per-process RSS ceiling already breached (${maxRss} > ${config.thresholds.rssMaxBytes})`;
      }
      return null;
    };

    try {
      fileLoop: for (const relativePath of sampleRelativePaths) {
        if (Date.now() > deadline) break;
        const absolute = path.join(config.corpusDir, relativePath);
        let text: string;
        try {
          text = readFileSync(absolute, "utf8");
        } catch {
          accounting.filesSkipped += 1;
          continue;
        }
        const uri = pathToFileURL(absolute).href;
        client.sendNotification("textDocument/didOpen", {
          textDocument: { uri, languageId: "vue", version: 1, text },
        });
        accounting.filesOpened += 1;

        // Capped per-file settle; its own failure is not a wedge (the liveness
        // check decides that) but a statistics hang here must not stall the run.
        const settleResult = await raceSettlement(
          pollUntilQuiesced(
            async () =>
              extractQuiescenceCounters(
                await client.sendRequest(GET_STATISTICS_METHOD, {}, config.openSettleCapMs),
              ),
            () => [],
            { timeoutMs: config.openSettleCapMs },
          ),
          config.openSettleCapMs + 10_000,
        );
        if (!settleResult.settled) {
          wedged = true;
          wedgeDetail = `open-settle poll never settled for a sampled file (after ${config.openSettleCapMs + 10_000}ms)`;
          log(`[corpus-gate:${route}] WEDGE — ${wedgeDetail}`);
          break;
        }

        const probes: CorpusProbe[] = mineCorpusProbes(text, config.maxProbesPerFile);
        accounting.probesMined += probes.length;
        for (const probe of probes) {
          for (const kind of probe.kinds) {
            if (Date.now() > deadline) break fileLoop;
            if (!client.isAlive()) {
              fatalError = `verter-lsp died mid-session (before ${kind} @ file ${accounting.filesOpened})`;
              log(`[corpus-gate:${route}] ${fatalError}`);
              break fileLoop;
            }
            const position = { line: probe.line, character: probe.character };
            const requestPromise: Promise<unknown> =
              kind === "hover"
                ? client.sendRequest(
                    "textDocument/hover",
                    { textDocument: { uri }, position },
                    config.requestTimeoutMs,
                  )
                : kind === "definition"
                  ? client.sendRequest(
                      "textDocument/definition",
                      { textDocument: { uri }, position },
                      config.requestTimeoutMs,
                    )
                  : kind === "completion"
                    ? client.sendRequest(
                        "textDocument/completion",
                        {
                          textDocument: { uri },
                          position,
                          context: { triggerKind: 2, triggerCharacter: "." },
                        },
                        config.requestTimeoutMs,
                      )
                    : client.sendRequest(
                        "textDocument/references",
                        {
                          textDocument: { uri },
                          position,
                          context: { includeDeclaration: true },
                        },
                        config.requestTimeoutMs,
                      );
            const requestStart = Date.now();
            accounting.requestsSent += 1;
            // Hard settlement bound = client timeout + grace: the client-side
            // timeout should reject first; the race winning means the timeout
            // machinery itself hung — the wedge class.
            const raced = await raceSettlement(requestPromise, config.requestTimeoutMs + 10_000);
            const ms = Date.now() - requestStart;
            if (!raced.settled) {
              accounting.requestsAbandoned += 1;
              wedged = true;
              wedgeDetail =
                `request never settled: ${kind} (${probe.category}) after ${ms}ms — ` +
                `client-side timeout did not fire`;
              log(`[corpus-gate:${route}] WEDGE — ${wedgeDetail}`);
              break fileLoop;
            }
            let verdict: CorpusRequestObservation["verdict"];
            if (raced.error !== undefined) {
              const message = String((raced.error as Error)?.message ?? raced.error);
              verdict = /timed out|timeout/i.test(message) ? "timeout" : "error";
              if (verdict === "timeout") {
                accounting.requestsTimedOut += 1;
                // A timeout demands a liveness verdict: answered-late vs dark.
                if (!(await checkLiveness(`after ${kind} timeout`))) {
                  observations.push({
                    kind,
                    category: probe.category,
                    ms,
                    verdict,
                    unexpectedEmpty: false,
                  });
                  break fileLoop;
                }
              } else {
                accounting.requestsErrored += 1;
              }
            } else {
              const empty =
                kind === "hover"
                  ? hoverIsEmpty(raced.value)
                  : kind === "definition"
                    ? definitionIsEmpty(raced.value)
                    : kind === "completion"
                      ? completionIsEmpty(raced.value)
                      : referencesIsEmpty(raced.value);
              verdict = empty ? "empty" : "ok";
              accounting.requestsAnswered += 1;
              if (empty) accounting.requestsEmpty += 1;
            }
            observations.push({
              kind,
              category: probe.category,
              ms,
              verdict,
              unexpectedEmpty: verdict === "empty" && !allowedEmpty.has(probe.category),
            });
            const stopReason = earlyStopReason();
            if (stopReason !== null) {
              earlyStop.stopped = true;
              earlyStop.reason = stopReason;
              log(`[corpus-gate:${route}] early stop (fast mode) — ${stopReason}`);
              break fileLoop;
            }
          }
        }
        refreshTreeRoots();
        if (!wedged && !(await checkLiveness(`after file ${accounting.filesOpened}`))) break;
        log(
          `[corpus-gate:${route}] ${accounting.filesOpened}/${sampleRelativePaths.length} files, ` +
            `${accounting.requestsSent} requests`,
        );
      }
      completed = !wedged && fatalError === null && !earlyStop.stopped && Date.now() <= deadline;
    } finally {
      // One last topology pass so a provider that only appeared late is still
      // attributed, then freeze the samplers.
      refreshTreeRoots();
      await treeSampler.refreshTopology().catch(() => undefined);
      treeSampler.stop();
      // Disposal is itself raced: teardown of a wedged server must not hang the gate.
      const disposal = await raceSettlement(handle.dispose(), 30_000);
      if (!disposal.settled) log(`[corpus-gate:${route}] dispose never settled (ignored)`);
    }
  }

  const elapsedMs = Date.now() - startedAt;
  const finalEarlyStop: CorpusRouteEarlyStop = { ...earlyStop };
  return {
    route,
    completed,
    wedged,
    wedgeDetail,
    fatalError,
    startup,
    accounting: { ...accounting },
    kinds: summarizeKinds(observations),
    memory: treeSampler.trends(),
    providerAttribution: treeSampler.attribution(),
    earlyStop: finalEarlyStop,
    // Fail-closed: the ORCHESTRATOR observes and stamps isolation. A route
    // runner never declares its own measurement valid.
    isolation: UNPROVEN_ISOLATION,
    liveness: { checks: livenessChecks, failures: livenessFailures },
    wallClock: {
      budgetMs: config.routeBudgetMs,
      elapsedMs,
      budgetExceeded: elapsedMs > config.routeBudgetMs,
    },
    sampleManifestHash: sampleManifestHash(sampleRelativePaths),
    ...(config.includeFileDetail ? { files: [...sampleRelativePaths] } : {}),
  };
}
