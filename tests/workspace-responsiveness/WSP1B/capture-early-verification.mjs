// Record the WSP1B large-project real-Lapce capture and qualification products.
//
// node tests/workspace-responsiveness/WSP1B/capture-early-verification.mjs \
//   --lapce <instrumented lapce binary> --volt <volt dir> --server <verter-lsp binary>
//
// Generates the WSP equal-work synthetic SFC slice (2615 Vue files; PrimeVue-scale
// equivalent), plants the WSP1L oracle Fixture.vue, drives open/type/complete/
// navigate/close/reopen on the instrumented client, and writes the sealed capture
// plus Issue93EarlyVerification / Issue93CarryForwardCase products.
// Reference-machine tooling only — never part of hermetic CI.

import { createHash } from "node:crypto";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

import {
  ISSUE93_ORACLE_EXPECTATION,
  ISSUE93_RETIREMENT,
  PRIMEVUE_EQUIVALENT_VUE_FILES,
  REAL_LAPCE_AUTOMATION_PATH,
  RealLapceHost,
  LapceInteractionDriver,
  buildDrivenCaptureArtifact,
  carriesRealClientEvidence,
  correlateServerTraces,
  driveLapceSession,
  extractCapturedSemanticOracle,
  isProviderEnginePin,
  isSha256BuildPin,
  issue93CarryForwardCase,
  issue93EarlyVerification,
  loadRecordedLapceCapture,
  paintMetricFromRun,
  stallOnImmediateServer,
  typedAnswersFromOracle,
  versionsMatchPinnedManifest,
  writeDrivenCaptureArtifact,
  measuredMetric,
  unknownMetric,
} from "../../../packages/dx-harness/dist/lapce/index.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");

function arg(name, fallback) {
  const at = process.argv.indexOf(`--${name}`);
  return at !== -1 ? process.argv[at + 1] : fallback;
}

const lapceBin = path.resolve(arg("lapce"));
const voltDir = path.resolve(arg("volt"));
const serverBin = path.resolve(arg("server"));
const productsDir = path.join(here, "products");
const captureOut = path.resolve(
  arg("out", path.join(productsDir, "large-project-capture.v1.json")),
);
const patchPath = path.join(
  repoRoot,
  "packages/dx-harness/lapce/instrumented-client/lapce-0.4.6-wsp1l.patch",
);
const generator = path.join(repoRoot, "test-corpora/perf/synthetic-15k/generator/generate.mjs");
const wsp1lFixture = path.join(here, "../WSP1L/fixtures/ws");

const workspaceDir = mkdtempSync(path.join(os.tmpdir(), "wsp1b-ws-"));
const corpusDir = path.join(workspaceDir, "corpus");

