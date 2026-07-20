/**
 * Hermetic unit suite for the corpus benchmark gate.
 *
 * Runs entirely against the committed synthetic corpus in
 * `test/fixtures/corpus-gate-synthetic/` plus injected fake route runners —
 * no external corpus, no server spawn, no network. Proves the gate: samples
 * deterministically, mines authored probes exactly, measures and summarises,
 * enforces the acceptance bar (including non-vacuity), emits a comparable
 * receipt with zero corpus paths in it, diffs receipts, and honest-skips when
 * the corpus env is unset.
 */
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, describe, expect, it } from "vitest";

import { evaluateCorpusGate, evaluateRoute } from "../src/corpus-gate/assertions.js";
import { CORPUS_GATE_DIR_ENV, resolveCorpusGateEnv } from "../src/corpus-gate/config.js";
import {
  downsampleSeries,
  percentile,
  summarizeKind,
  summarizeKinds,
} from "../src/corpus-gate/metrics.js";
import { mineCorpusProbes } from "../src/corpus-gate/probes.js";
import {
  compareCorpusReceipts,
  corpusReceiptDestination,
  formatCompare,
  loadCorpusReceipt,
} from "../src/corpus-gate/receipt.js";
import {
  enumerateCorpusVueFiles,
  profileCorpus,
  profileCorpusFile,
  sampleManifestHash,
  selectRepresentativeSample,
} from "../src/corpus-gate/sample.js";
import { runCorpusGate } from "../src/corpus-gate/index.js";
import {
  completionIsEmpty,
  definitionIsEmpty,
  hoverIsEmpty,
  referencesIsEmpty,
} from "../src/corpus-gate/verdicts.js";
import type {
  CorpusGateConfig,
  CorpusGateRoute,
  CorpusKindSummary,
  CorpusRequestObservation,
  CorpusRouteReport,
} from "../src/corpus-gate/types.js";

const PACKAGE_ROOT = fileURLToPath(new URL("..", import.meta.url));
const SYNTHETIC_CORPUS = fileURLToPath(
  new URL("./fixtures/corpus-gate-synthetic", import.meta.url),
);

const tempDirs: string[] = [];
function tempDir(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "corpus-gate-test-"));
  tempDirs.push(dir);
  return dir;
}
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
});

const ALL_ROUTES: readonly CorpusGateRoute[] = ["tsserver", "tsgo", "shared-tsgo"];

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

