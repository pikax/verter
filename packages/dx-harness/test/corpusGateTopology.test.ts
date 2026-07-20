/**
 * Execution topology, measurement isolation, and the fidelity-gated bar.
 *
 * The invariant under test: latency percentiles gate ONLY where they were
 * measured in isolation, while stability, wedge, liveness, unexpected-empty,
 * accounting, memory and attribution gate in BOTH modes. A contended run must
 * be unable to emit a gating latency verdict, and an isolated one must still
 * emit it — both directions are asserted, and an advisory line can never be
 * read as a gating failure.
 *
 * Hermetic: injected route runners and an injected machine capability, so the
 * topology decision under test is the code's, never the test runner's core
 * count. No server spawn, no external corpus.
 */
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, describe, expect, it } from "vitest";

import {
  evaluateCorpusGate,
  evaluateCorpusGateAdvisories,
  evaluateRoute,
  evaluateRouteAdvisories,
  reportIsolation,
} from "../src/corpus-gate/assertions.js";
import { CORPUS_GATE_DIR_ENV, resolveCorpusGateEnv } from "../src/corpus-gate/config.js";
import { runCorpusGate } from "../src/corpus-gate/index.js";
import { sampleManifestHash } from "../src/corpus-gate/sample.js";
import {
  MIN_CORES_PER_PARALLEL_ROUTE,
  RouteConcurrencyTracker,
  UNPROVEN_ISOLATION,
  classifyIsolation,
  resolveExecutionTopology,
} from "../src/corpus-gate/topology.js";
import type {
  CorpusGateConfig,
  CorpusGateRoute,
  CorpusKindSummary,
  CorpusMachineCapability,
  CorpusRouteReport,
} from "../src/corpus-gate/types.js";

const SYNTHETIC_CORPUS = fileURLToPath(
  new URL("./fixtures/corpus-gate-synthetic", import.meta.url),
);
const ALL_ROUTES: readonly CorpusGateRoute[] = ["tsserver", "tsgo", "shared-tsgo"];

/** A machine big enough for 3 isolated concurrent route sessions. */
const BIG_MACHINE: CorpusMachineCapability = {
  cpuCount: 3 * MIN_CORES_PER_PARALLEL_ROUTE,
  totalMemBytes: 3 * 4 * 1024 * 1024 * 1024,
};
/** A machine that cannot isolate 3 concurrent route sessions. */
const SMALL_MACHINE: CorpusMachineCapability = {
  cpuCount: 4,
  totalMemBytes: 8 * 1024 * 1024 * 1024,
};

const tempDirs: string[] = [];
function tempDir(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "corpus-gate-topology-"));
  tempDirs.push(dir);
  return dir;
}
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

function testConfig(overrides: Partial<CorpusGateConfig> = {}): CorpusGateConfig {
  return {
    corpusDir: SYNTHETIC_CORPUS,
    corpusLabel: "Corpus A",
    routes: ALL_ROUTES,
    topology: "serial",
    executor: "unattested",
    requireIsolatedLatency: false,
    gateBudgetMs: 20 * 60_000,
    gateBudgetFatal: false,
    fastMode: false,
    sampleSize: 6,
    maxProbesPerFile: 24,
    requestTimeoutMs: 15_000,
    wedgeLivenessTimeoutMs: 10_000,
    routeBudgetMs: 60_000,
    startupReadyCapMs: 1_000,
    startupSettleCapMs: 1_000,
    openSettleCapMs: 1_000,
    rssSampleIntervalMs: 1_000,
    receiptPath: null,
    baselinePath: null,
    includeFileDetail: false,
    thresholds: {
      hoverP95Ms: 300,
      definitionP95Ms: 500,
      completionP95Ms: 500,
      referencesP95Ms: 800,
      rssMaxBytes: 4 * 1024 * 1024 * 1024,
      allowedEmptyCategories: ["classToken"],
    },
    ...overrides,
  };
}

function kindSummary(overrides: Partial<CorpusKindSummary> = {}): CorpusKindSummary {
  return {
    count: 4,
    p50Ms: 20,
    p90Ms: 40,
    p95Ms: 50,
    maxMs: 60,
    over2500Count: 0,
    timeoutCount: 0,
    emptyCount: 0,
    unexpectedEmptyCount: 0,
    errorCount: 0,
    ...overrides,
  };
}

