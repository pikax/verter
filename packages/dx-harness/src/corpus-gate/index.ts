/**
 * The corpus benchmark gate — public API.
 *
 * `runCorpusGate` orchestrates: deterministic sampling of the external corpus,
 * one bounded benchmark session per requested route (serially — routes share
 * the machine, parallel sessions would corrupt each other's latency), receipt
 * assembly, acceptance-bar evaluation, receipt emission, and (optionally) a
 * diff against a prior receipt. Each route session is additionally raced
 * against its wall-clock budget from OUT here so the gate can never wedge even
 * if a session's own bounds all fail.
 */
import { profileCorpus, selectRepresentativeSample } from "./sample.js";
import { runCorpusRoute, type RunCorpusRouteOptions } from "./session.js";
import { evaluateCorpusGate } from "./assertions.js";
import {
  compareCorpusReceipts,
  formatCompare,
  loadCorpusReceipt,
  writeCorpusReceipt,
  type CorpusCompareResult,
} from "./receipt.js";
import type {
  CorpusGateConfig,
  CorpusGateReceipt,
  CorpusGateRoute,
  CorpusRouteReport,
} from "./types.js";

export * from "./types.js";
export { CORPUS_GATE_DIR_ENV, resolveCorpusGateEnv, resolveThresholds } from "./config.js";
export {
  enumerateCorpusVueFiles,
  profileCorpus,
  profileCorpusFile,
  sampleManifestHash,
  selectRepresentativeSample,
  type CorpusFileFeatures,
  type CorpusFileProfile,
} from "./sample.js";
export { mineCorpusProbes, type CorpusProbe } from "./probes.js";
export {
  INTERACTIVE_THRESHOLD_MS,
  downsampleSeries,
  percentile,
  summarizeKind,
  summarizeKinds,
} from "./metrics.js";
export {
  completionIsEmpty,
  definitionIsEmpty,
  hoverIsEmpty,
  referencesIsEmpty,
} from "./verdicts.js";
export { spawnCorpusGateLsp, type CorpusGateLspHandle } from "./spawn.js";
export { runCorpusRoute, type RunCorpusRouteOptions } from "./session.js";
export {
  compareCorpusReceipts,
  corpusReceiptDestination,
  formatCompare,
  loadCorpusReceipt,
  writeCorpusReceipt,
  type CorpusCompareLine,
  type CorpusCompareResult,
} from "./receipt.js";
export { evaluateCorpusGate, evaluateRoute } from "./assertions.js";

/** A route runner — injectable so the hermetic unit suite needs no server. */
export type CorpusRouteRunner = (
  route: CorpusGateRoute,
  config: CorpusGateConfig,
  sampleRelativePaths: readonly string[],
  options?: RunCorpusRouteOptions,
) => Promise<CorpusRouteReport>;

export interface RunCorpusGateOptions {
  readonly runRoute?: CorpusRouteRunner;
  readonly log?: (message: string) => void;
}

export interface CorpusGateOutcome {
  readonly receipt: CorpusGateReceipt;
  readonly receiptPath: string;
  /** Acceptance-bar failures; empty ⇒ the bar passed. */
  readonly failures: readonly string[];
  /** Present when a baseline receipt was configured. */
  readonly compare: CorpusCompareResult | null;
  readonly compareText: readonly string[];
}

/** A route session gets its budget plus this teardown grace before being abandoned. */
const ROUTE_ABANDON_GRACE_MS = 90_000;

