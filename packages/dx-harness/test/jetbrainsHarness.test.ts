/**
 * JBT1H comparison harness: real-IDE evidence (AC1), provider-required memory
 * (AC2), swapped-order paired runs (AC3), fixture workflows, labelled-unknown
 * metrics. Hermetic — no Gradle, no mock LSP counted as UI evidence.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  COMPARISON_WORKFLOWS,
  FIXTURE_CORPUS,
  REAL_IDE_HOST,
  SKELETON_UNSUPPORTED_REASON,
  classifyUiEvidence,
  compareSwappedPairs,
  defaultWorkflowRows,
  evaluateMemoryRow,
  ingestCapture,
  labelOutliers,
  pairOrderFromSeed,
  parseIdeCapture,
  providerRequiredForMemory,
  recordPair,
  runPairedCampaign,
  sameReceiptBasis,
  swappedOrder,
  unknownMetric,
  type IdeCapture,
  type ProcessTreeSnapshot,
  type ProductReceiptBasis,
} from "../jetbrains/index.js";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

const BASIS: ProductReceiptBasis = {
  sourceRevisions: "fixture:dx-harness-hermetic@pinned",
  projectConfiguration: "packages/dx-harness/fixtures/hermetic",
  engineIdentity: "unknown: skeleton carries no TypeScript engine (JBT2)",
  hostIdentity: "real-jetbrains-ide:platform-test-framework:WS-2026.2.3",
  completenessState: "partial",
};

function tree(overrides: Partial<ProcessTreeSnapshot> = {}): ProcessTreeSnapshot {
  return {
    rootPid: 10,
    members: [
      {
        pid: 10,
        parentPid: null,
        image: "idea",
        role: "ide",
        rssBytes: unknownMetric("rss not sampled in hermetic fixture"),
      },
    ],
    typeProviderStatus: "unknown",
    typeProviderPids: [],
    typeProviderReason:
      "no TypeScript provider process in the test-IDE tree (skeleton has no engine; JBT2 owns lifecycle)",
    ...overrides,
  };
}

function capture(overrides: Partial<IdeCapture> = {}): IdeCapture {
  const side = overrides.side ?? "verter";
  return {
    hostKind: REAL_IDE_HOST,
    usedSleepForReadiness: false,
    versions: {
      ideProduct: "WebStorm",
      ideBuild: "WS-262.10968.77",
      pluginVersion: "0.1.0-jbt1",
      engine: unknownMetric("skeleton carries no TypeScript engine (JBT2)"),
    },
    paint: {
      uiApplyPaintMs: { status: "measured", value: 12, unit: "ms" },
      rpcDurationMs: unknownMetric("no semantic RPC on the skeleton health-action path"),
    },
    processTree: tree(),
    workflows: defaultWorkflowRows(side),
    receiptBasis: BASIS,
    side,
    sessionState: "cold",
    basisKind: "fresh",
    ...overrides,
  };
}

describe("JBT1H-AC1 — mock LSP is not real JetBrains UI evidence", () => {
  it("admits a real-jetbrains-ide capture with measured ui-apply-paint", () => {
    const verdict = classifyUiEvidence(capture());
    expect(verdict.admissible).toBe(true);
    expect(ingestCapture(capture()).uiAdmissible).toBe(true);
  });

  it("rejects a mock LSP client as UI evidence", () => {
    const mock = capture({ hostKind: "mock-lsp" });
    const verdict = classifyUiEvidence(mock);
    expect(verdict.admissible).toBe(false);
    if (verdict.admissible) throw new Error("expected rejected UI evidence");
    expect(verdict.reason).toMatch(/mock LSP/i);
    expect(() => ingestCapture(mock)).toThrow(/JBT1H-AC1|mock-lsp/);
  });

  it("rejects raw-LSP, screenshot and protocol-smoke hosts", () => {
    for (const hostKind of ["raw-lsp", "screenshot", "protocol-smoke"] as const) {
      const verdict = classifyUiEvidence(capture({ hostKind }));
      expect(verdict.admissible).toBe(false);
      if (verdict.admissible) throw new Error("expected rejected UI evidence");
      expect(verdict.reason).toMatch(/cannot certify JetBrains UI/);
    }
  });

  it("rejects RPC-only duration as a real-client paint claim", () => {
    const rpcOnly = capture({
      paint: {
        uiApplyPaintMs: unknownMetric("not measured"),
        rpcDurationMs: { status: "measured", value: 4, unit: "ms" },
      },
    });
    const verdict = classifyUiEvidence(rpcOnly);
    expect(verdict.admissible).toBe(false);
    if (verdict.admissible) throw new Error("expected rejected UI evidence");
    expect(verdict.reason).toMatch(/ui-apply-paint/);
  });

  it("rejects sleep-as-readiness", () => {
    const slept = capture({ usedSleepForReadiness: true });
    expect(classifyUiEvidence(slept).admissible).toBe(false);
    expect(() => ingestCapture(slept)).toThrow(/sleep-as-readiness/);
  });
});

describe("JBT1H-AC2 — excluding the TypeScript provider invalidates memory", () => {
  it("invalidates a numeric retained-memory row when the provider is excluded", () => {
    const snapshot = tree({
      typeProviderStatus: "excluded",
      typeProviderReason: "provider process omitted from the sampled tree",
      members: [
        {
          pid: 10,
          parentPid: null,
          image: "idea",
          role: "ide",
          rssBytes: { status: "measured", value: 400_000_000, unit: "bytes" },
        },
      ],
    });
    const row = evaluateMemoryRow({
      tree: snapshot,
      retainedMemory: { status: "measured", value: 400_000_000, unit: "bytes" },
      providerCpu: { status: "measured", value: 12, unit: "ms" },
      providerWall: { status: "measured", value: 20, unit: "ms" },
      outboundBytes: unknownMetric("not instrumented"),
    });
    expect(row.status).toBe("invalidated");
    expect(row.retainedMemory.status).toBe("unknown");
    expect(row.reason).toMatch(/JBT1H-AC2/);
    expect(providerRequiredForMemory(snapshot).ok).toBe(false);
  });

  it("invalidates when the advertised provider pid is outside the tree", () => {
    const snapshot = tree({
      typeProviderStatus: "observed",
      typeProviderPids: [999],
      typeProviderReason: "advertised pid 999",
    });
    const row = evaluateMemoryRow({
      tree: snapshot,
      retainedMemory: { status: "measured", value: 1, unit: "bytes" },
      providerCpu: unknownMetric("n/a"),
      providerWall: unknownMetric("n/a"),
      outboundBytes: unknownMetric("n/a"),
    });
    expect(row.status).toBe("invalidated");
    expect(row.reason).toMatch(/not in the process tree/);
  });

  it("does not guess zeros: unknown provider keeps memory unknown, not measured 0", () => {
    const row = evaluateMemoryRow({
      tree: tree(),
      retainedMemory: unknownMetric("unmeasured"),
      providerCpu: unknownMetric("unmeasured"),
      providerWall: unknownMetric("unmeasured"),
      outboundBytes: unknownMetric("unmeasured"),
    });
    expect(row.status).toBe("unknown");
    expect(row.retainedMemory).toEqual(unknownMetric("unmeasured"));
    expect(row.retainedMemory).not.toEqual({ status: "measured", value: 0, unit: "bytes" });
  });

  it("accepts a memory row only when the provider pid is in the tree", () => {
    const snapshot = tree({
      typeProviderStatus: "observed",
      typeProviderPids: [20],
      typeProviderReason: "tsgo descendant of the IDE",
      members: [
        {
          pid: 10,
          parentPid: null,
          image: "idea",
          role: "ide",
          rssBytes: { status: "measured", value: 100, unit: "bytes" },
        },
        {
          pid: 20,
          parentPid: 10,
          image: "tsgo",
          role: "provider",
          rssBytes: { status: "measured", value: 50, unit: "bytes" },
        },
      ],
    });
    const row = evaluateMemoryRow({
      tree: snapshot,
      retainedMemory: { status: "measured", value: 150, unit: "bytes" },
      providerCpu: { status: "measured", value: 3, unit: "ms" },
      providerWall: { status: "measured", value: 5, unit: "ms" },
      outboundBytes: { status: "measured", value: 80, unit: "bytes" },
    });
    expect(row.status).toBe("valid");
    expect(row.retainedMemory.status).toBe("measured");
  });
});

describe("JBT1H-AC3 — swapped-order paired runs stay within the recorded noise bound", () => {
  it("records randomised order from the seed and inverts on the swapped seed", () => {
    expect(pairOrderFromSeed(2)).toEqual(["official", "verter"]);
    expect(pairOrderFromSeed(3)).toEqual(["verter", "official"]);
    expect(swappedOrder(pairOrderFromSeed(2))).toEqual(["verter", "official"]);
  });

  it("keeps failed runs and labels outliers instead of dropping them", () => {
    const official = capture({
      side: "official",
      paint: {
        uiApplyPaintMs: { status: "measured", value: 10, unit: "ms" },
        rpcDurationMs: unknownMetric("n/a"),
      },
    });
    const verter = capture({
      side: "verter",
      paint: {
        uiApplyPaintMs: { status: "measured", value: 40, unit: "ms" },
        rpcDurationMs: unknownMetric("n/a"),
      },
    });
    const pair = labelOutliers(
      recordPair({
        seed: 2,
        sessionState: "cold",
        captures: [official, verter],
        retainedFailed: [capture({ side: "verter", hostKind: REAL_IDE_HOST })],
      }),
      { maxAbsMs: 5, recordedAs: "campaign-predeclared" },
    );
    expect(pair.retainedFailed).toHaveLength(1);
    expect(pair.sides.some((side) => side.outlier)).toBe(true);
    expect(pair.sides).toHaveLength(2);
  });

  it("accepts two swapped pairs whose per-side paint stays inside the bound", () => {
    const official = capture({
      side: "official",
      paint: {
        uiApplyPaintMs: { status: "measured", value: 20, unit: "ms" },
        rpcDurationMs: unknownMetric("n/a"),
      },
    });
    const verter = capture({
      side: "verter",
      paint: {
        uiApplyPaintMs: { status: "measured", value: 22, unit: "ms" },
        rpcDurationMs: unknownMetric("n/a"),
      },
    });
    const campaign = runPairedCampaign({
      seed: 2,
      sessionState: "cold",
      official,
      verter,
      noiseBound: { maxAbsMs: 8, recordedAs: "campaign-predeclared" },
    });
    expect(campaign.first.order).toEqual(["official", "verter"]);
    expect(campaign.swapped.order).toEqual(["verter", "official"]);
    expect(campaign.comparable.comparable).toBe(true);
  });

  it("rejects swapped pairs that exceed the recorded noise bound", () => {
    const first = recordPair({
      seed: 2,
      sessionState: "cold",
      captures: [
        capture({
          side: "official",
          paint: {
            uiApplyPaintMs: { status: "measured", value: 10, unit: "ms" },
            rpcDurationMs: unknownMetric("n/a"),
          },
        }),
        capture({
          side: "verter",
          paint: {
            uiApplyPaintMs: { status: "measured", value: 11, unit: "ms" },
            rpcDurationMs: unknownMetric("n/a"),
          },
        }),
      ],
    });
    const second = recordPair({
      seed: 3,
      sessionState: "cold",
      captures: [
        capture({
          side: "official",
          paint: {
            uiApplyPaintMs: { status: "measured", value: 40, unit: "ms" },
            rpcDurationMs: unknownMetric("n/a"),
          },
        }),
        capture({
          side: "verter",
          paint: {
            uiApplyPaintMs: { status: "measured", value: 12, unit: "ms" },
            rpcDurationMs: unknownMetric("n/a"),
          },
        }),
      ],
    });
    const verdict = compareSwappedPairs(first, second, {
      maxAbsMs: 5,
      recordedAs: "campaign-predeclared",
    });
    expect(verdict.comparable).toBe(false);
    expect(verdict.reason).toMatch(/noise bound/);
  });
});

describe("fixture driver and receipt basis", () => {
  it("pins the equal-work hermetic corpus and the JBT0 workflow population", () => {
    expect(FIXTURE_CORPUS.requiredFeaturesEnabled).toBe(true);
    expect(FIXTURE_CORPUS.diagnosticsEnabled).toBe(true);
    expect(COMPARISON_WORKFLOWS).toHaveLength(12);
    const matrix = JSON.parse(
      readFileSync(
        path.join(
          repoRoot,
          "tests/jetbrains-baseline/JBT0/products/required-workflow-matrix.v1.json",
        ),
        "utf8",
      ),
    ) as { workflows: { id: string }[] };
    expect(matrix.workflows.map((row) => row.id)).toEqual([...COMPARISON_WORKFLOWS]);
  });

  it("records Verter skeleton workflows as unsupported, never as a false complete", () => {
    for (const row of defaultWorkflowRows("verter")) {
      expect(row.completeness).toBe("unsupported");
      expect(row.reason).toBe(SKELETON_UNSUPPORTED_REASON);
    }
  });

  it("parses a Kotlin-shaped capture and refuses a guessed unknown-with-value metric", () => {
    const parsed = parseIdeCapture(capture({ side: "official" }));
    expect(parsed.hostKind).toBe(REAL_IDE_HOST);
    expect(parsed.versions.ideBuild).toMatch(/262/);
    expect(() =>
      parseIdeCapture({
        ...capture(),
        paint: {
          uiApplyPaintMs: { status: "unknown", reason: "missing", value: 0 },
          rpcDurationMs: unknownMetric("n/a"),
        },
      }),
    ).toThrow(/must not carry a value/);
  });

  it("binds incremental vs fresh on the same receipt basis", () => {
    const fresh = capture({ basisKind: "fresh" });
    const incremental = capture({ basisKind: "incremental" });
    expect(sameReceiptBasis(fresh.receiptBasis, incremental.receiptBasis)).toBe(true);
    expect(fresh.basisKind).not.toBe(incremental.basisKind);
    expect(sameReceiptBasis(fresh.receiptBasis, { ...BASIS, sourceRevisions: "old-source" })).toBe(
      false,
    );
  });

  it("does not count a mock-LSP capture that sneaks in through parse+ingest", () => {
    const parsed = parseIdeCapture(capture({ hostKind: "mock-lsp" }));
    expect(classifyUiEvidence(parsed).admissible).toBe(false);
    expect(() => ingestCapture(parsed)).toThrow(/mock-lsp/);
  });
});
