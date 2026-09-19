/**
 * WSP1B — Early actual-Lapce regression verification.
 *
 * Hermetic discriminators: a UI stall with an immediate server fails even
 * when the two-file smoke is green (AC1/AC3); oracle mismatch fails (AC2);
 * a protocol smoke, unpinned workload, missing GUI, issue-93 repair claim
 * or duplicated owner never become qualified.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  ISSUE93_ORACLE_EXPECTATION,
  ISSUE93_RETIREMENT,
  ISSUE93_RETAINED_SCRIPT,
  LARGE_PROJECT_MIN_VUE_FILES,
  OPERATOR_MANUAL_PROBE,
  PRIMEVUE_EQUIVALENT_VUE_FILES,
  extractCapturedSemanticOracle,
  extractLspJson,
  issue93CarryForwardCase,
  issue93EarlyVerification,
  loadRecordedLapceCapture,
  paintMetricFromRun,
  qualifyIssue93EarlyVerification,
  typedAnswersFromOracle,
  uriPathEndsWith,
  RealLapceHost,
  LapceInteractionDriver,
  measuredMetric,
  unknownMetric,
  uiStampDigest,
  type Issue93EarlyVerificationInput,
  type LargeRunObservation,
  type ProductReceiptBasis,
  type SemanticOracleObservation,
  type SmallSmokeObservation,
  type UiStampPayload,
  type WorkloadPin,
} from "../lapce/index.js";

const GREEN_SMOKE: SmallSmokeObservation = {
  certified: true,
  uiStallDetected: false,
  vueFileCount: 2,
};

const LARGE_WORKLOAD: WorkloadPin = {
  id: "wsp-equal-work-synthetic-sfc-slice",
  class: "large-project",
  vueFileCount: PRIMEVUE_EQUIVALENT_VUE_FILES,
  sourcePin: "test-corpora/perf/synthetic-15k/generator/generate.mjs#1.1.0 count=2615",
  configurationPin: "tsconfig include Fixture.vue + generated corpus",
  privateCorpusLabel: "operator PrimeVue 2615 Vue files (unlabeled, not published)",
};

const ORACLE_OK: SemanticOracleObservation = {
  diagnosticCount: measuredMetric(7, "count"),
  typedAnswers: [
    {
      kind: "completion",
      expected: ["greeting", "user", "message"],
      observed: ["greeting", "user", "message"],
      match: true,
    },
    {
      kind: "definition",
      expected: ["/Fixture.vue", "1:6-1:14"],
      observed: ["file:///ws/Fixture.vue#1:6"],
      match: true,
    },
    {
      kind: "diagnostics",
      expected: ["/Fixture.vue:0", "/Helper.vue:0", "/Comp0000_000.vue:7"],
      observed: [
        "/Fixture.vue:0(expected 0)",
        "/Helper.vue:0(expected 0)",
        "/Comp0000_000.vue:7(expected 7)",
      ],
      match: true,
    },
  ],
  requiredFeaturesEnabled: true,
};

const RECEIPT: ProductReceiptBasis = {
  sourceRevisions: "candidate:test",
  projectConfiguration: "large-project",
  engineIdentity: "verter-lsp 0.0.1-beta.5",
  hostIdentity: "real-lapce:0.4.6+Nightly.126d356",
  completenessState: "complete",
};

const BOUND_STAMPS: readonly UiStampPayload[] = [
  { kind: "open", requestEpoch: 1, sourceEpoch: null, stage: "painted", atUnixMs: 10 },
];

const BOUND_PROVENANCE = {
  schema: "driven-lapce-capture.v1" as const,
  sessionId: "hermetic",
  recordedAs: "hermetic",
  captureSha256: uiStampDigest(BOUND_STAMPS),
  lapceClientVersion: "0.4.6+Nightly.126d356",
};

const CHURN_KINDS = [
  "open",
  "type",
  "complete",
  "type",
  "complete",
  "type",
  "complete",
  "navigate",
  "close",
] as const;

const SERVER_PIN = `sha256:${"ab".repeat(32)}`;

function largeRun(overrides: Partial<LargeRunObservation> = {}): LargeRunObservation {
  return {
    hostKind: "real-lapce",
    protocolSmokeOnly: false,
    uiStallDetected: false,
    serverImmediate: true,
    realClientEvidence: true,
    completenessState: "complete",
    usedSleepForReadiness: false,
    versionsPinned: true,
    serverBuildPin: SERVER_PIN,
    providerEnginePin: "7.0.2",
    interactionKinds: [...CHURN_KINDS],
    reopenAfterClose: true,
    sameDocumentReopen: true,
    diagnosticsObserved: true,
    duration: measuredMetric(12_000, "ms"),
    workload: LARGE_WORKLOAD,
    oracle: ORACLE_OK,
    resources: {
      clientPaint: measuredMetric(18, "ms"),
      providerProcessCost: measuredMetric(12, "ms"),
      outboundBytes: measuredMetric(48_000, "bytes"),
      retainedMemory: measuredMetric(256, "MiB"),
    },
    claimsIssue93Fixed: false,
    finalOwner: "expansion.workspace-responsiveness",
    duplicatedAuthority: false,
    captureProvenance: BOUND_PROVENANCE,
    observedUiStamps: BOUND_STAMPS,
    ...overrides,
  };
}

function input(
  overrides: Partial<Issue93EarlyVerificationInput> & { largeRun?: LargeRunObservation } = {},
): Issue93EarlyVerificationInput {
  return {
    smallSmoke: GREEN_SMOKE,
    largeRun: overrides.largeRun ?? largeRun(),
    receiptBasis: RECEIPT,
    ...overrides,
  };
}

describe("WSP1B-AC1 — UI freeze with an immediate server fails", () => {
  it("fails when the Lapce UI stalls while the server stays immediate", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ uiStallDetected: true, serverImmediate: true }) }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("WSP1B-AC1");
    expect(verdict.reason).toMatch(/small-project smoke remaining green/);
  });

  it("does not treat a slow server as an AC1 UI freeze", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ uiStallDetected: false, serverImmediate: false }) }),
    );
    expect(verdict.rule).not.toBe("WSP1B-AC1");
  });
});

describe("WSP1B-AC2 — semantic oracle on the same basis", () => {
  it("fails when a required typed answer is missing", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          oracle: {
            ...ORACLE_OK,
            typedAnswers: [
              {
                kind: "completion",
                expected: ["greeting"],
                observed: ["unrelated"],
                match: false,
              },
            ],
          },
        }),
      }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("WSP1B-AC2");
  });

  it("fails when required features were disabled", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({ oracle: { ...ORACLE_OK, requiredFeaturesEnabled: false } }),
      }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("WSP1B-AC2");
  });

  it("stays unverified when diagnostic counts were never observed", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          oracle: {
            ...ORACLE_OK,
            diagnosticCount: unknownMetric("publishDiagnostics never appeared"),
          },
        }),
      }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("WSP1B-AC2");
  });

  it("fails AC2 when observed diagnostics drop from 7 to 0", () => {
    const dropped = typedAnswersFromOracle(
      {
        diagnosticCount: 0,
        diagnosticUris: ["file:///ws/corpus/kernel/k0000/Comp0000_000.vue"],
        diagnosticsByUri: [
          {
            uri: "file:///ws/Fixture.vue",
            version: 1,
            count: 0,
            messages: [],
          },
          {
            uri: "file:///ws/Helper.vue",
            version: 1,
            count: 0,
            messages: [],
          },
          {
            uri: "file:///ws/corpus/kernel/k0000/Comp0000_000.vue",
            version: 1,
            count: 0,
            messages: [],
          },
        ],
        completionLabels: ["greeting", "user", "message"],
        definitionLocations: [
          {
            uri: "file:///ws/Fixture.vue",
            range: ISSUE93_ORACLE_EXPECTATION.definition.range,
          },
        ],
        definitionUris: ["file:///ws/Fixture.vue"],
        outboundBytes: 12,
        providerPid: 1,
        providerVersion: "7.0.2",
      },
      ISSUE93_ORACLE_EXPECTATION,
    );
    expect(dropped.find((answer) => answer.kind === "diagnostics")?.match).toBe(false);
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          oracle: {
            diagnosticCount: measuredMetric(0, "count"),
            typedAnswers: dropped,
            requiredFeaturesEnabled: true,
          },
        }),
      }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("WSP1B-AC2");
  });
});

describe("WSP1B-AC3 — large scenario discriminates from a green small smoke", () => {
  it("refuses to qualify a two-file workload as the large-project scenario", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          workload: {
            ...LARGE_WORKLOAD,
            class: "small-smoke",
            vueFileCount: 2,
            sourcePin: "WSP1L two-file fixture",
          },
        }),
      }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("unpinned-workload");
    expect(LARGE_PROJECT_MIN_VUE_FILES).toBe(1000);
  });

  it("detects a large-project stall even when the small smoke stayed green", () => {
    const verification = issue93EarlyVerification(
      input({ largeRun: largeRun({ uiStallDetected: true, serverImmediate: true }) }),
    );
    expect(verification.smallSmoke.certified).toBe(true);
    expect(verification.smallSmoke.uiStallDetected).toBe(false);
    expect(verification.status).toBe("failed");
    expect(verification.rule).toBe("WSP1B-AC1");
    expect(verification.historicalIssue93.notAFix).toBe(true);
    expect(verification.historicalIssue93.state).toBe("unreproduced");
  });

  it("stays unverified when open/type/complete/navigate/close/reopen is incomplete", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ reopenAfterClose: false }) }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("WSP1B-AC3");
  });

  it("stays unverified for a one-edit script without sustained churn", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          interactionKinds: ["open", "type", "complete", "navigate", "close"],
        }),
      }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("WSP1B-AC3");
  });

  it("stays unverified when reopen is not the closed document", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ sameDocumentReopen: false }) }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("WSP1B-AC3");
  });
});

describe("WSP1B forbidden wrong-completes", () => {
  it("refuses a claim that issue 93 was repaired", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ claimsIssue93Fixed: true }) }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("issue93-not-a-fix");
  });

  it("refuses a protocol-smoke-only run", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({ protocolSmokeOnly: true, hostKind: "protocol-smoke" }),
      }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("protocol-smoke");
  });

  it("refuses sleep-as-readiness", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ usedSleepForReadiness: true }) }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("sleep-as-readiness");
  });

  it("refuses a stale basis", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ completenessState: "stale" }) }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("stale-basis");
  });

  it("refuses a stale receipt even when the run is complete", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ receiptBasis: { ...RECEIPT, completenessState: "stale" } }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("stale-basis");
  });

  it("refuses cancelled and failed receipts", () => {
    for (const state of ["cancelled", "failed"] as const) {
      const runVerdict = qualifyIssue93EarlyVerification(
        input({ largeRun: largeRun({ completenessState: state }) }),
      );
      expect(runVerdict.status).toBe("failed");
      expect(runVerdict.rule).toBe("WSP1B-AC-BASIS");
      const receiptVerdict = qualifyIssue93EarlyVerification(
        input({ receiptBasis: { ...RECEIPT, completenessState: state } }),
      );
      expect(receiptVerdict.status).toBe("failed");
      expect(receiptVerdict.rule).toBe("WSP1B-AC-BASIS");
    }
  });

  it("does not promote partial, pending, unsupported or ambiguous evidence", () => {
    for (const state of ["partial", "pending", "unsupported", "ambiguous"] as const) {
      const verdict = qualifyIssue93EarlyVerification(
        input({ largeRun: largeRun({ completenessState: state }) }),
      );
      expect(verdict.status).not.toBe("qualified");
      expect(verdict.status).toBe("unverified");
      expect(verdict.rule).toBe("WSP1B-AC-BASIS");
    }
  });

  it("refuses a duplicated owner", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({ largeRun: largeRun({ duplicatedAuthority: true }) }),
    );
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("WSP1B-AC-OWNER");
  });

  it("stays unverified without real-client GUI evidence", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({ realClientEvidence: false, hostKind: "instrumented-fixture" }),
      }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("missing-gui");
  });

  it("stays unverified when provenance does not bind the observed stamps", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          captureProvenance: { ...BOUND_PROVENANCE, captureSha256: "0".repeat(64) },
        }),
      }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("missing-gui");
  });

  it("fails a detected UI stall even when the small smoke is unavailable", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        smallSmoke: { certified: false, uiStallDetected: false, vueFileCount: 2 },
        largeRun: largeRun({ uiStallDetected: true, serverImmediate: false }),
      }),
    );
    expect(verdict.status).not.toBe("qualified");
    expect(verdict.status).toBe("failed");
    expect(verdict.rule).toBe("WSP1B-AC3");
  });

  it("stays unverified when provider cost or retained memory is unknown", () => {
    const provider = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          resources: {
            clientPaint: measuredMetric(18, "ms"),
            providerProcessCost: unknownMetric("CPU not sampled"),
            outboundBytes: measuredMetric(48_000, "bytes"),
            retainedMemory: measuredMetric(256, "MiB"),
          },
        }),
      }),
    );
    expect(provider.status).toBe("unverified");
    expect(provider.rule).toBe("WSP1B-AC-RESOURCE");
    const memory = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          resources: {
            clientPaint: measuredMetric(18, "ms"),
            providerProcessCost: measuredMetric(12, "ms"),
            outboundBytes: measuredMetric(48_000, "bytes"),
            retainedMemory: unknownMetric("RSS not sampled"),
          },
        }),
      }),
    );
    expect(memory.status).toBe("unverified");
    expect(memory.rule).toBe("WSP1B-AC-RESOURCE");
  });

  it("stays unverified without immutable server/provider build pins", () => {
    const verdict = qualifyIssue93EarlyVerification(
      input({
        largeRun: largeRun({
          versionsPinned: true,
          serverBuildPin: "0.0.1-beta.5",
          providerEnginePin: "tsgo",
        }),
      }),
    );
    expect(verdict.status).toBe("unverified");
    expect(verdict.rule).toBe("unpinned-workload");
  });
});

describe("WSP1B qualified current behavior is not a repair claim", () => {
  it("qualifies a complete real-client large run and keeps issue 93 unreproduced", () => {
    const verification = issue93EarlyVerification(input());
    expect(verification.status).toBe("qualified");
    expect(verification.historicalIssue93).toEqual(ISSUE93_RETIREMENT);
    expect(verification.historicalIssue93.notAFix).toBe(true);
    expect(verification.operatorManualProbe).toEqual(OPERATOR_MANUAL_PROBE);
    expect(verification.smallSmoke.surface).toBeDefined();
    const carry = issue93CarryForwardCase(verification);
    expect(carry.id).toBe("Issue93CarryForwardCase");
    expect(carry.reusedAfter).toEqual(["WSP8", "ED1"]);
    expect(carry.qualificationStatus).toBe("qualified");
    expect(carry.historicalIssue93.state).toBe("unreproduced");
    expect(carry.workload.vueFileCount).toBe(PRIMEVUE_EQUIVALENT_VUE_FILES);
    expect(carry.script.map((step) => step.kind)).toEqual(
      ISSUE93_RETAINED_SCRIPT.map((step) => step.kind),
    );
    expect(carry.script.some((step) => String(step.role).startsWith("reopen-"))).toBe(true);
    expect(carry.diagnostics.length).toBeGreaterThan(0);
    expect(carry.teardown.length).toBeGreaterThan(0);
  });
});

describe("WSP1B recorded large-project capture", () => {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
  const capturePath = path.join(
    repoRoot,
    "tests/workspace-responsiveness/WSP1B/products/large-project-capture.v1.json",
  );

  it("replays through RealLapceHost without claiming issue 93 was fixed", () => {
    const recorded = loadRecordedLapceCapture(capturePath);
    const host = new RealLapceHost(recorded.session);
    const driver = new LapceInteractionDriver({
      host,
      serverTraces: recorded.serverTraces,
      versions: recorded.versions,
      stallThreshold: {
        stallThresholdMs: 5_000,
        recordedAs: "WSP1B large-project stall threshold",
      },
      immediacyBound: {
        maxServerTotalMs: 5_000,
        recordedAs: "WSP1B large-project immediacy bound",
      },
      protocolSmokePassed: true,
      completenessState: "partial",
      receiptBasis: {
        sourceRevisions: `recorded-capture:${recorded.provenance.sessionId}`,
        projectConfiguration: "wsp-equal-work-synthetic-sfc-slice count=2615",
        engineIdentity: "verter-lsp (pinned manifest)",
        hostIdentity: `real-lapce:${recorded.provenance.lapceClientVersion}`,
        completenessState: "partial",
      },
      capture: { provenance: recorded.provenance, uiStamps: recorded.session.uiStamps },
    });
    const run = driver.runScriptedInteraction(recorded.steps);
    expect(run.hostKind).toBe("real-lapce");
    expect(run.timelines.length).toBe(recorded.steps.length);
    expect(run.timelines.some((timeline) => timeline.stall.detected)).toBe(false);
    const product = JSON.parse(
      readFileSync(
        path.join(
          repoRoot,
          "tests/workspace-responsiveness/WSP1B/products/issue93-early-verification.v1.json",
        ),
        "utf8",
      ),
    );
    expect(product.status).not.toBe("qualified");
    expect(product.historicalIssue93.notAFix).toBe(true);
    expect(product.historicalIssue93.state).toBe("unreproduced");
    expect(product.operatorManualProbe.limitations.length).toBeGreaterThan(0);
    driver.teardown();
  });
});

describe("captured-line semantic oracle extract", () => {
  const fixtureDiag = (
    uri: string,
    diagnostics: readonly { readonly message: string }[],
    version = 1,
  ) =>
    `prefix read from lsp: ${JSON.stringify({
      jsonrpc: "2.0",
      method: "textDocument/publishDiagnostics",
      params: { uri, version, diagnostics },
    })}`;

  it("counts latest publishDiagnostics, completion labels, definition locations and server outbound bytes", () => {
    const completionRequest = {
      jsonrpc: "2.0",
      method: "textDocument/completion",
      id: 1,
      params: { textDocument: { uri: "file:///ws/Fixture.vue" } },
    };
    const definitionRequest = {
      jsonrpc: "2.0",
      method: "textDocument/definition",
      id: 2,
      params: { textDocument: { uri: "file:///ws/Fixture.vue" } },
    };
    const definitionResult = {
      uri: "file:///ws/Fixture.vue",
      range: ISSUE93_ORACLE_EXPECTATION.definition.range,
    };
    const lines = [
      `prefix write to lsp: ${JSON.stringify(completionRequest)}`,
      fixtureDiag("file:///ws/Helper.vue", []),
      fixtureDiag("file:///ws/Fixture.vue", []),
      fixtureDiag("file:///ws/corpus/kernel/k0000/Comp0000_000.vue", [
        { message: "a" },
        { message: "b" },
        { message: "c" },
        { message: "d" },
        { message: "e" },
        { message: "f" },
        { message: "g" },
      ]),
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        result: {
          isIncomplete: false,
          items: [{ label: "greeting" }, { label: "user" }, { label: "message" }],
        },
      })}`,
      `prefix write to lsp: ${JSON.stringify(definitionRequest)}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        id: 2,
        result: definitionResult,
      })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        method: "$/verter/tsgoStarted",
        params: { pid: 4242, version: "7.0.2" },
      })}`,
    ];
    const oracle = extractCapturedSemanticOracle(lines);
    expect(oracle.diagnosticCount).toBe(7);
    expect(oracle.completionLabels).toEqual(["greeting", "user", "message"]);
    expect(oracle.definitionUris).toEqual(["file:///ws/Fixture.vue"]);
    expect(oracle.providerPid).toBe(4242);
    expect(oracle.providerVersion).toBe("7.0.2");
    expect(oracle.outboundBytes).toBeGreaterThan(0);
    const answers = typedAnswersFromOracle(oracle, ISSUE93_ORACLE_EXPECTATION);
    expect(answers.every((answer) => answer.match)).toBe(true);
  });

  it("counts server outbound bytes from read-from-lsp, not client writes", () => {
    const small = { jsonrpc: "2.0", id: 1, result: { ok: true } };
    const large = { jsonrpc: "2.0", id: 1, result: { ok: true, pad: "x".repeat(200) } };
    const write = {
      jsonrpc: "2.0",
      method: "textDocument/completion",
      id: 1,
      params: { pad: "y".repeat(400) },
    };
    const base = extractCapturedSemanticOracle([`prefix read from lsp: ${JSON.stringify(small)}`]);
    const enlargedResponse = extractCapturedSemanticOracle([
      `prefix read from lsp: ${JSON.stringify(large)}`,
    ]);
    const enlargedWrite = extractCapturedSemanticOracle([
      `prefix write to lsp: ${JSON.stringify(write)}`,
      `prefix read from lsp: ${JSON.stringify(small)}`,
    ]);
    expect(enlargedResponse.outboundBytes).toBeGreaterThan(base.outboundBytes);
    expect(enlargedWrite.outboundBytes).toBe(base.outboundBytes);
  });

  it("rejects a wrong definition path or range", () => {
    const oracle = extractCapturedSemanticOracle([
      `prefix write to lsp: ${JSON.stringify({ jsonrpc: "2.0", method: "textDocument/definition", id: 2 })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        id: 2,
        result: {
          uri: "file:///wrong/Fixture.vue.bak",
          range: { start: { line: 999, character: 0 }, end: { line: 999, character: 1 } },
        },
      })}`,
    ]);
    expect(uriPathEndsWith(oracle.definitionUris[0]!, "/Fixture.vue")).toBe(false);
    const answers = typedAnswersFromOracle(oracle, ISSUE93_ORACLE_EXPECTATION);
    expect(answers.find((answer) => answer.kind === "definition")?.match).toBe(false);
  });

  it("lets a later empty completion replace an earlier success", () => {
    const lines = [
      `prefix write to lsp: ${JSON.stringify({ jsonrpc: "2.0", method: "textDocument/completion", id: 1 })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        result: {
          isIncomplete: false,
          items: [{ label: "greeting" }, { label: "user" }, { label: "message" }],
        },
      })}`,
      `prefix write to lsp: ${JSON.stringify({ jsonrpc: "2.0", method: "textDocument/completion", id: 2 })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        id: 2,
        result: { isIncomplete: false, items: [] },
      })}`,
    ];
    const oracle = extractCapturedSemanticOracle(lines);
    expect(oracle.completionLabels).toEqual([]);
    const answers = typedAnswersFromOracle(oracle, ISSUE93_ORACLE_EXPECTATION);
    expect(answers.find((answer) => answer.kind === "completion")?.match).toBe(false);
  });

  it("uses the latest diagnostic population per URI, not the sum of publications", () => {
    const uri = "file:///ws/corpus/kernel/k0000/Comp0000_000.vue";
    const lines = [
      fixtureDiag("file:///ws/Helper.vue", []),
      fixtureDiag("file:///ws/Fixture.vue", []),
      fixtureDiag(
        uri,
        Array.from({ length: 7 }, (_, i) => ({ message: `d${i}` })),
        1,
      ),
      fixtureDiag(uri, [], 2),
    ];
    const oracle = extractCapturedSemanticOracle(lines);
    expect(oracle.diagnosticCount).toBe(0);
    expect(
      typedAnswersFromOracle(oracle).find((answer) => answer.kind === "diagnostics")?.match,
    ).toBe(false);
  });

  it("returns null for a line without an LSP marker instead of guessing", () => {
    expect(extractLspJson("verter ui-stamp {}", "read from lsp:")).toBeNull();
  });
});

describe("paintMetricFromRun", () => {
  it("propagates a missing paint on one required interaction", () => {
    const metric = paintMetricFromRun({
      timelines: [
        {
          step: { kind: "open" },
          inputToPaintMs: measuredMetric(12, "ms"),
        },
        {
          step: { kind: "type" },
          inputToPaintMs: unknownMetric("no paint ack"),
        },
      ],
    });
    expect(metric.status).toBe("unknown");
  });
});
