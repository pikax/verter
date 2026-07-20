/**
 * Coverage equality: nothing the gate verified before is verified less now.
 *
 * Making the gate fit a wall-clock budget must not buy speed with coverage. So
 * the acceptance bar's PRE-CHANGE failure classes are enumerated here as data
 * and each is driven with a fixture that must still make it fire on the
 * default (serial, isolated) path — the path every existing consumer runs.
 * The classes added since are enumerated separately, so the two lists together
 * are the before/after proof: same classes, plus more.
 *
 * The sampling knobs are pinned in the same spirit: sampled-file count, probes
 * per file, and the exact fired-request budget of a corpus file are asserted
 * unchanged, because "fewer probes" is the other way a gate quietly shrinks.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { evaluateCorpusGate, evaluateRoute } from "../src/corpus-gate/assertions.js";
import { CORPUS_GATE_DIR_ENV, resolveCorpusGateEnv } from "../src/corpus-gate/config.js";
import { mineCorpusProbes } from "../src/corpus-gate/probes.js";
import { sampleManifestHash } from "../src/corpus-gate/sample.js";
import type {
  CorpusGateReceipt,
  CorpusGateRoute,
  CorpusGateThresholds,
  CorpusKindSummary,
  CorpusRouteReport,
} from "../src/corpus-gate/types.js";

const SYNTHETIC_CORPUS = fileURLToPath(
  new URL("./fixtures/corpus-gate-synthetic", import.meta.url),
);

const THRESHOLDS: CorpusGateThresholds = {
  hoverP95Ms: 300,
  definitionP95Ms: 500,
  completionP95Ms: 500,
  referencesP95Ms: 800,
  rssMaxBytes: 4 * 1024 * 1024 * 1024,
  allowedEmptyCategories: ["classToken"],
};

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

const SAMPLE = ["src/components/BaseButton.vue"];

function healthy(overrides: Partial<CorpusRouteReport> = {}): CorpusRouteReport {
  return {
    route: "tsgo",
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
    liveness: { checks: 1, failures: 0 },
    wallClock: { budgetMs: 60_000, elapsedMs: 2_000, budgetExceeded: false },
    sampleManifestHash: sampleManifestHash(SAMPLE),
    ...overrides,
  };
}

function receiptOf(routes: Partial<Record<CorpusGateRoute, CorpusRouteReport>>): CorpusGateReceipt {
  return {
    schemaVersion: 1,
    harness: "corpus-gate",
    createdAt: new Date().toISOString(),
    corpusLabel: "Corpus A",
    corpus: { vueFileCount: 6, sampledCount: 1 },
    config: {
      routes: ["tsgo"],
      sampleSize: 40,
      maxProbesPerFile: 24,
      requestTimeoutMs: 15_000,
      wedgeLivenessTimeoutMs: 10_000,
      routeBudgetMs: 1_200_000,
      thresholds: THRESHOLDS,
    },
    routes,
    assertionFailures: [],
    pass: false,
  };
}

interface BarCase {
  readonly id: string;
  readonly expected: string;
  readonly failures: () => readonly string[];
}

/**
 * The failure classes the bar enforced BEFORE execution topology, provider
 * attribution and the budget were added. Every one must still fire.
 */