async function raceRoute(
  promise: Promise<CorpusRouteReport>,
  route: CorpusGateRoute,
  config: CorpusGateConfig,
  log: (message: string) => void,
): Promise<CorpusRouteReport> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const bound = new Promise<CorpusRouteReport | null>((resolve) => {
    timer = setTimeout(() => resolve(null), config.routeBudgetMs + ROUTE_ABANDON_GRACE_MS);
    timer.unref?.();
  });
  try {
    const settled = await Promise.race([promise, bound]);
    if (settled !== null) return settled;
    // The session runner itself never settled — the harness-level wedge. The
    // orphaned session's processes exit with this process; synthesize a report
    // so the receipt states exactly what happened instead of hanging.
    log(`[corpus-gate:${route}] route runner never settled — abandoned at the harness bound`);
    return {
      route,
      completed: false,
      wedged: true,
      wedgeDetail: `route runner never settled within budget+grace (${config.routeBudgetMs + ROUTE_ABANDON_GRACE_MS}ms)`,
      fatalError: null,
      startup: {
        initializeMs: 0,
        readyObserved: false,
        syncObserved: false,
        quiesced: false,
        settleMs: 0,
      },
      accounting: {
        requestsSent: 0,
        requestsAnswered: 0,
        requestsEmpty: 0,
        requestsTimedOut: 0,
        requestsErrored: 0,
        requestsAbandoned: 0,
        filesOpened: 0,
        filesSkipped: 0,
        probesMined: 0,
      },
      kinds: {
        hover: emptyKind(),
        definition: emptyKind(),
        completion: emptyKind(),
        references: emptyKind(),
      },
      memory: [],
      liveness: { checks: 0, failures: 0 },
      wallClock: {
        budgetMs: config.routeBudgetMs,
        elapsedMs: config.routeBudgetMs + ROUTE_ABANDON_GRACE_MS,
        budgetExceeded: true,
      },
      sampleManifestHash: "",
    };
  } finally {
    if (timer) clearTimeout(timer);
  }
}

function emptyKind() {
  return {
    count: 0,
    p50Ms: 0,
    p90Ms: 0,
    p95Ms: 0,
    maxMs: 0,
    over2500Count: 0,
    timeoutCount: 0,
    emptyCount: 0,
    unexpectedEmptyCount: 0,
    errorCount: 0,
  };
}

/** Run the all-route corpus gate end-to-end and emit the receipt. */
export async function runCorpusGate(
  config: CorpusGateConfig,
  options: RunCorpusGateOptions = {},
): Promise<CorpusGateOutcome> {
  const log = options.log ?? ((message: string) => console.log(message));
  const runRoute = options.runRoute ?? runCorpusRoute;

  const profiles = profileCorpus(config.corpusDir);
  const sample = selectRepresentativeSample(profiles, config.sampleSize);
  const sampleRelativePaths = sample.map((profile) => profile.relativePath);
  log(
    `[corpus-gate] ${config.corpusLabel}: ${profiles.length} .vue files discovered, ` +
      `${sampleRelativePaths.length} sampled, routes: ${config.routes.join(", ")}`,
  );

  const routes: Partial<Record<CorpusGateRoute, CorpusRouteReport>> = {};
  for (const route of config.routes) {
    log(`[corpus-gate] route ${route}: starting bounded session`);
    const report = await raceRoute(
      runRoute(route, config, sampleRelativePaths, { log }),
      route,
      config,
      log,
    );
    routes[route] = report;
    log(
      `[corpus-gate] route ${route}: done in ${report.wallClock.elapsedMs}ms — ` +
        `${report.accounting.requestsSent} requests, wedged=${report.wedged}, ` +
        `completed=${report.completed}`,
    );
  }

  const receiptBase: Omit<CorpusGateReceipt, "assertionFailures" | "pass"> = {
    schemaVersion: 1,
    harness: "corpus-gate",
    createdAt: new Date().toISOString(),
    corpusLabel: config.corpusLabel,
    corpus: { vueFileCount: profiles.length, sampledCount: sampleRelativePaths.length },
    config: {
      routes: config.routes,
      sampleSize: config.sampleSize,
      maxProbesPerFile: config.maxProbesPerFile,
      requestTimeoutMs: config.requestTimeoutMs,
      wedgeLivenessTimeoutMs: config.wedgeLivenessTimeoutMs,
      routeBudgetMs: config.routeBudgetMs,
      thresholds: config.thresholds,
    },
    routes,
  };
  const failures = evaluateCorpusGate(
    { ...receiptBase, assertionFailures: [], pass: false },
    config.routes,
    config.thresholds,
  );
  const receipt: CorpusGateReceipt = {
    ...receiptBase,
    assertionFailures: failures,
    pass: failures.length === 0,
  };
  const receiptPath = writeCorpusReceipt(receipt, config.receiptPath);

  let compare: CorpusCompareResult | null = null;
  let compareText: string[] = [];
  if (config.baselinePath !== null) {
    const baseline = loadCorpusReceipt(config.baselinePath);
    compare = compareCorpusReceipts(baseline, receipt);
    compareText = formatCompare(compare);
    for (const line of compareText) log(`[corpus-gate:compare] ${line}`);
  }
  return { receipt, receiptPath, failures, compare, compareText };
}