function report(
  route: CorpusGateRoute,
  sample: readonly string[],
  overrides: Partial<CorpusRouteReport> = {},
): CorpusRouteReport {
  return {
    route,
    completed: true,
    wedged: false,
    wedgeDetail: null,
    fatalError: null,
    startup: {
      initializeMs: 50,
      readyObserved: true,
      syncObserved: true,
      quiesced: true,
      settleMs: 400,
    },
    accounting: {
      requestsSent: 16,
      requestsAnswered: 16,
      requestsEmpty: 0,
      requestsTimedOut: 0,
      requestsErrored: 0,
      requestsAbandoned: 0,
      filesOpened: sample.length,
      filesSkipped: 0,
      probesMined: 10,
    },
    kinds: {
      hover: kindSummary(),
      definition: kindSummary(),
      completion: kindSummary(),
      references: kindSummary(),
    },
    memory: [
      {
        label: "verter-lsp",
        pid: 4242,
        supported: true,
        sampleCount: 3,
        firstRssBytes: 100_000_000,
        lastRssBytes: 120_000_000,
        maxRssBytes: 130_000_000,
        samples: [],
        role: "server",
        parentPid: 1,
        image: "verter-lsp",
      },
    ],
    providerAttribution: {
      status: "verified",
      providerPid: 4243,
      detail: "provider pid 4243 is a descendant of the spawned tree",
      unattributedPids: [],
      sampledProcessCount: 2,
    },
    earlyStop: { enabled: false, stopped: false, reason: null },
    isolation: {
      topology: "serial",
      executor: "unattested",
      mode: "isolated",
      observedConcurrentRoutes: 1,
      latencyGating: true,
      attestationContradicted: false,
      evidence: "sole route session in flight on this executor",
    },
    liveness: { checks: sample.length, failures: 0 },
    wallClock: { budgetMs: 60_000, elapsedMs: 2_000, budgetExceeded: false },
    sampleManifestHash: sampleManifestHash(sample),
    ...overrides,
  };
}

const SAMPLE = ["src/components/BaseButton.vue"];

/** A report whose hover p95 breaches the bar, in the given isolation mode. */
function slowReport(latencyGating: boolean): CorpusRouteReport {
  return report("tsgo", SAMPLE, {
    kinds: {
      hover: kindSummary({ p95Ms: 9_000 }),
      definition: kindSummary(),
      completion: kindSummary(),
      references: kindSummary(),
    },
    isolation: latencyGating
      ? {
          topology: "serial",
          executor: "dedicated",
          mode: "isolated",
          observedConcurrentRoutes: 1,
          latencyGating: true,
          attestationContradicted: false,
          evidence: "sole route session in flight on this executor",
        }
      : {
          topology: "parallel",
          executor: "unattested",
          mode: "contended",
          observedConcurrentRoutes: 3,
          latencyGating: false,
          attestationContradicted: false,
          evidence: "3 route sessions were in flight on this executor",
        },
  });
}

describe("topology resolution", () => {
  it("keeps serial as the default and never downgrades it", () => {
    expect(resolveExecutionTopology("serial", 3, SMALL_MACHINE)).toEqual({
      requested: "serial",
      effective: "serial",
      downgradeReason: null,
    });
  });

  it("honours parallel only on a machine that can isolate every session", () => {
    const big = resolveExecutionTopology("parallel", 3, BIG_MACHINE);
    expect(big.effective).toBe("parallel");
    expect(big.downgradeReason).toBeNull();

    const small = resolveExecutionTopology("parallel", 3, SMALL_MACHINE);
    expect(small.effective).toBe("serial");
    expect(small.downgradeReason).toContain("cannot isolate 3 concurrent route sessions");
  });

  it("downgrades parallel for a single route (nothing to overlap)", () => {
    const single = resolveExecutionTopology("parallel", 1, BIG_MACHINE);
    expect(single.effective).toBe("serial");
    expect(single.downgradeReason).toContain("meaningless");
  });
});

