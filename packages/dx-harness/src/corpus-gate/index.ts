/**
 * The corpus benchmark gate — public API.
 *
 * `runCorpusGate` orchestrates: deterministic sampling of the external corpus,
 * one bounded benchmark session per requested route, receipt assembly,
 * acceptance-bar evaluation, receipt emission, and (optionally) a diff against
 * a prior receipt. Each route session is additionally raced against its
 * wall-clock budget from OUT here so the gate can never wedge even if a
 * session's own bounds all fail.
 *
 * Routes run SERIALLY by default: they are independent sessions, but three
 * language servers plus their providers on one box distort exactly the p95
 * this gate exists to protect. `parallel` is opt-in, capability-gated, and
 * records every route it overlapped as CONTENDED — latency then reports as
 * advisory while every other assertion keeps gating. The way to buy wall clock
 * WITHOUT losing latency fidelity is the CI fan-out: one route per machine,
 * each running a single-route serial gate on a dedicated executor.
 *
 * Isolation is stamped HERE, from the orchestrator's own observation of how
 * many sessions were in flight during each route's window — a route runner
 * cannot declare its own measurement valid.
 */
import { profileCorpus, selectRepresentativeSample } from "./sample.js";
import { runCorpusRoute, type RunCorpusRouteOptions } from "./session.js";
import { evaluateCorpusGate, evaluateCorpusGateAdvisories } from "./assertions.js";
import {
  RouteConcurrencyTracker,
  UNPROVEN_ISOLATION,
  classifyIsolation,
  probeMachineCapability,
  resolveExecutionTopology,
} from "./topology.js";
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
  CorpusMachineCapability,
  CorpusRouteReport,
} from "./types.js";

export * from "./types.js";
export {
  CORPUS_GATE_DIR_ENV,
  DEFAULT_GATE_BUDGET_MS,
  resolveCorpusGateEnv,
  resolveThresholds,
} from "./config.js";
export {
  MIN_CORES_PER_PARALLEL_ROUTE,
  MIN_MEMORY_BYTES_PER_PARALLEL_ROUTE,
  RouteConcurrencyTracker,
  UNPROVEN_ISOLATION,
  classifyIsolation,
  probeMachineCapability,
  resolveExecutionTopology,
  type IsolationObservation,
  type TopologyResolution,
} from "./topology.js";
export {
  ProcessTreeSampler,
  classifyProviderAttribution,
  descendantPids,
  processImage,
  snapshotProcessTable,
  unsampledTreeMembers,
  type AttributionInput,
  type ProcessRow,
  type ProcessTreeRoots,
} from "./processTree.js";
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
export {
  evaluateCorpusGate,
  evaluateCorpusGateAdvisories,
  evaluateRoute,
  evaluateRouteAdvisories,
  reportIsolation,
} from "./assertions.js";
export {
  formatShardSummary,
  summarizeShards,
  type ShardSummary,
  type ShardWallClock,
} from "./shards.js";

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
  /**
   * Machine resources override. Production probes the real machine; the
   * hermetic suite injects a capability so the topology decision under test is
   * the code's, not the test runner's core count.
   */
  readonly machine?: CorpusMachineCapability;
}

export interface CorpusGateOutcome {
  readonly receipt: CorpusGateReceipt;
  readonly receiptPath: string;
  /** Acceptance-bar failures; empty ⇒ the bar passed. */
  readonly failures: readonly string[];
  /** Recorded-but-NOT-gating observations (every line prefixed `ADVISORY`). */
  readonly advisories: readonly string[];
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
      providerAttribution: {
        status: "missing",
        providerPid: null,
        detail: "the route runner never settled — no provider was ever observed",
        unattributedPids: [],
        sampledProcessCount: 0,
      },
      earlyStop: { enabled: config.fastMode, stopped: false, reason: null },
      isolation: UNPROVEN_ISOLATION,
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

  const machine = options.machine ?? probeMachineCapability();
  const topology = resolveExecutionTopology(config.topology, config.routes.length, machine);
  if (topology.downgradeReason !== null) {
    log(
      `[corpus-gate] topology ${config.topology} DOWNGRADED to ${topology.effective}: ` +
        `${topology.downgradeReason}`,
    );
  }
  log(
    `[corpus-gate] topology=${topology.effective} executor=${config.executor} ` +
      `machine=${machine.cpuCount} cores / ${Math.round(machine.totalMemBytes / 1024 ** 3)} GiB` +
      (config.fastMode ? " fastMode=ON (census deliberately incomplete on failure)" : ""),
  );

