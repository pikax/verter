/**
 * Fan-out shard aggregation: the place a distributed gate loses coverage.
 *
 * The CI topology runs one route per machine. That buys wall clock only if the
 * recombination is paranoid: a shard that never reported must FAIL rather than
 * vanish, two receipts for one route must FAIL rather than merge, and a
 * shard's own failures must survive the merge. All three are asserted here in
 * both directions, plus the wall-clock arithmetic that makes the fan-out worth
 * doing (slowest shard, not the sum).
 */
import { describe, expect, it } from "vitest";

import { formatShardSummary, summarizeShards } from "../src/corpus-gate/shards.js";
import { sampleManifestHash } from "../src/corpus-gate/sample.js";
import type {
  CorpusGateReceipt,
  CorpusGateRoute,
  CorpusGateThresholds,
  CorpusKindSummary,
  CorpusRouteReport,
} from "../src/corpus-gate/types.js";

const ALL_ROUTES: readonly CorpusGateRoute[] = ["tsserver", "tsgo", "shared-tsgo"];
const BUDGET_MS = 20 * 60_000;

const THRESHOLDS: CorpusGateThresholds = {
  hoverP95Ms: 300,
  definitionP95Ms: 500,
  completionP95Ms: 500,
  referencesP95Ms: 800,
  rssMaxBytes: 4 * 1024 * 1024 * 1024,
  allowedEmptyCategories: ["classToken"],
};

function kindSummary(): CorpusKindSummary {
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
  };
}

function routeReport(
  route: CorpusGateRoute,
  elapsedMs: number,
  latencyGating = true,
): CorpusRouteReport {
  const sample = ["src/components/BaseButton.vue"];
  return {
    route,
    completed: true,
    wedged: false,
    wedgeDetail: null,
    fatalError: null,
    startup: {
      initializeMs: 10,
      readyObserved: true,
      syncObserved: true,
      quiesced: true,
      settleMs: 10,
    },
    accounting: {
      requestsSent: 16,
      requestsAnswered: 16,
      requestsEmpty: 0,
      requestsTimedOut: 0,
      requestsErrored: 0,
      requestsAbandoned: 0,
      filesOpened: 1,
      filesSkipped: 0,
      probesMined: 10,
    },
    kinds: {
      hover: kindSummary(),
      definition: kindSummary(),
      completion: kindSummary(),
      references: kindSummary(),
    },
    memory: [],
    providerAttribution: {
      status: "verified",
      providerPid: 2,
      detail: "verified",
      unattributedPids: [],
      sampledProcessCount: 2,
    },
    earlyStop: { enabled: false, stopped: false, reason: null },
    isolation: {
      topology: "serial",
      executor: "dedicated",
      mode: latencyGating ? "isolated" : "contended",
      observedConcurrentRoutes: 1,
      latencyGating,
      attestationContradicted: false,
      evidence: latencyGating ? "sole route session" : "executor declared shared",
    },
    liveness: { checks: 1, failures: 0 },
    wallClock: { budgetMs: 1_200_000, elapsedMs, budgetExceeded: false },
    sampleManifestHash: sampleManifestHash(sample),
  };
}

function shard(
  route: CorpusGateRoute,
  options: {
    elapsedMs?: number;
    failures?: readonly string[];
    advisories?: readonly string[];
    latencyGating?: boolean;
  } = {},
): CorpusGateReceipt {
  const elapsedMs = options.elapsedMs ?? 60_000;
  const failures = options.failures ?? [];
  return {
    schemaVersion: 1,
    harness: "corpus-gate",
    createdAt: new Date().toISOString(),
    corpusLabel: "Corpus A",
    corpus: { vueFileCount: 731, sampledCount: 40 },
    config: {
      routes: [route],
      sampleSize: 40,
      maxProbesPerFile: 24,
      requestTimeoutMs: 15_000,
      wedgeLivenessTimeoutMs: 10_000,
      routeBudgetMs: 1_200_000,
      thresholds: THRESHOLDS,
    },
    routes: { [route]: routeReport(route, elapsedMs, options.latencyGating ?? true) },
    assertionFailures: failures,
    pass: failures.length === 0,
    budget: {
      targetMs: BUDGET_MS,
      actualMs: elapsedMs,
      exceeded: elapsedMs > BUDGET_MS,
      fatal: false,
    },
    advisories: options.advisories ?? [],
  };
}