describe("isolation classification", () => {
  it("gates latency for a sole session on an unshared executor", () => {
    const isolation = classifyIsolation({
      topology: "serial",
      executor: "unattested",
      observedConcurrentRoutes: 1,
    });
    expect(isolation.mode).toBe("isolated");
    expect(isolation.latencyGating).toBe(true);
    expect(isolation.evidence).toContain("sole route session");
  });

  it("refuses to gate latency when sessions overlapped", () => {
    const isolation = classifyIsolation({
      topology: "parallel",
      executor: "unattested",
      observedConcurrentRoutes: 3,
    });
    expect(isolation.mode).toBe("contended");
    expect(isolation.latencyGating).toBe(false);
    expect(isolation.observedConcurrentRoutes).toBe(3);
  });

  it("records a dedicated attestation refuted by observed concurrency as a contradiction", () => {
    const isolation = classifyIsolation({
      topology: "parallel",
      executor: "dedicated",
      observedConcurrentRoutes: 2,
    });
    expect(isolation.attestationContradicted).toBe(true);
    expect(isolation.latencyGating).toBe(false);
    expect(isolation.evidence).toContain("CONTRADICTS");
  });

  it("refuses to gate a declared-shared executor even when alone", () => {
    const isolation = classifyIsolation({
      topology: "serial",
      executor: "shared",
      observedConcurrentRoutes: 1,
    });
    expect(isolation.mode).toBe("contended");
    expect(isolation.latencyGating).toBe(false);
  });

  it("refuses to gate parallel topology without a dedicated-executor attestation", () => {
    const isolation = classifyIsolation({
      topology: "parallel",
      executor: "unattested",
      observedConcurrentRoutes: 1,
    });
    expect(isolation.latencyGating).toBe(false);
    expect(isolation.evidence).toContain("isolation is unproven");
  });

  it("gates a single-route parallel shard on a dedicated executor (the CI fan-out)", () => {
    const isolation = classifyIsolation({
      topology: "parallel",
      executor: "dedicated",
      observedConcurrentRoutes: 1,
    });
    expect(isolation.mode).toBe("isolated");
    expect(isolation.latencyGating).toBe(true);
  });

  it("fails closed for a report that recorded no isolation at all", () => {
    expect(UNPROVEN_ISOLATION.latencyGating).toBe(false);
    const { isolation: _dropped, ...withoutIsolation } = report("tsgo", SAMPLE);
    expect(reportIsolation(withoutIsolation as CorpusRouteReport).latencyGating).toBe(false);
  });
});

describe("concurrency tracker", () => {
  it("records 1 for strictly serial sessions", () => {
    const tracker = new RouteConcurrencyTracker();
    for (const route of ALL_ROUTES) {
      const release = tracker.start(route);
      release();
    }
    for (const route of ALL_ROUTES) expect(tracker.peakFor(route)).toBe(1);
  });

  it("raises the peak of every session already in flight when another starts", () => {
    const tracker = new RouteConcurrencyTracker();
    const releaseA = tracker.start("tsserver");
    const releaseB = tracker.start("tsgo");
    releaseB();
    const releaseC = tracker.start("shared-tsgo");
    releaseA();
    releaseC();
    expect(tracker.peakFor("tsserver")).toBe(2);
    expect(tracker.peakFor("tsgo")).toBe(2);
    expect(tracker.peakFor("shared-tsgo")).toBe(2);
  });
});