  const tracker = new RouteConcurrencyTracker();
  const gateStartedAt = Date.now();
  const runOne = async (route: CorpusGateRoute): Promise<CorpusRouteReport> => {
    const release = tracker.start(route);
    log(`[corpus-gate] route ${route}: starting bounded session`);
    try {
      const report = await raceRoute(
        runRoute(route, config, sampleRelativePaths, { log }),
        route,
        config,
        log,
      );
      log(
        `[corpus-gate] route ${route}: done in ${report.wallClock.elapsedMs}ms — ` +
          `${report.accounting.requestsSent} requests, wedged=${report.wedged}, ` +
          `completed=${report.completed}`,
      );
      return report;
    } finally {
      release();
    }
  };

  const routes: Partial<Record<CorpusGateRoute, CorpusRouteReport>> = {};
  if (topology.effective === "parallel") {
    const settled = await Promise.all(config.routes.map((route) => runOne(route)));
    for (const report of settled) routes[report.route] = report;
  } else {
    for (const route of config.routes) {
      routes[route] = await runOne(route);
    }
  }

  // Isolation is stamped from what the ORCHESTRATOR observed, overwriting the
  // route runner's fail-closed placeholder. A runner cannot vouch for itself.
  for (const route of config.routes) {
    const report = routes[route];
    if (!report) continue;
    const isolation = classifyIsolation({
      topology: topology.effective,
      executor: config.executor,
      observedConcurrentRoutes: tracker.peakFor(route),
    });
    routes[route] = { ...report, isolation };
    log(
      `[corpus-gate] route ${route}: latency ${isolation.latencyGating ? "GATING" : "ADVISORY"} ` +
        `(${isolation.mode}) — ${isolation.evidence}`,
    );
  }

  const gateElapsedMs = Date.now() - gateStartedAt;
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
    execution: {
      requestedTopology: topology.requested,
      effectiveTopology: topology.effective,
      downgradeReason: topology.downgradeReason,
      executor: config.executor,
      machine,
      requireIsolatedLatency: config.requireIsolatedLatency,
      fastMode: config.fastMode,
      latencyGatingRoutes: config.routes.filter(
        (route) => routes[route]?.isolation?.latencyGating === true,
      ),
      latencyAdvisoryRoutes: config.routes.filter(
        (route) => routes[route] !== undefined && routes[route]?.isolation?.latencyGating !== true,
      ),
    },
    budget: {
      targetMs: config.gateBudgetMs,
      actualMs: gateElapsedMs,
      exceeded: gateElapsedMs > config.gateBudgetMs,
      fatal: config.gateBudgetFatal,
    },
  };
  const draft: CorpusGateReceipt = { ...receiptBase, assertionFailures: [], pass: false };
  const failures = evaluateCorpusGate(draft, config.routes, config.thresholds, {
    requireIsolatedLatency: config.requireIsolatedLatency,
  });
  const advisories = evaluateCorpusGateAdvisories(draft, config.routes, config.thresholds);
  const receipt: CorpusGateReceipt = {
    ...receiptBase,
    assertionFailures: failures,
    pass: failures.length === 0,
    advisories,
  };
  const receiptPath = writeCorpusReceipt(receipt, config.receiptPath);
  for (const advisory of advisories) log(`[corpus-gate] ${advisory}`);
  log(
    `[corpus-gate] wall clock ${gateElapsedMs}ms vs ${config.gateBudgetMs}ms budget ` +
      `(${gateElapsedMs > config.gateBudgetMs ? "OVER" : "within"}${config.gateBudgetFatal ? ", fatal" : ", reported"})`,
  );

  let compare: CorpusCompareResult | null = null;
  let compareText: string[] = [];
  if (config.baselinePath !== null) {
    const baseline = loadCorpusReceipt(config.baselinePath);
    compare = compareCorpusReceipts(baseline, receipt);
    compareText = formatCompare(compare);
    for (const line of compareText) log(`[corpus-gate:compare] ${line}`);
  }
  return { receipt, receiptPath, failures, advisories, compare, compareText };
}