describe("shard aggregation", () => {
  it("passes when every expected route reported exactly once", () => {
    const summary = summarizeShards(
      ALL_ROUTES.map((route) => shard(route)),
      ALL_ROUTES,
      BUDGET_MS,
    );
    expect(summary.pass).toBe(true);
    expect(summary.missingRoutes).toEqual([]);
    expect([...summary.coveredRoutes].sort()).toEqual([...ALL_ROUTES].sort());
    expect([...summary.latencyGatingRoutes].sort()).toEqual([...ALL_ROUTES].sort());
  });

  it("FAILS when a shard never reported — a missing shard is never a skip", () => {
    const summary = summarizeShards([shard("tsgo"), shard("tsserver")], ALL_ROUTES, BUDGET_MS);
    expect(summary.pass).toBe(false);
    expect(summary.missingRoutes).toEqual(["shared-tsgo"]);
    expect(summary.failures).toContainEqual(
      expect.stringContaining("[shared-tsgo] shard produced no receipt"),
    );
    expect(formatShardSummary(summary)).toContainEqual("corpus gate: FAIL");
  });

  it("FAILS on duplicate receipts for one route (ambiguous evidence)", () => {
    const summary = summarizeShards(
      [shard("tsgo"), shard("tsgo"), shard("tsserver"), shard("shared-tsgo")],
      ALL_ROUTES,
      BUDGET_MS,
    );
    expect(summary.pass).toBe(false);
    expect(summary.failures).toContainEqual(expect.stringContaining("ambiguous evidence"));
  });

  it("keeps a shard's own failures through the merge", () => {
    const summary = summarizeShards(
      [
        shard("tsgo", { failures: ["[tsgo] WEDGED: request never settled"] }),
        shard("tsserver"),
        shard("shared-tsgo"),
      ],
      ALL_ROUTES,
      BUDGET_MS,
    );
    expect(summary.pass).toBe(false);
    expect(summary.failures).toContainEqual("[tsgo] WEDGED: request never settled");
  });

  it("reports the fan-out wall clock as the SLOWEST shard, not the sum", () => {
    const summary = summarizeShards(
      [
        shard("tsserver", { elapsedMs: 900_000 }),
        shard("tsgo", { elapsedMs: 400_000 }),
        shard("shared-tsgo", { elapsedMs: 300_000 }),
      ],
      ALL_ROUTES,
      BUDGET_MS,
    );
    expect(summary.wallClock.fanOutMs).toBe(900_000);
    expect(summary.wallClock.totalMachineMs).toBe(1_600_000);
    expect(summary.budget.exceeded).toBe(false);
    expect(summary.pass).toBe(true);
  });

  it("reports a budget breach as an advisory without failing the merge", () => {
    const summary = summarizeShards(
      ALL_ROUTES.map((route) => shard(route, { elapsedMs: BUDGET_MS + 1_000 })),
      ALL_ROUTES,
      BUDGET_MS,
    );
    expect(summary.budget.exceeded).toBe(true);
    expect(summary.advisories).toContainEqual(expect.stringContaining("exceeds the"));
    expect(summary.failures).toEqual([]);
    expect(summary.pass).toBe(true);
  });

  it("separates gating from advisory latency across shards", () => {
    const summary = summarizeShards(
      [shard("tsserver"), shard("tsgo", { latencyGating: false }), shard("shared-tsgo")],
      ALL_ROUTES,
      BUDGET_MS,
    );
    expect(summary.latencyGatingRoutes).toEqual(["tsserver", "shared-tsgo"]);
    expect(summary.latencyAdvisoryRoutes).toEqual(["tsgo"]);
    const rendered = formatShardSummary(summary).join("\n");
    expect(rendered).toContain("latency GATING: tsserver, shared-tsgo");
    expect(rendered).toContain("ADVISORY: tsgo");
  });

  it("notes an unexpected extra route without failing", () => {
    const summary = summarizeShards(
      [shard("tsgo"), shard("tsserver")],
      ["tsgo", "tsserver"],
      BUDGET_MS,
    );
    expect(summary.pass).toBe(true);
    const withExtra = summarizeShards(
      [shard("tsgo"), shard("tsserver"), shard("shared-tsgo")],
      ["tsgo", "tsserver"],
      BUDGET_MS,
    );
    expect(withExtra.pass).toBe(true);
    expect(withExtra.advisories).toContainEqual(
      expect.stringContaining("covers a route that was not expected"),
    );
  });

  it("fails an empty fan-out (no receipts at all)", () => {
    const summary = summarizeShards([], ALL_ROUTES, BUDGET_MS);
    expect(summary.pass).toBe(false);
    expect(summary.failures).toHaveLength(3);
    expect(summary.wallClock.fanOutMs).toBe(0);
  });
});