describe("fidelity-gated latency bar", () => {
  const thresholds = testConfig().thresholds;

  it("gates a p95 breach measured in isolation", () => {
    const failures = evaluateRoute(slowReport(true), thresholds);
    expect(failures.some((failure) => failure.includes("hover p95 9000ms"))).toBe(true);
    expect(evaluateRouteAdvisories(slowReport(true), thresholds)).toEqual([]);
  });

  it("does NOT gate the same breach measured under contention — it advises", () => {
    const contended = slowReport(false);
    const failures = evaluateRoute(contended, thresholds);
    expect(failures.some((failure) => failure.includes("p95"))).toBe(false);
    const advisories = evaluateRouteAdvisories(contended, thresholds);
    expect(advisories.some((line) => line.includes("hover p95 9000ms"))).toBe(true);
    // Impossible to mistake for a gating verdict: every advisory says so.
    for (const line of advisories) expect(line.startsWith("ADVISORY ")).toBe(true);
    expect(advisories.some((line) => line.includes("NOT gating"))).toBe(true);
  });

  it("keeps stability, wedge, empty, accounting and memory gating under contention", () => {
    const contendedIsolation = slowReport(false).isolation;
    const broken = report("tsgo", SAMPLE, {
      isolation: contendedIsolation,
      wedged: true,
      wedgeDetail: "request never settled",
      fatalError: "provider died",
      wallClock: { budgetMs: 60_000, elapsedMs: 61_000, budgetExceeded: true },
      kinds: {
        hover: kindSummary({ unexpectedEmptyCount: 2 }),
        definition: kindSummary(),
        completion: kindSummary(),
        references: kindSummary(),
      },
      accounting: {
        requestsSent: 10,
        requestsAnswered: 4,
        requestsEmpty: 9,
        requestsTimedOut: 1,
        requestsErrored: 0,
        requestsAbandoned: 0,
        filesOpened: 1,
        filesSkipped: 0,
        probesMined: 5,
      },
      memory: [
        {
          label: "provider",
          pid: 77,
          supported: true,
          sampleCount: 2,
          firstRssBytes: 1,
          lastRssBytes: 2,
          maxRssBytes: thresholds.rssMaxBytes + 1,
          samples: [],
          role: "provider",
          parentPid: 4242,
          image: "node",
        },
      ],
    });
    const failures = evaluateRoute(broken, thresholds);
    for (const expected of [
      "WEDGED",
      "fatal route error",
      "wall-clock budget",
      "unexpected empty result",
      "accounting identity violated",
      "exceeds the",
    ]) {
      expect(
        failures.some((failure) => failure.includes(expected)),
        `${expected} must still gate under contention`,
      ).toBe(true);
    }
  });

  it("fails a non-gating route when isolation is REQUIRED", () => {
    const contended = slowReport(false);
    expect(evaluateRoute(contended, thresholds, { requireIsolatedLatency: true })).toContainEqual(
      expect.stringContaining("isolation was REQUIRED"),
    );
    // …and does not fire that failure for an isolated route.
    expect(
      evaluateRoute(slowReport(true), thresholds, { requireIsolatedLatency: true }).some(
        (failure) => failure.includes("isolation was REQUIRED"),
      ),
    ).toBe(false);
  });

  it("fails a contradicted dedicated-executor attestation", () => {
    const contradicted = report("tsgo", SAMPLE, {
      isolation: {
        topology: "parallel",
        executor: "dedicated",
        mode: "contended",
        observedConcurrentRoutes: 3,
        latencyGating: false,
        attestationContradicted: true,
        evidence: "3 concurrent sessions observed",
      },
    });
    expect(evaluateRoute(contradicted, thresholds)).toContainEqual(
      expect.stringContaining("isolation attestation CONTRADICTED"),
    );
  });
});