try {
  const generated = spawnSync(
    process.execPath,
    [
      generator,
      "--out",
      corpusDir,
      "--count",
      String(PRIMEVUE_EQUIVALENT_VUE_FILES),
      "--modules",
      "80",
      "--composite",
      "8",
      "--quiet",
    ],
    { cwd: repoRoot, encoding: "utf8" },
  );
  if (generated.status !== 0) {
    throw new Error(`corpus generator failed: ${generated.stderr || generated.stdout}`);
  }

  cpSync(path.join(wsp1lFixture, "Fixture.vue"), path.join(workspaceDir, "Fixture.vue"));
  cpSync(path.join(wsp1lFixture, "Helper.vue"), path.join(workspaceDir, "Helper.vue"));
  writeFileSync(
    path.join(workspaceDir, "tsconfig.json"),
    `${JSON.stringify(
      {
        compilerOptions: {
          target: "ESNext",
          module: "ESNext",
          moduleResolution: "Bundler",
          strict: true,
        },
        include: ["Fixture.vue", "Helper.vue", "corpus/**/*.vue"],
      },
      null,
      2,
    )}\n`,
  );
  mkdirSync(path.join(workspaceDir, ".lapce"), { recursive: true });
  writeFileSync(
    path.join(workspaceDir, ".lapce", "settings.toml"),
    `[verter-volt]\nuiTrace.enabled = true\nlsp.serverPath = '${serverBin.replaceAll("\\", "/")}'\n`,
  );

  const fixture = path.join(workspaceDir, "Fixture.vue");
  const helper = path.join(workspaceDir, "Helper.vue");
  // Same-URI reopen of Fixture.vue after close does not emit a paint ack on
  // this client (180s timeout). The live drive therefore reopens a not-yet-
  // opened corpus SFC so recapture can finish; sameDocumentReopen stays false
  // and qualification stays unverified until that paint exists (WSP1B.3).
  const reopen = path.join(workspaceDir, "corpus", "kernel", "k0000", "Comp0000_000.vue");
  const script = [
    { kind: "open", requestEpoch: 1, sourceEpoch: null, uri: helper },
    { kind: "open", requestEpoch: 2, sourceEpoch: null, uri: fixture, line: 4, column: 20 },
    { kind: "navigate", requestEpoch: 3, sourceEpoch: null },
    { kind: "type", requestEpoch: 4, sourceEpoch: null, text: " " },
    { kind: "complete", requestEpoch: 5, sourceEpoch: null },
    { kind: "type", requestEpoch: 6, sourceEpoch: null, text: " " },
    { kind: "complete", requestEpoch: 7, sourceEpoch: null },
    { kind: "type", requestEpoch: 8, sourceEpoch: null, text: " " },
    { kind: "complete", requestEpoch: 9, sourceEpoch: null },
    { kind: "close", requestEpoch: 10, sourceEpoch: null },
    { kind: "open", requestEpoch: 11, sourceEpoch: null, uri: reopen, line: 1, column: 1 },
  ];
  const sameDocumentReopen = path.resolve(reopen) === path.resolve(fixture);

  const session = await driveLapceSession({
    lapceBin,
    workspaceDir,
    voltDir,
    script,
    readyTimeoutMs: 600_000,
    stepTimeoutMs: 180_000,
  });
  const correlated = correlateServerTraces(session);
  const artifact = buildDrivenCaptureArtifact({
    session,
    correlated,
    recordedAs: path.relative(repoRoot, captureOut).replaceAll("\\", "/"),
    lapceClientSource:
      "Lapce v0.4.6 (github.com/lapce/lapce tag v0.4.6 source tarball) + the WSP1L instrumented-client patch",
    patchPath,
    automationPath: REAL_LAPCE_AUTOMATION_PATH,
  });
  writeDrivenCaptureArtifact(artifact, captureOut);

  const recorded = loadRecordedLapceCapture(captureOut);
  const host = new RealLapceHost(recorded.session);
  const driver = new LapceInteractionDriver({
    host,
    serverTraces: recorded.serverTraces,
    versions: recorded.versions,
    stallThreshold: { stallThresholdMs: 5_000, recordedAs: "WSP1B large-project stall threshold" },
    immediacyBound: { maxServerTotalMs: 5_000, recordedAs: "WSP1B large-project immediacy bound" },
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
  driver.teardown();
  const stall = stallOnImmediateServer(run);
  const capturedOracle = extractCapturedSemanticOracle(session.capturedLines);
  const typedAnswers = typedAnswersFromOracle(capturedOracle, ISSUE93_ORACLE_EXPECTATION);
  const lastStamp = Math.max(...recorded.session.uiStamps.map((stamp) => stamp.atUnixMs));
  const durationMs = lastStamp - session.startedUnixMs;
  const serverBuildPin = `sha256:${createHash("sha256").update(readFileSync(serverBin)).digest("hex")}`;
  const providerEnginePin = capturedOracle.providerVersion ?? "";
  const versionsPinned =
    versionsMatchPinnedManifest(recorded.versions).ok &&
    isSha256BuildPin(serverBuildPin) &&
    isProviderEnginePin(providerEnginePin);
  const realClientEvidence = carriesRealClientEvidence(run);
  const completenessState = "partial";

  const verification = issue93EarlyVerification({
    smallSmoke: {
      certified: true,
      uiStallDetected: false,
      vueFileCount: 2,
    },
    largeRun: {
      hostKind: run.hostKind,
      protocolSmokeOnly: false,
      uiStallDetected: stall.uiStallDetected,
      serverImmediate: stall.serverImmediate,
      realClientEvidence,
      completenessState,
      usedSleepForReadiness: false,
      versionsPinned,
      serverBuildPin,
      providerEnginePin,
      interactionKinds: recorded.steps.map((step) => step.kind),
      reopenAfterClose: true,
      sameDocumentReopen,
      diagnosticsObserved: capturedOracle.diagnosticsByUri.some((set) => set.count > 0),
      duration: measuredMetric(durationMs, "ms"),
      workload: {
        id: "wsp-equal-work-synthetic-sfc-slice",
        class: "large-project",
        vueFileCount: PRIMEVUE_EQUIVALENT_VUE_FILES,
        sourcePin:
          "test-corpora/perf/synthetic-15k/generator/generate.mjs generatorVersion=1.1.0 count=2615 seed=0x5eed15",
        configurationPin:
          "planted Fixture.vue + Helper.vue under a tsconfig that includes the generated corpus",
        privateCorpusLabel: "operator PrimeVue 2615 Vue files (unlabeled, not published)",
      },
      oracle: {
        diagnosticCount: measuredMetric(capturedOracle.diagnosticCount, "count"),
        typedAnswers,
        requiredFeaturesEnabled: true,
      },
      resources: {
        clientPaint: paintMetricFromRun(run),
        providerProcessCost: unknownMetric(
          capturedOracle.providerPid === null
            ? "no $/verter/tsgoStarted pid observed"
            : `provider pid ${capturedOracle.providerPid} observed; CPU not sampled`,
        ),
        outboundBytes: measuredMetric(capturedOracle.outboundBytes, "bytes"),
        retainedMemory: unknownMetric("process-tree RSS not sampled on this capture"),
      },
      claimsIssue93Fixed: false,
      finalOwner: "expansion.workspace-responsiveness",
      duplicatedAuthority: false,
      captureProvenance: recorded.provenance,
      observedUiStamps: recorded.session.uiStamps,
    },
    receiptBasis: {
      sourceRevisions: `recorded-capture:${recorded.provenance.sessionId}`,
      projectConfiguration: "wsp-equal-work-synthetic-sfc-slice count=2615",
      engineIdentity: `verter-lsp ${serverBuildPin} / ${providerEnginePin || "provider-unpinned"}`,
      hostIdentity: `real-lapce:${recorded.provenance.lapceClientVersion}`,
      completenessState,
    },
  });
  const carry = issue93CarryForwardCase(verification);
  writeFileSync(
    path.join(productsDir, "issue93-early-verification.v1.json"),
    `${JSON.stringify({ ...verification, historicalIssue93: ISSUE93_RETIREMENT, capture: artifact.recordedAs, contentSha256: artifact.contentSha256 }, null, 2)}\n`,
  );
  writeFileSync(
    path.join(productsDir, "issue93-carry-forward.v1.json"),
    `${JSON.stringify(carry, null, 2)}\n`,
  );
  console.log(
    `recorded ${artifact.session.sessionId}: ${artifact.capturedLines.length} lines status=${verification.status} rule=${verification.rule} -> ${captureOut}`,
  );
} finally {
  rmSync(workspaceDir, { recursive: true, force: true });
}
