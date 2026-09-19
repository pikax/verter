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
  ISSUE93_RETIREMENT,
  LARGE_PROJECT_MIN_VUE_FILES,
  PRIMEVUE_EQUIVALENT_VUE_FILES,
  extractCapturedSemanticOracle,
  extractLspJson,
  issue93CarryForwardCase,
  issue93EarlyVerification,
  loadRecordedLapceCapture,
  qualifyIssue93EarlyVerification,
  typedAnswersFromOracle,
  RealLapceHost,
  LapceInteractionDriver,
  measuredMetric,
  unknownMetric,
  type Issue93EarlyVerificationInput,
  type LargeRunObservation,
  type ProductReceiptBasis,
  type SemanticOracleObservation,
  type SmallSmokeObservation,
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
  diagnosticCount: measuredMetric(3, "count"),
  typedAnswers: [
    {
      kind: "completion",
      expected: ["greeting", "user", "message"],
      observed: ["greeting", "user", "message"],
      match: true,
    },
    {
      kind: "definition",
      expected: ["Fixture.vue"],
      observed: ["file:///ws/Fixture.vue"],
      match: true,
    },
    { kind: "diagnostics", expected: ["counted"], observed: ["3"], match: true },
  ],
  requiredFeaturesEnabled: true,
};

const RECEIPT: ProductReceiptBasis = {
  sourceRevisions: "candidate:test",
  projectConfiguration: "large-project",
  engineIdentity: "verter-lsp 0.0.1-beta.5",
  hostIdentity: "real-lapce:0.4.6+Nightly.126d356",
  completenessState: "partial",
};

function largeRun(overrides: Partial<LargeRunObservation> = {}): LargeRunObservation {
  return {
    hostKind: "real-lapce",
    protocolSmokeOnly: false,
    uiStallDetected: false,
    serverImmediate: true,
    realClientEvidence: true,
    completenessState: "partial",
    usedSleepForReadiness: false,
    versionsPinned: true,
    interactionKinds: ["open", "type", "complete", "navigate", "close"],
    reopenAfterClose: true,
    diagnosticsObserved: true,
    duration: measuredMetric(12_000, "ms"),
    workload: LARGE_WORKLOAD,
    oracle: ORACLE_OK,
    resources: {
      clientPaint: measuredMetric(18, "ms"),
      providerProcessCost: unknownMetric("provider CPU not sampled on this host"),
      outboundBytes: measuredMetric(48_000, "bytes"),
      retainedMemory: unknownMetric("process-tree RSS not sampled on this host"),
    },
    claimsIssue93Fixed: false,
    finalOwner: "expansion.workspace-responsiveness",
    duplicatedAuthority: false,
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
});

describe("WSP1B qualified current behavior is not a repair claim", () => {
  it("qualifies a complete real-client large run and keeps issue 93 unreproduced", () => {
    const verification = issue93EarlyVerification(input());
    expect(verification.status).toBe("qualified");
    expect(verification.historicalIssue93).toEqual(ISSUE93_RETIREMENT);
    expect(verification.historicalIssue93.notAFix).toBe(true);
    const carry = issue93CarryForwardCase(verification);
    expect(carry.id).toBe("Issue93CarryForwardCase");
    expect(carry.reusedAfter).toEqual(["WSP8", "ED1"]);
    expect(carry.qualificationStatus).toBe("qualified");
    expect(carry.historicalIssue93.state).toBe("unreproduced");
    expect(carry.workload.vueFileCount).toBe(PRIMEVUE_EQUIVALENT_VUE_FILES);
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
    expect(product.status).toBe("qualified");
    expect(product.historicalIssue93.notAFix).toBe(true);
    expect(product.historicalIssue93.state).toBe("unreproduced");
    driver.teardown();
  });
});

describe("captured-line semantic oracle extract", () => {
  it("counts publishDiagnostics, completion labels, definition uris and outbound bytes", () => {
    const lines = [
      `prefix write to lsp: ${JSON.stringify({ jsonrpc: "2.0", method: "textDocument/completion", id: 1 })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        method: "textDocument/publishDiagnostics",
        params: {
          uri: "file:///ws/Fixture.vue",
          diagnostics: [{ message: "a" }, { message: "b" }],
        },
      })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        result: { isIncomplete: false, items: [{ label: "greeting" }, { label: "user" }] },
      })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        id: 2,
        result: { uri: "file:///ws/Fixture.vue", range: { start: { line: 1, character: 6 } } },
      })}`,
      `prefix read from lsp: ${JSON.stringify({
        jsonrpc: "2.0",
        method: "$/verter/tsgoStarted",
        params: { pid: 4242 },
      })}`,
    ];
    const oracle = extractCapturedSemanticOracle(lines);
    expect(oracle.diagnosticCount).toBe(2);
    expect(oracle.completionLabels).toEqual(["greeting", "user"]);
    expect(oracle.definitionUris).toEqual(["file:///ws/Fixture.vue"]);
    expect(oracle.providerPid).toBe(4242);
    expect(oracle.outboundBytes).toBeGreaterThan(0);
    const answers = typedAnswersFromOracle(oracle, {
      completionLabels: ["greeting", "user"],
      definitionNeedle: "Fixture.vue",
    });
    expect(answers.every((answer) => answer.match)).toBe(true);
  });

  it("returns null for a line without an LSP marker instead of guessing", () => {
    expect(extractLspJson("verter ui-stamp {}", "read from lsp:")).toBeNull();
  });
});