describe("end-to-end topologies (hermetic, injected runner)", () => {
  async function run(overrides: Partial<CorpusGateConfig>, machine: CorpusMachineCapability) {
    const receiptPath = path.join(tempDir(), "receipt.json");
    const started = new Set<CorpusGateRoute>();
    let maxOverlap = 0;
    return {
      outcome: await runCorpusGate(testConfig({ receiptPath, ...overrides }), {
        log: () => {},
        machine,
        runRoute: async (route, _config, sample) => {
          started.add(route);
          maxOverlap = Math.max(maxOverlap, started.size);
          await new Promise((resolve) => setTimeout(resolve, 60));
          started.delete(route);
          return report(route, sample);
        },
      }),
      overlap: () => maxOverlap,
    };
  }

  it("serial: every route measured alone, latency GATING, receipt says so", async () => {
    const { outcome, overlap } = await run({ topology: "serial" }, BIG_MACHINE);
    expect(overlap()).toBe(1);
    expect(outcome.receipt.execution?.effectiveTopology).toBe("serial");
    for (const route of ALL_ROUTES) {
      const isolation = outcome.receipt.routes[route]?.isolation;
      expect(isolation?.mode).toBe("isolated");
      expect(isolation?.latencyGating).toBe(true);
      expect(isolation?.observedConcurrentRoutes).toBe(1);
    }
    expect(outcome.receipt.execution?.latencyGatingRoutes).toEqual([...ALL_ROUTES]);
    expect(outcome.receipt.execution?.latencyAdvisoryRoutes).toEqual([]);
    expect(outcome.failures).toEqual([]);
    expect(outcome.receipt.pass).toBe(true);
  });

  it("parallel: sessions really overlap and every route is recorded CONTENDED", async () => {
    const { outcome, overlap } = await run({ topology: "parallel" }, BIG_MACHINE);
    expect(overlap()).toBeGreaterThan(1);
    expect(outcome.receipt.execution?.effectiveTopology).toBe("parallel");
    for (const route of ALL_ROUTES) {
      const isolation = outcome.receipt.routes[route]?.isolation;
      expect(isolation?.mode).toBe("contended");
      expect(isolation?.latencyGating).toBe(false);
      expect(isolation?.observedConcurrentRoutes).toBeGreaterThan(1);
    }
    expect(outcome.receipt.execution?.latencyGatingRoutes).toEqual([]);
    expect(outcome.receipt.execution?.latencyAdvisoryRoutes).toEqual([...ALL_ROUTES]);
    expect(outcome.advisories.some((line) => line.includes("NOT gating"))).toBe(true);
  });

  it("a contended run cannot emit a gating latency failure; an isolated one can", async () => {
    const slowRunner = async (route: CorpusGateRoute, sample: readonly string[]) =>
      report(route, sample, {
        kinds: {
          hover: kindSummary({ p95Ms: 9_000 }),
          definition: kindSummary(),
          completion: kindSummary(),
          references: kindSummary(),
        },
      });

    const contended = await runCorpusGate(
      testConfig({ topology: "parallel", receiptPath: path.join(tempDir(), "c.json") }),
      {
        log: () => {},
        machine: BIG_MACHINE,
        runRoute: async (route, _config, sample) => slowRunner(route, sample),
      },
    );
    expect(contended.failures.some((failure) => failure.includes("p95"))).toBe(false);
    expect(contended.receipt.pass).toBe(true);
    expect(contended.advisories.some((line) => line.includes("hover p95 9000ms"))).toBe(true);

    const isolated = await runCorpusGate(
      testConfig({ topology: "serial", receiptPath: path.join(tempDir(), "i.json") }),
      {
        log: () => {},
        machine: BIG_MACHINE,
        runRoute: async (route, _config, sample) => slowRunner(route, sample),
      },
    );
    expect(
      isolated.failures.filter((failure) => failure.includes("hover p95 9000ms")),
    ).toHaveLength(3);
    expect(isolated.receipt.pass).toBe(false);
  });

  it("a parallel request on a machine that cannot isolate DOWNGRADES to serial and still gates", async () => {
    const { outcome, overlap } = await run({ topology: "parallel" }, SMALL_MACHINE);
    expect(overlap()).toBe(1);
    expect(outcome.receipt.execution?.requestedTopology).toBe("parallel");
    expect(outcome.receipt.execution?.effectiveTopology).toBe("serial");
    expect(outcome.receipt.execution?.downgradeReason).toContain("cannot isolate");
    for (const route of ALL_ROUTES) {
      expect(outcome.receipt.routes[route]?.isolation?.latencyGating).toBe(true);
    }
  });

  it("verifies IDENTICAL work in both topologies — no sample, probe or request is dropped", async () => {
    const serial = await run({ topology: "serial" }, BIG_MACHINE);
    const parallel = await run({ topology: "parallel" }, BIG_MACHINE);
    for (const route of ALL_ROUTES) {
      const before = serial.outcome.receipt.routes[route];
      const after = parallel.outcome.receipt.routes[route];
      expect(after?.sampleManifestHash).toBe(before?.sampleManifestHash);
      expect(after?.accounting).toEqual(before?.accounting);
      expect(after?.kinds).toEqual(before?.kinds);
    }
    expect(parallel.outcome.receipt.corpus).toEqual(serial.outcome.receipt.corpus);
    expect(parallel.outcome.receipt.config.sampleSize).toBe(
      serial.outcome.receipt.config.sampleSize,
    );
    expect(parallel.outcome.receipt.config.maxProbesPerFile).toBe(
      serial.outcome.receipt.config.maxProbesPerFile,
    );
  });

  it("records isolation as first-class receipt data the JSON actually carries", async () => {
    const { outcome } = await run({ topology: "serial", executor: "dedicated" }, BIG_MACHINE);
    const raw = JSON.parse(readFileSync(outcome.receiptPath, "utf8")) as Record<string, unknown>;
    const routes = raw.routes as Record<string, { isolation?: Record<string, unknown> }>;
    expect(routes.tsgo.isolation).toMatchObject({
      topology: "serial",
      executor: "dedicated",
      mode: "isolated",
      latencyGating: true,
      observedConcurrentRoutes: 1,
    });
    expect(typeof (routes.tsgo.isolation as { evidence: string }).evidence).toBe("string");
    expect(raw.execution).toMatchObject({ executor: "dedicated", effectiveTopology: "serial" });
  });

  it("an injected runner cannot forge its own isolation — the orchestrator overwrites it", async () => {
    const receiptPath = path.join(tempDir(), "forged.json");
    const outcome = await runCorpusGate(testConfig({ receiptPath, topology: "parallel" }), {
      log: () => {},
      machine: BIG_MACHINE,
      runRoute: async (route, _config, sample) =>
        report(route, sample, {
          // A route runner claiming its own contended measurement is gating.
          isolation: {
            topology: "serial",
            executor: "dedicated",
            mode: "isolated",
            observedConcurrentRoutes: 1,
            latencyGating: true,
            attestationContradicted: false,
            evidence: "forged by the route runner",
          },
        }),
    });
    for (const route of ALL_ROUTES) {
      const isolation = outcome.receipt.routes[route]?.isolation;
      expect(isolation?.evidence).not.toContain("forged");
      expect(isolation?.latencyGating).toBe(false);
    }
  });
});