const PRE_CHANGE_CLASSES: readonly BarCase[] = [
  {
    id: "fatal-route-error",
    expected: "fatal route error",
    failures: () => evaluateRoute(healthy({ fatalError: "spawn failed" }), THRESHOLDS),
  },
  {
    id: "wedge",
    expected: "WEDGED",
    failures: () =>
      evaluateRoute(healthy({ wedged: true, wedgeDetail: "never settled" }), THRESHOLDS),
  },
  {
    id: "route-budget",
    expected: "exceeded its wall-clock budget",
    failures: () =>
      evaluateRoute(
        healthy({ wallClock: { budgetMs: 60_000, elapsedMs: 61_000, budgetExceeded: true } }),
        THRESHOLDS,
      ),
  },
  {
    id: "vacuous-requests",
    expected: "zero requests were fired",
    failures: () =>
      evaluateRoute(
        healthy({
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
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "vacuous-files",
    expected: "zero sampled files were opened",
    failures: () =>
      evaluateRoute(
        healthy({
          accounting: {
            requestsSent: 4,
            requestsAnswered: 4,
            requestsEmpty: 0,
            requestsTimedOut: 0,
            requestsErrored: 0,
            requestsAbandoned: 0,
            filesOpened: 0,
            filesSkipped: 0,
            probesMined: 2,
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "vacuous-probes",
    expected: "zero authored probes were mined",
    failures: () =>
      evaluateRoute(
        healthy({
          accounting: {
            requestsSent: 4,
            requestsAnswered: 4,
            requestsEmpty: 0,
            requestsTimedOut: 0,
            requestsErrored: 0,
            requestsAbandoned: 0,
            filesOpened: 1,
            filesSkipped: 0,
            probesMined: 0,
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "accounting-identity-sent",
    expected: "accounting identity violated: sent=",
    failures: () =>
      evaluateRoute(
        healthy({
          accounting: {
            requestsSent: 10,
            requestsAnswered: 4,
            requestsEmpty: 0,
            requestsTimedOut: 1,
            requestsErrored: 0,
            requestsAbandoned: 0,
            filesOpened: 1,
            filesSkipped: 0,
            probesMined: 5,
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "accounting-identity-empty",
    expected: "accounting identity violated: empty=",
    failures: () =>
      evaluateRoute(
        healthy({
          accounting: {
            requestsSent: 5,
            requestsAnswered: 4,
            requestsEmpty: 9,
            requestsTimedOut: 1,
            requestsErrored: 0,
            requestsAbandoned: 0,
            filesOpened: 1,
            filesSkipped: 0,
            probesMined: 5,
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "vacuous-kind",
    expected: "vacuous kind: zero completion requests were measured",
    failures: () =>
      evaluateRoute(
        healthy({
          kinds: {
            hover: kindSummary(),
            definition: kindSummary(),
            completion: kindSummary({ count: 0 }),
            references: kindSummary(),
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "latency-p95",
    expected: "hover p95 9000ms breaches the < 300ms bar",
    failures: () =>
      evaluateRoute(
        healthy({
          kinds: {
            hover: kindSummary({ p95Ms: 9_000 }),
            definition: kindSummary(),
            completion: kindSummary(),
            references: kindSummary(),
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "unexpected-empty",
    expected: "unexpected empty result(s)",
    failures: () =>
      evaluateRoute(
        healthy({
          kinds: {
            hover: kindSummary({ emptyCount: 2, unexpectedEmptyCount: 2 }),
            definition: kindSummary(),
            completion: kindSummary(),
            references: kindSummary(),
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "rss-ceiling",
    expected: "exceeds the",
    failures: () =>
      evaluateRoute(
        healthy({
          memory: [
            {
              label: "provider",
              pid: 77,
              supported: true,
              sampleCount: 2,
              firstRssBytes: 1,
              lastRssBytes: 2,
              maxRssBytes: THRESHOLDS.rssMaxBytes + 1,
              samples: [],
              role: "provider",
              parentPid: 4242,
              image: "node",
            },
          ],
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "missing-route-report",
    expected: "requested route produced no report",
    failures: () =>
      evaluateCorpusGate(receiptOf({ tsgo: healthy() }), ["tsgo", "tsserver"], THRESHOLDS),
  },
  {
    id: "empty-receipt",
    expected: "receipt contains zero route reports",
    failures: () => evaluateCorpusGate(receiptOf({}), [], THRESHOLDS),
  },
];

/** Failure classes this change ADDS — the "or greater" half of the proof. */
const ADDED_CLASSES: readonly BarCase[] = [
  {
    id: "isolation-contradiction",
    expected: "isolation attestation CONTRADICTED",
    failures: () =>
      evaluateRoute(
        healthy({
          isolation: {
            topology: "parallel",
            executor: "dedicated",
            mode: "contended",
            observedConcurrentRoutes: 3,
            latencyGating: false,
            attestationContradicted: true,
            evidence: "3 concurrent sessions",
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "isolation-required",
    expected: "isolation was REQUIRED",
    failures: () =>
      evaluateRoute(
        healthy({
          isolation: {
            topology: "serial",
            executor: "shared",
            mode: "contended",
            observedConcurrentRoutes: 1,
            latencyGating: false,
            attestationContradicted: false,
            evidence: "executor declared shared",
          },
        }),
        THRESHOLDS,
        { requireIsolatedLatency: true },
      ),
  },
  {
    id: "attribution-unrecorded",
    expected: "provider attribution was never recorded",
    failures: () => {
      const { providerAttribution: _dropped, ...rest } = healthy();
      return evaluateRoute(rest as CorpusRouteReport, THRESHOLDS);
    },
  },
  {
    id: "attribution-missing",
    expected: "provider attribution MISSING",
    failures: () =>
      evaluateRoute(
        healthy({
          providerAttribution: {
            status: "missing",
            providerPid: null,
            detail: "no provider advertised",
            unattributedPids: [],
            sampledProcessCount: 1,
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "attribution-mismatched",
    expected: "provider attribution MISMATCHED",
    failures: () =>
      evaluateRoute(
        healthy({
          providerAttribution: {
            status: "mismatched",
            providerPid: 900,
            detail: "not a descendant",
            unattributedPids: [],
            sampledProcessCount: 2,
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "unsampled-tree-member",
    expected: "never sampled",
    failures: () =>
      evaluateRoute(
        healthy({
          providerAttribution: {
            status: "verified",
            providerPid: 4243,
            detail: "verified",
            unattributedPids: [4244],
            sampledProcessCount: 2,
          },
        }),
        THRESHOLDS,
      ),
  },
  {
    id: "gate-budget-fatal",
    expected: "exceeds the 10ms budget",
    failures: () =>
      evaluateCorpusGate(
        {
          ...receiptOf({ tsgo: healthy() }),
          budget: { targetMs: 10, actualMs: 99, exceeded: true, fatal: true },
        },
        ["tsgo"],
        THRESHOLDS,
      ),
  },
];

describe("acceptance-bar coverage equality (before/after)", () => {
  it.each(PRE_CHANGE_CLASSES.map((barCase) => [barCase.id, barCase] as const))(
    "still enforces the pre-change class %s",
    (_id, barCase) => {
      const failures = barCase.failures();
      expect(
        failures.some((failure) => failure.includes(barCase.expected)),
        `expected a failure containing ${JSON.stringify(barCase.expected)}; got ${JSON.stringify(failures)}`,
      ).toBe(true);
    },
  );

  it.each(ADDED_CLASSES.map((barCase) => [barCase.id, barCase] as const))(
    "adds the new class %s",
    (_id, barCase) => {
      const failures = barCase.failures();
      expect(
        failures.some((failure) => failure.includes(barCase.expected)),
        `expected a failure containing ${JSON.stringify(barCase.expected)}; got ${JSON.stringify(failures)}`,
      ).toBe(true);
    },
  );

  it("enforces strictly MORE classes than before (14 → 21) and none were dropped", () => {
    expect(PRE_CHANGE_CLASSES).toHaveLength(14);
    expect(ADDED_CLASSES.length).toBeGreaterThan(0);
    expect(PRE_CHANGE_CLASSES.length + ADDED_CLASSES.length).toBe(21);
    // Every enumerated class has a distinct id (no fixture counted twice).
    const ids = [...PRE_CHANGE_CLASSES, ...ADDED_CLASSES].map((barCase) => barCase.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("keeps a healthy report passing — the bar is not just always-red", () => {
    expect(evaluateRoute(healthy(), THRESHOLDS)).toEqual([]);
    expect(evaluateCorpusGate(receiptOf({ tsgo: healthy() }), ["tsgo"], THRESHOLDS)).toEqual([]);
  });
});

describe("sampling breadth is unchanged (no quiet shrink)", () => {
  it("keeps the sampled-file and per-file probe defaults", () => {
    const resolution = resolveCorpusGateEnv({ [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS });
    expect(resolution.kind).toBe("run");
    if (resolution.kind !== "run") return;
    expect(resolution.config.sampleSize).toBe(40);
    expect(resolution.config.maxProbesPerFile).toBe(24);
    expect(resolution.config.routes).toEqual(["tsserver", "tsgo", "shared-tsgo"]);
    expect(resolution.config.requestTimeoutMs).toBe(15_000);
  });

  it("keeps the exact fired-request budget of a corpus file", () => {
    const text = readFileSync(
      path.join(SYNTHETIC_CORPUS, "src", "components", "BaseButton.vue"),
      "utf8",
    );
    const probes = mineCorpusProbes(text, 24);
    expect(probes).toHaveLength(10);
    expect(probes.reduce((total, probe) => total + probe.kinds.length, 0)).toBe(19);
  });
});