function healthyReport(
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
        samples: [
          { atMs: 0, rssBytes: 100_000_000 },
          { atMs: 1000, rssBytes: 120_000_000 },
        ],
      },
    ],
    providerAttribution: {
      status: "verified",
      providerPid: 4243,
      detail: "provider pid 4243 (node) is a descendant of the spawned tree",
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

describe("corpus sampling", () => {
  it("enumerates exactly the committed synthetic .vue files, sorted", () => {
    const files = enumerateCorpusVueFiles(SYNTHETIC_CORPUS);
    expect(files).toEqual([
      "src/components/BaseButton.vue",
      "src/components/BaseCard.vue",
      "src/components/GenericFrame.vue",
      "src/components/widgets/WidgetList.vue",
      "src/legacy/OptionsPanel.vue",
      "src/pages/HomePage.vue",
    ]);
  });

  it("skips node_modules and build-output directories", () => {
    const root = tempDir();
    mkdirSync(path.join(root, "src"), { recursive: true });
    mkdirSync(path.join(root, "node_modules", "decoy"), { recursive: true });
    mkdirSync(path.join(root, "dist"), { recursive: true });
    mkdirSync(path.join(root, ".hidden"), { recursive: true });
    writeFileSync(path.join(root, "src", "Real.vue"), "<template><div /></template>\n");
    writeFileSync(path.join(root, "node_modules", "decoy", "Decoy.vue"), "<template />\n");
    writeFileSync(path.join(root, "dist", "Built.vue"), "<template />\n");
    writeFileSync(path.join(root, ".hidden", "Hidden.vue"), "<template />\n");
    expect(enumerateCorpusVueFiles(root)).toEqual(["src/Real.vue"]);
  });

  it("profiles structural features exactly", () => {
    const text = readFileSync(
      path.join(SYNTHETIC_CORPUS, "src", "components", "widgets", "WidgetList.vue"),
      "utf8",
    );
    const profile = profileCorpusFile("src/components/widgets/WidgetList.vue", text);
    expect(profile.features).toEqual({
      script: true,
      template: true,
      slots: false,
      events: true,
      directives: true,
      styles: false,
      props: true,
      emits: true,
      deepImport: true,
      barrelImport: false,
      generic: false,
      large: false,
    });
    expect(profile.score).toBe(7);
  });

  it("detects generic + barrel + slot features on the other members", () => {
    const profiles = profileCorpus(SYNTHETIC_CORPUS);
    const byPath = new Map(profiles.map((profile) => [profile.relativePath, profile]));
    expect(byPath.get("src/components/GenericFrame.vue")?.features.generic).toBe(true);
    expect(byPath.get("src/components/BaseButton.vue")?.features.barrelImport).toBe(true);
    expect(byPath.get("src/components/BaseButton.vue")?.features.styles).toBe(true);
    expect(byPath.get("src/pages/HomePage.vue")?.features.slots).toBe(true);
    expect(byPath.get("src/legacy/OptionsPanel.vue")?.features.props).toBe(false);
  });

  it("selects a deterministic representative sample (bucket round-robin)", () => {
    const profiles = profileCorpus(SYNTHETIC_CORPUS);
    const first = selectRepresentativeSample(profiles, 3).map((profile) => profile.relativePath);
    const second = selectRepresentativeSample(profiles, 3).map((profile) => profile.relativePath);
    // generic bucket -> GenericFrame; slots bucket -> BaseButton (highest score);
    // events bucket -> WidgetList; output re-sorted by path.
    expect(first).toEqual([
      "src/components/BaseButton.vue",
      "src/components/GenericFrame.vue",
      "src/components/widgets/WidgetList.vue",
    ]);
    expect(second).toEqual(first);
  });

  it("selects everything when n exceeds the corpus", () => {
    const profiles = profileCorpus(SYNTHETIC_CORPUS);
    expect(selectRepresentativeSample(profiles, 40)).toHaveLength(6);
  });

  it("hashes sample manifests stably and order-sensitively", () => {
    const hashA = sampleManifestHash(["a.vue", "b.vue"]);
    expect(sampleManifestHash(["a.vue", "b.vue"])).toBe(hashA);
    expect(sampleManifestHash(["b.vue", "a.vue"])).not.toBe(hashA);
    expect(hashA).toMatch(/^[0-9a-f]{16}$/);
  });
});

describe("probe mining", () => {
  it("mines the exact authored probes from BaseButton.vue", () => {
    const text = readFileSync(
      path.join(SYNTHETIC_CORPUS, "src", "components", "BaseButton.vue"),
      "utf8",
    );
    const probes = mineCorpusProbes(text, 24);
    expect(probes.map((probe) => [probe.category, probe.token, [...probe.kinds]] as const)).toEqual(
      [
        ["propBind", "variant", ["hover", "definition"]],
        ["eventBind", "click", ["hover"]],
        ["classToken", "base-button", ["hover", "definition"]],
        ["slotDef", "icon", ["hover"]],
        ["interp", "label", ["hover", "definition", "references"]],
        ["classToken", "base-label", ["hover", "definition"]],
        ["importName", "computed", ["definition", "hover"]],
        ["definePropsVar", "props", ["hover", "references"]],
        ["scriptMemberCompl", "props.label", ["completion", "hover"]],
        ["scriptMemberCompl", "props.label", ["completion", "hover"]],
      ],
    );
    // Fired-request budget for this file: sum of kinds.
    expect(probes.reduce((total, probe) => total + probe.kinds.length, 0)).toBe(19);
  });

  it("mines v-for aliases, template member completions and imports from WidgetList.vue", () => {
    const text = readFileSync(
      path.join(SYNTHETIC_CORPUS, "src", "components", "widgets", "WidgetList.vue"),
      "utf8",
    );
    const categories = mineCorpusProbes(text, 24).map((probe) => probe.category);
    expect(categories).toEqual([
      "eventBind",
      "vfor",
      "interp",
      "templMemberCompl",
      "importName",
      "definePropsVar",
    ]);
  });

  it("caps the probe count deterministically", () => {
    const text = readFileSync(
      path.join(SYNTHETIC_CORPUS, "src", "components", "BaseButton.vue"),
      "utf8",
    );
    const probes = mineCorpusProbes(text, 4);
    expect(probes.map((probe) => probe.category)).toEqual([
      "propBind",
      "eventBind",
      "classToken",
      "slotDef",
    ]);
  });

  it("positions point at the mined token", () => {
    const text = readFileSync(
      path.join(SYNTHETIC_CORPUS, "src", "components", "BaseButton.vue"),
      "utf8",
    );
    const lines = text.split(/\r\n|\n|\r/);
    for (const probe of mineCorpusProbes(text, 24)) {
      const slice = lines[probe.line].slice(probe.character);
      const expected =
        probe.category === "scriptMemberCompl" || probe.category === "templMemberCompl"
          ? probe.token.split(".")[1] // position sits AFTER the dot
          : probe.token;
      expect(
        slice.startsWith(expected),
        `${probe.category} @ ${probe.line}:${probe.character}`,
      ).toBe(true);
    }
  });
});

describe("metrics", () => {
  it("computes nearest-rank percentiles exactly", () => {
    const samples = Array.from({ length: 100 }, (_, index) => index + 1);
    expect(percentile(samples, 50)).toBe(50);
    expect(percentile(samples, 90)).toBe(90);
    expect(percentile(samples, 95)).toBe(95);
    expect(percentile(samples, 100)).toBe(100);
    expect(percentile([250], 95)).toBe(250);
    expect(percentile([], 95)).toBe(0);
    expect(percentile([30, 10, 20], 50)).toBe(20);
  });

  it("summarises one kind with exact counts", () => {
    const observations: CorpusRequestObservation[] = [
      { kind: "hover", category: "interp", ms: 10, verdict: "ok", unexpectedEmpty: false },
      { kind: "hover", category: "interp", ms: 3000, verdict: "ok", unexpectedEmpty: false },
      { kind: "hover", category: "vfor", ms: 20, verdict: "empty", unexpectedEmpty: true },
      { kind: "hover", category: "classToken", ms: 30, verdict: "empty", unexpectedEmpty: false },
      { kind: "hover", category: "interp", ms: 15_000, verdict: "timeout", unexpectedEmpty: false },
      { kind: "definition", category: "interp", ms: 999, verdict: "ok", unexpectedEmpty: false },
    ];
    const summary = summarizeKind(observations, "hover");
    expect(summary).toEqual({
      count: 5,
      p50Ms: 30,
      p90Ms: 15_000,
      p95Ms: 15_000,
      maxMs: 15_000,
      over2500Count: 2,
      timeoutCount: 1,
      emptyCount: 2,
      unexpectedEmptyCount: 1,
      errorCount: 0,
    });
    expect(summarizeKinds(observations).definition.count).toBe(1);
    expect(summarizeKinds(observations).completion.count).toBe(0);
  });

  it("downsamples keeping endpoints", () => {
    const series = Array.from({ length: 500 }, (_, index) => index);
    const sampled = downsampleSeries(series, 60);
    expect(sampled).toHaveLength(60);
    expect(sampled[0]).toBe(0);
    expect(sampled[59]).toBe(499);
    expect(downsampleSeries([1, 2], 60)).toEqual([1, 2]);
  });
});

describe("verdicts", () => {
  it("classifies hover emptiness", () => {
    expect(hoverIsEmpty(null)).toBe(true);
    expect(hoverIsEmpty({ contents: "" })).toBe(true);
    expect(hoverIsEmpty({ contents: { value: "  " } })).toBe(true);
    expect(hoverIsEmpty({ contents: "const x: string" })).toBe(false);
    expect(hoverIsEmpty({ contents: [{ value: "a" }, "b"] })).toBe(false);
  });
  it("classifies definition/completion/references emptiness", () => {
    expect(definitionIsEmpty(null)).toBe(true);
    expect(definitionIsEmpty([])).toBe(true);
    expect(definitionIsEmpty([{ uri: "file:///a.ts" }])).toBe(false);
    expect(definitionIsEmpty({ uri: "file:///a.ts" })).toBe(false);
    expect(completionIsEmpty(null)).toBe(true);
    expect(completionIsEmpty({ items: [] })).toBe(true);
    expect(completionIsEmpty({ items: [{ label: "x" }] })).toBe(false);
    expect(completionIsEmpty([{ label: "x" }])).toBe(false);
    expect(referencesIsEmpty([])).toBe(true);
    expect(referencesIsEmpty([{ uri: "u" }])).toBe(false);
  });
});

describe("acceptance bar", () => {
  const sample = ["src/components/BaseButton.vue"];

  it("passes a healthy report", () => {
    const report = healthyReport("tsgo", sample);
    expect(evaluateRoute(report, testConfig().thresholds)).toEqual([]);
  });

  it("fails a wedged route and does not double-report vacuous kinds", () => {
    const report = healthyReport("tsgo", sample, {
      wedged: true,
      wedgeDetail: "request never settled: definition",
      completed: false,
      kinds: {
        hover: kindSummary({ count: 0, p95Ms: 0 }),
        definition: kindSummary({ count: 0, p95Ms: 0 }),
        completion: kindSummary({ count: 0, p95Ms: 0 }),
        references: kindSummary({ count: 0, p95Ms: 0 }),
      },
    });
    const failures = evaluateRoute(report, testConfig().thresholds);
    expect(failures.some((failure) => failure.includes("WEDGED"))).toBe(true);
    expect(failures.some((failure) => failure.includes("vacuous kind"))).toBe(false);
  });

  it("fails a vacuous run (zero requests) — non-vacuity is part of the bar", () => {
    const report = healthyReport("tsgo", sample, {
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
        hover: kindSummary({ count: 0 }),
        definition: kindSummary({ count: 0 }),
        completion: kindSummary({ count: 0 }),
        references: kindSummary({ count: 0 }),
      },
    });
    const failures = evaluateRoute(report, testConfig().thresholds);
    expect(failures.some((failure) => failure.includes("zero requests were fired"))).toBe(true);
    expect(failures.some((failure) => failure.includes("zero sampled files were opened"))).toBe(
      true,
    );
    // A completed-but-idle route also fails per-kind vacuity.
    expect(failures.some((failure) => failure.includes("vacuous kind"))).toBe(true);
  });

  it("fails accounting identity violations", () => {
    const report = healthyReport("tsgo", sample, {
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
    });
    expect(
      evaluateRoute(report, testConfig().thresholds).some((failure) =>
        failure.includes("accounting identity violated"),
      ),
    ).toBe(true);
  });

  it("fails p95 breaches, unexpected empties, memory breaches and budget blow-through", () => {
    const thresholds = testConfig().thresholds;
    const slow = healthyReport("tsgo", sample, {
      kinds: {
        hover: kindSummary({ p95Ms: 301 }),
        definition: kindSummary({ p95Ms: 7_800 }),
        completion: kindSummary(),
        references: kindSummary(),
      },
    });
    const slowFailures = evaluateRoute(slow, thresholds);
    expect(slowFailures.some((failure) => failure.includes("hover p95 301ms"))).toBe(true);
    expect(slowFailures.some((failure) => failure.includes("definition p95 7800ms"))).toBe(true);

    const empties = healthyReport("tsgo", sample, {
      kinds: {
        hover: kindSummary({ emptyCount: 2, unexpectedEmptyCount: 2 }),
        definition: kindSummary(),
        completion: kindSummary(),
        references: kindSummary(),
      },
    });
    expect(
      evaluateRoute(empties, thresholds).some((failure) =>
        failure.includes("2 unexpected empty result(s)"),
      ),
    ).toBe(true);

    const fat = healthyReport("tsgo", sample, {
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
        },
      ],
    });
    expect(evaluateRoute(fat, thresholds).some((failure) => failure.includes("exceeds the"))).toBe(
      true,
    );

    const late = healthyReport("tsgo", sample, {
      wallClock: { budgetMs: 60_000, elapsedMs: 61_000, budgetExceeded: true },
    });
    expect(
      evaluateRoute(late, thresholds).some((failure) => failure.includes("wall-clock budget")),
    ).toBe(true);

    // Unsupported memory sampling is an explicit skip, never a failure.
    const unsupported = healthyReport("tsgo", sample, {
      memory: [
        {
          label: "provider",
          pid: null,
          supported: false,
          sampleCount: 0,
          firstRssBytes: null,
          lastRssBytes: null,
          maxRssBytes: null,
          samples: [],
        },
      ],
    });
    expect(evaluateRoute(unsupported, thresholds)).toEqual([]);
  });

  it("fails a requested route that produced no report", () => {
    const receipt = {
      schemaVersion: 1 as const,
      harness: "corpus-gate" as const,
      createdAt: new Date().toISOString(),
      corpusLabel: "Corpus A",
      corpus: { vueFileCount: 6, sampledCount: 1 },
      config: testConfigEcho(),
      routes: { tsgo: healthyReport("tsgo", sample) },
      assertionFailures: [],
      pass: false,
    };
    const failures = evaluateCorpusGate(receipt, ALL_ROUTES, testConfig().thresholds);
    expect(failures.some((failure) => failure.includes("[tsserver] requested route"))).toBe(true);
    expect(failures.some((failure) => failure.includes("[shared-tsgo] requested route"))).toBe(
      true,
    );
  });
});