describe("whole-gate budget", () => {
  it("records target vs actual and advises (not fails) on a breach", async () => {
    const receiptPath = path.join(tempDir(), "budget.json");
    const outcome = await runCorpusGate(
      testConfig({ receiptPath, gateBudgetMs: 1, routes: ["tsgo"] }),
      {
        log: () => {},
        machine: BIG_MACHINE,
        runRoute: async (route, _config, sample) => {
          await new Promise((resolve) => setTimeout(resolve, 30));
          return report(route, sample);
        },
      },
    );
    expect(outcome.receipt.budget?.targetMs).toBe(1);
    expect(outcome.receipt.budget?.actualMs).toBeGreaterThan(1);
    expect(outcome.receipt.budget?.exceeded).toBe(true);
    expect(outcome.receipt.budget?.fatal).toBe(false);
    expect(outcome.failures.some((failure) => failure.includes("budget"))).toBe(false);
    expect(outcome.advisories.some((line) => line.includes("exceeds the"))).toBe(true);
    expect(outcome.receipt.pass).toBe(true);
  });

  it("promotes a breach to a gating failure when opted in", () => {
    const receipt = {
      schemaVersion: 1 as const,
      harness: "corpus-gate" as const,
      createdAt: new Date().toISOString(),
      corpusLabel: "Corpus A",
      corpus: { vueFileCount: 6, sampledCount: 1 },
      config: {
        routes: ["tsgo"] as CorpusGateRoute[],
        sampleSize: 6,
        maxProbesPerFile: 24,
        requestTimeoutMs: 1,
        wedgeLivenessTimeoutMs: 1,
        routeBudgetMs: 1,
        thresholds: testConfig().thresholds,
      },
      routes: { tsgo: report("tsgo", SAMPLE) },
      assertionFailures: [],
      pass: false,
      budget: { targetMs: 10, actualMs: 99, exceeded: true, fatal: true },
    };
    expect(evaluateCorpusGate(receipt, ["tsgo"], testConfig().thresholds)).toContainEqual(
      expect.stringContaining("exceeds the 10ms budget"),
    );
    expect(
      evaluateCorpusGateAdvisories(receipt, ["tsgo"], testConfig().thresholds).some((line) =>
        line.includes("budget"),
      ),
    ).toBe(false);
  });
});

describe("opt-in fast mode", () => {
  it("is OFF by default and ON only via the explicit env flag", () => {
    const off = resolveCorpusGateEnv({ [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS });
    expect(off.kind === "run" && off.config.fastMode).toBe(false);
    const on = resolveCorpusGateEnv({
      [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS,
      VERTER_CORPUS_GATE_FAST: "1",
    });
    expect(on.kind === "run" && on.config.fastMode).toBe(true);
  });

  it("says so in the receipt and advises that the census is incomplete", async () => {
    const receiptPath = path.join(tempDir(), "fast.json");
    const outcome = await runCorpusGate(
      testConfig({ receiptPath, fastMode: true, routes: ["tsgo"] }),
      {
        log: () => {},
        machine: BIG_MACHINE,
        runRoute: async (route, _config, sample) =>
          report(route, sample, {
            completed: false,
            earlyStop: { enabled: true, stopped: true, reason: "unexpected empty result" },
            kinds: {
              hover: kindSummary({ unexpectedEmptyCount: 1 }),
              definition: kindSummary({ count: 0 }),
              completion: kindSummary({ count: 0 }),
              references: kindSummary({ count: 0 }),
            },
          }),
      },
    );
    expect(outcome.receipt.execution?.fastMode).toBe(true);
    expect(outcome.advisories.some((line) => line.includes("census counts are deliberately"))).toBe(
      true,
    );
    // The failure that decided the verdict still gates…
    expect(outcome.failures.some((failure) => failure.includes("unexpected empty"))).toBe(true);
    // …and the kinds it never got to measure are not re-reported as vacuity.
    expect(outcome.failures.some((failure) => failure.includes("vacuous kind"))).toBe(false);
  });

  it("still reports vacuous kinds when the route was NOT stopped early", () => {
    const idle = report("tsgo", SAMPLE, {
      kinds: {
        hover: kindSummary(),
        definition: kindSummary({ count: 0 }),
        completion: kindSummary({ count: 0 }),
        references: kindSummary({ count: 0 }),
      },
    });
    expect(
      evaluateRoute(idle, testConfig().thresholds).filter((failure) =>
        failure.includes("vacuous kind"),
      ),
    ).toHaveLength(3);
  });
});

describe("config resolution for the new knobs", () => {
  it("defaults to serial / unattested / non-required isolation", () => {
    const resolution = resolveCorpusGateEnv({ [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS });
    expect(resolution.kind).toBe("run");
    if (resolution.kind !== "run") return;
    expect(resolution.config.topology).toBe("serial");
    expect(resolution.config.executor).toBe("unattested");
    expect(resolution.config.requireIsolatedLatency).toBe(false);
    expect(resolution.config.gateBudgetMs).toBe(20 * 60_000);
    expect(resolution.config.gateBudgetFatal).toBe(false);
  });

  it("parses the CI fan-out shard configuration", () => {
    const resolution = resolveCorpusGateEnv({
      [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS,
      VERTER_CORPUS_GATE_ROUTES: "tsgo",
      VERTER_CORPUS_GATE_EXECUTOR: "dedicated",
      VERTER_CORPUS_GATE_REQUIRE_ISOLATION: "1",
      VERTER_CORPUS_GATE_BUDGET_MS: "600000",
    });
    expect(resolution.kind).toBe("run");
    if (resolution.kind !== "run") return;
    expect(resolution.config.routes).toEqual(["tsgo"]);
    expect(resolution.config.executor).toBe("dedicated");
    expect(resolution.config.requireIsolatedLatency).toBe(true);
    expect(resolution.config.gateBudgetMs).toBe(600_000);
  });

  it("rejects an unknown topology or executor loudly", () => {
    expect(() =>
      resolveCorpusGateEnv({
        [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS,
        VERTER_CORPUS_GATE_TOPOLOGY: "sharded",
      }),
    ).toThrow(/VERTER_CORPUS_GATE_TOPOLOGY/);
    expect(() =>
      resolveCorpusGateEnv({
        [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS,
        VERTER_CORPUS_GATE_EXECUTOR: "probably-fine",
      }),
    ).toThrow(/VERTER_CORPUS_GATE_EXECUTOR/);
  });
});