function testConfigEcho() {
  const config = testConfig();
  return {
    routes: config.routes,
    sampleSize: config.sampleSize,
    maxProbesPerFile: config.maxProbesPerFile,
    requestTimeoutMs: config.requestTimeoutMs,
    wedgeLivenessTimeoutMs: config.wedgeLivenessTimeoutMs,
    routeBudgetMs: config.routeBudgetMs,
    thresholds: config.thresholds,
  };
}

describe("config resolution", () => {
  it("honest-skips when the corpus env is unset, naming the variable", () => {
    const resolution = resolveCorpusGateEnv({});
    expect(resolution.kind).toBe("skip");
    if (resolution.kind === "skip") {
      expect(resolution.reason).toContain(CORPUS_GATE_DIR_ENV);
      expect(resolution.reason).toContain("explicit");
    }
  });

  it("throws loudly when the env points at a non-directory (no silent skip)", () => {
    expect(() =>
      resolveCorpusGateEnv({
        [CORPUS_GATE_DIR_ENV]: path.join(SYNTHETIC_CORPUS, "does-not-exist"),
      }),
    ).toThrow(/not a directory/);
  });

  it("resolves defaults and env overrides", () => {
    const resolution = resolveCorpusGateEnv({ [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS });
    expect(resolution.kind).toBe("run");
    if (resolution.kind === "run") {
      expect(resolution.config.corpusLabel).toBe("Corpus A");
      expect(resolution.config.routes).toEqual(ALL_ROUTES);
      expect(resolution.config.sampleSize).toBe(40);
      expect(resolution.config.thresholds.hoverP95Ms).toBe(300);
      expect(resolution.config.thresholds.allowedEmptyCategories).toEqual(["classToken"]);
      expect(resolution.config.includeFileDetail).toBe(false);
    }
    const tuned = resolveCorpusGateEnv({
      [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS,
      VERTER_CORPUS_GATE_ROUTES: "tsgo, tsserver",
      VERTER_CORPUS_GATE_SAMPLE: "12",
      VERTER_CORPUS_GATE_HOVER_P95_MS: "150",
      VERTER_CORPUS_GATE_ALLOWED_EMPTY: "classToken,eventBind",
    });
    if (tuned.kind === "run") {
      expect(tuned.config.routes).toEqual(["tsgo", "tsserver"]);
      expect(tuned.config.sampleSize).toBe(12);
      expect(tuned.config.thresholds.hoverP95Ms).toBe(150);
      expect(tuned.config.thresholds.allowedEmptyCategories).toEqual(["classToken", "eventBind"]);
    }
    expect(() =>
      resolveCorpusGateEnv({
        [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS,
        VERTER_CORPUS_GATE_ROUTES: "tsgo,nonsense",
      }),
    ).toThrow(/unknown route/);
    expect(() =>
      resolveCorpusGateEnv({
        [CORPUS_GATE_DIR_ENV]: SYNTHETIC_CORPUS,
        VERTER_CORPUS_GATE_SAMPLE: "-3",
      }),
    ).toThrow(/positive integer/);
  });
});

describe("receipt emission and compare", () => {
  it("resolves destinations for file, directory and default cases", () => {
    const receipt = {
      schemaVersion: 1 as const,
      harness: "corpus-gate" as const,
      createdAt: new Date().toISOString(),
      corpusLabel: "Corpus A",
      corpus: { vueFileCount: 0, sampledCount: 0 },
      config: testConfigEcho(),
      routes: {},
      assertionFailures: [],
      pass: false,
    };
    const dir = tempDir();
    const explicit = path.join(dir, "out", "receipt.json");
    expect(corpusReceiptDestination(receipt, explicit)).toBe(path.resolve(explicit));
    const inDir = corpusReceiptDestination(receipt, dir);
    expect(path.dirname(inDir)).toBe(path.resolve(dir));
    expect(path.basename(inDir)).toMatch(/^corpus-gate-\d+\.json$/);
    expect(path.dirname(corpusReceiptDestination(receipt, null))).toBe(path.resolve(tmpdir()));
  });

  it("rejects a non-receipt on load", () => {
    const dir = tempDir();
    const bogus = path.join(dir, "bogus.json");
    writeFileSync(bogus, JSON.stringify({ hello: "world" }));
    expect(() => loadCorpusReceipt(bogus)).toThrow(/not a corpus-gate/);
  });

  it("runs the full gate with an injected runner: receipt, bar, no corpus paths", async () => {
    const dir = tempDir();
    const receiptPath = path.join(dir, "receipt.json");
    const config = testConfig({ receiptPath });
    const logs: string[] = [];
    const outcome = await runCorpusGate(config, {
      log: (message) => logs.push(message),
      runRoute: async (route, _config, sample) => healthyReport(route, sample),
    });
    expect(outcome.failures).toEqual([]);
    expect(outcome.receipt.pass).toBe(true);
    expect(outcome.receipt.corpus).toEqual({ vueFileCount: 6, sampledCount: 6 });
    expect(Object.keys(outcome.receipt.routes).sort()).toEqual(
      ["shared-tsgo", "tsgo", "tsserver"].sort(),
    );
    expect(outcome.receiptPath).toBe(path.resolve(receiptPath));
    expect(existsSync(outcome.receiptPath)).toBe(true);

    // Redaction: the receipt must never leak the corpus location or file names.
    const raw = readFileSync(outcome.receiptPath, "utf8");
    expect(raw).toContain("Corpus A");
    expect(raw).not.toContain("corpus-gate-synthetic");
    expect(raw).not.toContain("BaseButton");
    expect(raw).not.toContain(SYNTHETIC_CORPUS.replaceAll("\\", "\\\\"));
    // All three routes ran through the runner (attested in the log stream).
    expect(logs.some((line) => line.includes("route tsserver: done"))).toBe(true);
    expect(logs.some((line) => line.includes("route shared-tsgo: done"))).toBe(true);
  });

  it("fails the bar when the injected runner reports a vacuous or wedged route", async () => {
    const config = testConfig({ receiptPath: path.join(tempDir(), "r.json") });
    const vacuous = await runCorpusGate(config, {
      log: () => {},
      runRoute: async (route, _config, sample) =>
        healthyReport(route, sample, {
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
    });
    expect(vacuous.receipt.pass).toBe(false);
    expect(vacuous.failures.some((failure) => failure.includes("zero requests"))).toBe(true);

    const wedgy = await runCorpusGate(config, {
      log: () => {},
      runRoute: async (route, _config, sample) =>
        route === "tsgo"
          ? healthyReport(route, sample, { wedged: true, wedgeDetail: "stuck", completed: false })
          : healthyReport(route, sample),
    });
    expect(wedgy.receipt.pass).toBe(false);
    expect(wedgy.failures.some((failure) => failure.includes("[tsgo] WEDGED: stuck"))).toBe(true);
  });

  it("compares two receipts with per-kind deltas and manifest caveats", async () => {
    const dir = tempDir();
    const baselinePath = path.join(dir, "baseline.json");
    const config = testConfig({ receiptPath: baselinePath, routes: ["tsgo"] });
    await runCorpusGate(config, {
      log: () => {},
      runRoute: async (route, _config, sample) => healthyReport(route, sample),
    });

    const secondPath = path.join(dir, "second.json");
    const second = await runCorpusGate(
      testConfig({ receiptPath: secondPath, baselinePath, routes: ["tsgo"] }),
      {
        log: () => {},
        runRoute: async (route, _config, sample) =>
          healthyReport(route, sample, {
            kinds: {
              hover: kindSummary(),
              definition: kindSummary({ p95Ms: 250 }),
              completion: kindSummary(),
              references: kindSummary(),
            },
          }),
      },
    );
    expect(second.compare).not.toBeNull();
    const definitionDelta = second.compare?.lines.find(
      (line) => line.route === "tsgo" && line.metric === "definition.p95Ms",
    );
    expect(definitionDelta).toEqual({
      route: "tsgo",
      metric: "definition.p95Ms",
      baseline: 50,
      current: 250,
      delta: 200,
    });
    // Same corpus + same sample ⇒ fully comparable.
    expect(second.compare?.comparable).toBe(true);
    expect(second.compareText.some((line) => line.includes("definition.p95Ms: 50 -> 250"))).toBe(
      true,
    );

    // A differing sample manifest is a caveat, not a silent mislead.
    const loaded = loadCorpusReceipt(baselinePath);
    const mutated = {
      ...loaded,
      routes: {
        tsgo: { ...loaded.routes.tsgo!, sampleManifestHash: "0000000000000000" },
      },
    };
    const caveated = compareCorpusReceipts(mutated, second.receipt);
    expect(caveated.comparable).toBe(false);
    expect(caveated.caveats.some((caveat) => caveat.includes("sample manifest differs"))).toBe(
      true,
    );
    expect(formatCompare(caveated).some((line) => line.startsWith("CAVEAT:"))).toBe(true);
  });
});

describe("lane wiring (the gate can actually be invoked)", () => {
  it("package.json exposes test:corpus-gate and the lane config includes the lane file", () => {
    const packageJson = JSON.parse(readFileSync(path.join(PACKAGE_ROOT, "package.json"), "utf8"));
    expect(packageJson.scripts["test:corpus-gate"]).toContain("vitest.corpus-gate.config.ts");
    const laneConfig = readFileSync(
      path.join(PACKAGE_ROOT, "vitest.corpus-gate.config.ts"),
      "utf8",
    );
    expect(laneConfig).toContain("test/corpusGate.lane.ts");
    expect(existsSync(path.join(PACKAGE_ROOT, "test", "corpusGate.lane.ts"))).toBe(true);
  });
});
