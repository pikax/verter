/**
 * WSP1L — Real-Lapce UI instrumentation and interaction driver.
 *
 * Hermetic proof of the charter's discriminating boundaries: a stalled UI
 * thread is detected while the server timeline shows immediate completion
 * (AC1); two runs of the same scripted interaction compare within a recorded
 * noise bound (AC2); a protocol-smoke-only run is never certified (AC3). The
 * fixture host is deterministic and is never counted as a real Lapce client.
 */
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import type { InteractionTrace, StageStamp } from "@verter/lsp-test-client";

import {
  FIXTURE_AUTOMATION_PATH,
  FixtureLapceHost,
  GUI_INSTRUMENTATION_UNAVAILABLE_REASON,
  LAPCE_FIXTURE_HOST,
  LAPCE_REAL_HOST,
  PINNED_LAPCE_VERSION_MANIFEST,
  LapceInteractionDriver,
  REAL_LAPCE_AUTOMATION_PATH,
  RealLapceHost,
  assertCertified,
  assertRealLapceProductClaim,
  buildUiTimeline,
  carriesRealClientEvidence,
  certifyRun,
  compareScriptedTimelines,
  detectUiStallWithImmediateServer,
  loadRecordedLapceCapture,
  versionsMatchPinnedManifest,
  collectStampMessages,
  parseLaunchStampValue,
  type LapceHost,
  type LapceUiRun,
  type LapceVersionManifest,
  type RealLapceCaptureSession,
  type ScriptedStep,
  type StallThreshold,
} from "../lapce/index.js";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const wsp1lProducts = path.join(repoRoot, "tests/workspace-responsiveness/WSP1L/products");

const STALL_THRESHOLD: StallThreshold = {
  stallThresholdMs: 100,
  recordedAs: "test-recorded fixture threshold",
};
const IMMEDIACY_BOUND = { maxServerTotalMs: 20, recordedAs: "test-recorded immediacy bound" };
const NOISE_BOUND = { maxAbsMs: 5, recordedAs: "test-recorded noise bound" };

/**
 * Immediate server trace: sub-millisecond stage gaps (the WSP1 blocked-stage
 * detector treats >= 1 ms gaps as blocked), nothing blocked. `sourceEpoch`
 * defaults to the request epoch; pass null to model a source-less trace.
 */
function immediateServerTrace(
  epoch: number,
  method = "textDocument/completion",
  sourceEpoch: number | null = epoch,
): InteractionTrace {
  const stamps: StageStamp[] = [
    { stage: "request_received", atMs: 1_000 },
    { stage: "admitted", atMs: 1_000.2 },
    { stage: "provider_work", atMs: 1_000.4 },
    { stage: "serialize", atMs: 1_000.8 },
    { stage: "outbound_enqueued", atMs: 1_000.8 },
    { stage: "outbound_written", atMs: 1_001.2 },
    { stage: "complete", atMs: 1_001.6 },
  ];
  return {
    requestEpoch: epoch,
    sourceEpoch,
    method,
    status: "complete",
    stamps,
    firstBlockedStage: null,
  };
}

/** Slow server trace: the provider_work gap is the blocked server stage. */
function stalledServerTrace(epoch: number): InteractionTrace {
  const stamps: StageStamp[] = [
    { stage: "request_received", atMs: 1_000 },
    { stage: "admitted", atMs: 1_001 },
    { stage: "provider_work", atMs: 1_002 },
    { stage: "serialize", atMs: 1_480 },
    { stage: "outbound_enqueued", atMs: 1_481 },
    { stage: "outbound_written", atMs: 1_482 },
    { stage: "complete", atMs: 1_483 },
  ];
  return {
    requestEpoch: epoch,
    sourceEpoch: epoch,
    method: "textDocument/hover",
    status: "complete",
    stamps,
    firstBlockedStage: "provider_work",
  };
}

function fixtureRun(overrides: Partial<LapceUiRun> = {}): LapceUiRun {
  const host = new FixtureLapceHost({ clockMs: sequenceClock(0) });
  const script: readonly ScriptedStep[] = [
    { kind: "open", requestEpoch: 1, sourceEpoch: 1 },
    { kind: "type", requestEpoch: 2, sourceEpoch: 2 },
    { kind: "complete", requestEpoch: 3, sourceEpoch: 3 },
    { kind: "navigate", requestEpoch: 4, sourceEpoch: 4 },
    { kind: "close", requestEpoch: 5, sourceEpoch: 5 },
  ];
  const timelines = script.map((step) =>
    buildUiTimeline(host.runScriptedStep(step), immediateServerTrace(step.requestEpoch), {
      stallThreshold: STALL_THRESHOLD,
      immediacyBound: IMMEDIACY_BOUND,
    }),
  );
  return {
    schema: "lapce-ui-run.v1",
    hostKind: LAPCE_FIXTURE_HOST,
    automationPath: FIXTURE_AUTOMATION_PATH,
    usedSleepForReadiness: false,
    versions: PINNED_LAPCE_VERSION_MANIFEST,
    protocolSmokePassed: true,
    timelines,
    receiptBasis: {
      sourceRevisions: "fixture:dx-harness-hermetic@pinned",
      projectConfiguration: "packages/dx-harness/fixtures/hermetic",
      engineIdentity: "unknown: fixture host carries no engine",
      hostIdentity: "instrumented-fixture:hermetic",
      completenessState: "partial",
    },
    completenessState: "partial",
    ...overrides,
  };
}

/** Deterministic monotonic clock with a controllable step (never a sleep). */
function sequenceClock(startMs: number, stepMs = 100): () => number {
  let current = startMs;
  return () => {
    const value = current;
    current += stepMs;
    return value;
  };
}

/**
 * Capture-contract fixture for the REAL path: the exact stderr line shapes a
 * driven Lapce session emits (`verter launch-stamp` / `verter ui-stamp`) plus
 * the capture-recorded clock anchor. It exercises the parsers, the
 * fail-closed RealLapceHost and the claim gate wiring end to end; it is a
 * CONTRACT fixture, not a recorded real capture — no recorded driven-client
 * capture stands behind it, so its runs stay inadmissible as real-client
 * paint evidence (only the committed recorded artifact is admissible).
 */
const REAL_CAPTURE_ANCHOR = {
  stampUnixMs: 1_758_000_000_000,
  timelineMs: 1_000,
  recordedAs: "test capture-contract clock anchor",
};

const REAL_CAPTURE_STDERR_LINES = [
  "lapce 0.4.6-test-reference: plugin volt loaded",
  'verter launch-stamp {"phase":"server_launch_issued","atUnixMs":1758000000000,' +
    '"serverUri":"urn:/opt/verter/verter-lsp","workspaceRoot":"/home/dev/proj",' +
    '"documentLanguages":["vue","svelte"]}',
  'verter ui-stamp {"kind":"type","requestEpoch":10,"sourceEpoch":null,' +
    '"stage":"input_dispatched","atUnixMs":1758000000012}',
  'verter ui-stamp {"kind":"type","requestEpoch":10,"sourceEpoch":null,' +
    '"stage":"decoded","atUnixMs":1758000000037}',
  'verter ui-stamp {"kind":"type","requestEpoch":10,"sourceEpoch":null,' +
    '"stage":"applied","atUnixMs":1758000000052}',
  'verter ui-stamp {"kind":"type","requestEpoch":10,"sourceEpoch":null,' +
    '"stage":"painted","atUnixMs":1758000000070}',
] as const;

/** The contract fixture certifies against the pinned manifest like any run. */
const REAL_CAPTURE_VERSIONS: LapceVersionManifest = PINNED_LAPCE_VERSION_MANIFEST;

/** A manifest whose lapce-client identity is unrecorded (negative case). */
const UNPINED_CLIENT_VERSIONS: LapceVersionManifest = {
  ...PINNED_LAPCE_VERSION_MANIFEST,
  items: PINNED_LAPCE_VERSION_MANIFEST.items.map((item) =>
    item.item === "lapce-client"
      ? {
          item: "lapce-client",
          version: null,
          status: "unrecorded" as const,
          reason: "test: identity not recorded for this run",
        }
      : item,
  ),
};

function realCaptureSession() {
  const { launchStamps, uiStamps } = collectStampMessages([...REAL_CAPTURE_STDERR_LINES]);
  return {
    lapceClientVersion: "0.4.6-test-reference",
    launchStamps,
    uiStamps,
    clockAnchor: REAL_CAPTURE_ANCHOR,
  };
}

function realCaptureRun(overrides: Partial<LapceUiRun> = {}): LapceUiRun {
  const host = new RealLapceHost(realCaptureSession());
  const driver = new LapceInteractionDriver({
    host,
    serverTraces: [immediateServerTrace(10, "textDocument/completion", null)],
    versions: REAL_CAPTURE_VERSIONS,
    stallThreshold: STALL_THRESHOLD,
    immediacyBound: IMMEDIACY_BOUND,
    protocolSmokePassed: true,
    receiptBasis: {
      sourceRevisions: "capture-contract:reference",
      projectConfiguration: "/home/dev/proj",
      engineIdentity: "unknown: capture contract carries no engine",
      hostIdentity: `real-lapce:${host.lapceClientVersion}`,
      completenessState: "partial",
    },
  });
  const run = driver.runScriptedInteraction([
    { kind: "type", requestEpoch: 10, sourceEpoch: null },
  ]);
  return { ...run, ...overrides };
}

describe("WSP1L-AC1 — stalled UI thread detected while the server timeline is immediate", () => {
  it("detects a deliberate UI stall and names the first blocked (client) stage", () => {
    const host = new FixtureLapceHost({
      clockMs: sequenceClock(0),
      stall: { afterStage: "applied", ms: 320 },
    });
    const timeline = buildUiTimeline(
      host.runScriptedStep({ kind: "type", requestEpoch: 1, sourceEpoch: 1 }),
      immediateServerTrace(1),
      { stallThreshold: STALL_THRESHOLD, immediacyBound: IMMEDIACY_BOUND },
    );
    const verdict = detectUiStallWithImmediateServer(timeline);
    expect(verdict.uiStallDetected).toBe(true);
    expect(verdict.serverImmediate).toBe(true);
    expect(verdict.firstBlockedStage).toEqual({ side: "ui", stage: "applied" });
    expect(verdict.reason).toMatch(/UI event loop stalled/);
    expect(verdict.reason).toMatch(/server completed immediately/);
  });

  it("keeps a server-side stall distinct: not a UI stall, first blocked stage is server", () => {
    const host = new FixtureLapceHost({ clockMs: sequenceClock(0) });
    const timeline = buildUiTimeline(
      host.runScriptedStep({ kind: "complete", requestEpoch: 1, sourceEpoch: 1 }),
      stalledServerTrace(1),
      { stallThreshold: STALL_THRESHOLD, immediacyBound: IMMEDIACY_BOUND },
    );
    const verdict = detectUiStallWithImmediateServer(timeline);
    expect(verdict.uiStallDetected).toBe(false);
    expect(verdict.firstBlockedStage).toEqual({ side: "server", stage: "provider_work" });
    expect(verdict.reason).toMatch(/not a UI stall/);
  });

  it("reports no blocked stage when neither side stalls (no false positive)", () => {
    const host = new FixtureLapceHost({ clockMs: sequenceClock(0) });
    const timeline = buildUiTimeline(
      host.runScriptedStep({ kind: "type", requestEpoch: 1, sourceEpoch: 1 }),
      immediateServerTrace(1),
      { stallThreshold: STALL_THRESHOLD, immediacyBound: IMMEDIACY_BOUND },
    );
    const verdict = detectUiStallWithImmediateServer(timeline);
    expect(verdict.uiStallDetected).toBe(false);
    expect(verdict.firstBlockedStage).toBeNull();
  });

  it("does not certify a UI stall when server immediacy is undecided", () => {
    const host = new FixtureLapceHost({
      clockMs: sequenceClock(0),
      stall: { afterStage: "decoded", ms: 400 },
    });
    const incompleteServer: InteractionTrace = {
      requestEpoch: 1,
      sourceEpoch: 1,
      method: "textDocument/completion",
      status: "pending",
      stamps: [{ stage: "request_received", atMs: 1_000 }],
      firstBlockedStage: null,
    };
    const timeline = buildUiTimeline(
      host.runScriptedStep({ kind: "type", requestEpoch: 1, sourceEpoch: 1 }),
      incompleteServer,
      { stallThreshold: STALL_THRESHOLD, immediacyBound: IMMEDIACY_BOUND },
    );
    const verdict = detectUiStallWithImmediateServer(timeline);
    expect(verdict.uiStallDetected).toBe(false);
    expect(verdict.reason).toMatch(/not certified/);
  });
});

describe("WSP1L-AC2 — two runs of the same scripted interaction stay in the noise bound", () => {
  const script: readonly ScriptedStep[] = [
    { kind: "open", requestEpoch: 1, sourceEpoch: 1 },
    { kind: "type", requestEpoch: 2, sourceEpoch: 2 },
    { kind: "complete", requestEpoch: 3, sourceEpoch: 3 },
    { kind: "navigate", requestEpoch: 4, sourceEpoch: 4 },
    { kind: "close", requestEpoch: 5, sourceEpoch: 5 },
  ];

  function runInteraction(host: FixtureLapceHost) {
    return script.map((step) =>
      buildUiTimeline(host.runScriptedStep(step), immediateServerTrace(step.requestEpoch), {
        stallThreshold: STALL_THRESHOLD,
        immediacyBound: IMMEDIACY_BOUND,
      }),
    );
  }

  it("accepts two identical scripted runs and records the per-metric deltas", () => {
    const first = runInteraction(new FixtureLapceHost({ clockMs: sequenceClock(0) }));
    const second = runInteraction(new FixtureLapceHost({ clockMs: sequenceClock(0) }));
    expect(first).toHaveLength(script.length);
    for (let i = 0; i < script.length; i += 1) {
      const comparison = compareScriptedTimelines(first[i]!, second[i]!, NOISE_BOUND);
      expect(comparison.comparable).toBe(true);
      expect(comparison.unknownMetrics).toEqual([]);
    }
    const typeComparison = compareScriptedTimelines(first[1]!, second[1]!, NOISE_BOUND);
    expect(typeComparison.deltasMs.some((delta) => delta.metric === "input-to-paint")).toBe(true);
  });

  it("accepts two runs whose timings drift within the recorded bound", () => {
    const first = runInteraction(new FixtureLapceHost({ clockMs: sequenceClock(0) }));
    const second = runInteraction(
      new FixtureLapceHost({
        clockMs: sequenceClock(0),
        stageDeltas: { input_dispatched: 0, decoded: 6, applied: 11, painted: 15 },
      }),
    );
    for (let i = 0; i < script.length; i += 1) {
      expect(compareScriptedTimelines(first[i]!, second[i]!, NOISE_BOUND).comparable).toBe(true);
    }
  });

  it("rejects runs that exceed the recorded noise bound", () => {
    const first = runInteraction(new FixtureLapceHost({ clockMs: sequenceClock(0) }));
    const divergent = runInteraction(
      new FixtureLapceHost({
        clockMs: sequenceClock(0),
        stageDeltas: { input_dispatched: 0, decoded: 4, applied: 8, painted: 30 },
      }),
    );
    const comparison = compareScriptedTimelines(first[1]!, divergent[1]!, NOISE_BOUND);
    expect(comparison.comparable).toBe(false);
    expect(comparison.reason).toMatch(/noise bound/);
  });

  it("refuses comparability on unknown input-to-paint instead of guessing zeros", () => {
    const first = runInteraction(new FixtureLapceHost({ clockMs: sequenceClock(0) }));
    const missingPaint = runInteraction(
      new FixtureLapceHost({ clockMs: sequenceClock(0), omitStages: ["painted"] }),
    );
    const comparison = compareScriptedTimelines(first[1]!, missingPaint[1]!, NOISE_BOUND);
    expect(comparison.comparable).toBe(false);
    expect(comparison.unknownMetrics).toContain("input-to-paint");
    expect(comparison.reason).toMatch(/unknown/);
    expect(missingPaint[1]!.inputToPaintMs).toEqual({
      status: "unknown",
      reason: "painted not recorded",
    });
  });

  it("refuses to compare different interactions", () => {
    const a = runInteraction(new FixtureLapceHost({ clockMs: sequenceClock(0) }))[1]!;
    const b = runInteraction(new FixtureLapceHost({ clockMs: sequenceClock(0) }))[2]!;
    const comparison = compareScriptedTimelines(a, b, NOISE_BOUND);
    expect(comparison.comparable).toBe(false);
    expect(comparison.reason).toMatch(/not the same scripted interaction/);
  });
});

describe("WSP1L-AC3 — the driver refuses to certify protocol-smoke-only runs", () => {
  it("refuses certification when the smoke passed but no UI timeline was captured", () => {
    const verdict = certifyRun(fixtureRun({ timelines: [] }));
    expect(verdict.certified).toBe(false);
    if (verdict.certified) throw new Error("expected refusal");
    expect(verdict.rule).toBe("WSP1L-AC3");
    expect(verdict.reason).toMatch(/protocol smoke passed but no UI timeline/);
    expect(() => assertCertified(fixtureRun({ timelines: [] }))).toThrow(/WSP1L-AC3/);
  });

  it("certifies the same run once its UI timelines are present", () => {
    const verdict = certifyRun(fixtureRun());
    expect(verdict.certified).toBe(true);
  });

  it("refuses rejected hosts even when timelines exist", () => {
    for (const hostKind of ["protocol-smoke", "mock-lsp", "screenshot", "raw-lsp"] as const) {
      const verdict = certifyRun(fixtureRun({ hostKind }));
      expect(verdict.certified).toBe(false);
      if (verdict.certified) throw new Error("expected refusal");
      expect(verdict.reason).toMatch(/cannot certify a Lapce UI timeline/);
    }
  });

  it("refuses sleep-as-readiness and stale runs", () => {
    expect(certifyRun(fixtureRun({ usedSleepForReadiness: true })).certified).toBe(false);
    const stale = certifyRun(fixtureRun({ completenessState: "stale" }));
    expect(stale.certified).toBe(false);
    if (stale.certified) throw new Error("expected refusal");
    expect(stale.rule).toBe("stale-basis");
  });

  it("refuses runs whose versions drifted from the pinned manifest", () => {
    const drifted = {
      ...PINNED_LAPCE_VERSION_MANIFEST,
      items: PINNED_LAPCE_VERSION_MANIFEST.items.map((item) =>
        item.item === "verter-lsp-server" ? { ...item, version: "0.0.1-beta.4" } : item,
      ),
    };
    const verdict = certifyRun(fixtureRun({ versions: drifted }));
    expect(verdict.certified).toBe(false);
    if (verdict.certified) throw new Error("expected refusal");
    expect(verdict.rule).toBe("version-manifest-drift");
    expect(verdict.reason).toMatch(/verter-lsp-server/);
  });
});

describe("LapceInteractionDriver — scripted open/type/complete/navigate/close/teardown", () => {
  it("drives the full script, correlates every step with its server epoch, and tears down", () => {
    const driver = makeFixtureDriver();
    const run = driver.runScriptedInteraction([
      { kind: "open", requestEpoch: 1, sourceEpoch: 1 },
      { kind: "type", requestEpoch: 2, sourceEpoch: 2 },
      { kind: "complete", requestEpoch: 3, sourceEpoch: 3 },
      { kind: "navigate", requestEpoch: 4, sourceEpoch: 4 },
      { kind: "close", requestEpoch: 5, sourceEpoch: 5 },
    ]);
    expect(run.timelines).toHaveLength(5);
    expect(run.timelines.map((timeline) => timeline.step.kind)).toEqual([
      "open",
      "type",
      "complete",
      "navigate",
      "close",
    ]);
    for (const timeline of run.timelines) {
      expect(timeline.server.requestEpoch).toBe(timeline.step.requestEpoch);
      expect(timeline.server.serverImmediate).toBe(true);
      expect(timeline.schema).toBe("ui-timeline.v1");
    }
    expect(certifyRun(run).certified).toBe(true);
    driver.teardown();
    driver.teardown(); // idempotent
    expect(() => driver.type("after teardown")).toThrow(/torn down/);
  });

  it("honors the script's request/source epochs when joining WSP1 traces (WSP1L.2)", () => {
    // A WSP1 replay whose epochs are not 1..n in drive order: the script's
    // epochs are the correlation keys and must reach the trace lookup verbatim.
    const driver = makeFixtureDriver([immediateServerTrace(10, "textDocument/hover", null)]);
    const run = driver.runScriptedInteraction([
      { kind: "type", label: "epoch-10", requestEpoch: 10, sourceEpoch: null },
    ]);
    expect(run.timelines).toHaveLength(1);
    const timeline = run.timelines[0]!;
    expect(timeline.step.requestEpoch).toBe(10);
    expect(timeline.step.sourceEpoch).toBeNull();
    expect(timeline.server.requestEpoch).toBe(10);
    expect(timeline.server.sourceEpoch).toBeNull();
    expect(timeline.server.method).toBe("textDocument/hover");
  });

  it("joins a step whose source epoch differs from its request epoch", () => {
    const driver = makeFixtureDriver([immediateServerTrace(10, "textDocument/completion", 7)]);
    const run = driver.runScriptedInteraction([{ kind: "open", requestEpoch: 10, sourceEpoch: 7 }]);
    const timeline = run.timelines[0]!;
    expect(timeline.step.requestEpoch).toBe(10);
    expect(timeline.step.sourceEpoch).toBe(7);
    expect(timeline.server.requestEpoch).toBe(10);
    expect(timeline.server.sourceEpoch).toBe(7);
  });

  it("refuses to bind a trace whose source epoch contradicts the step", () => {
    const driver = makeFixtureDriver([immediateServerTrace(10, "textDocument/hover", 8)]);
    expect(() =>
      driver.runScriptedInteraction([{ kind: "type", requestEpoch: 10, sourceEpoch: 7 }]),
    ).toThrow(/sourceEpoch .* refusing to bind the wrong trace/);
  });

  it("drives a request epoch at most once", () => {
    const driver = makeFixtureDriver();
    expect(() =>
      driver.runScriptedInteraction([
        { kind: "open", requestEpoch: 1, sourceEpoch: 1 },
        { kind: "type", requestEpoch: 1, sourceEpoch: 1 },
      ]),
    ).toThrow(/request epoch 1 was already driven/);
  });

  it("rejects invalid scripted epochs loud instead of guessing", () => {
    const driver = makeFixtureDriver();
    expect(() =>
      driver.runScriptedInteraction([{ kind: "type", requestEpoch: 0.5, sourceEpoch: 1 }]),
    ).toThrow(/requestEpoch must be a positive integer/);
    expect(() =>
      driver.runScriptedInteraction([{ kind: "type", requestEpoch: 2, sourceEpoch: 0 }]),
    ).toThrow(/sourceEpoch must be a positive integer or null/);
  });

  it("synthesizes only unused epochs for bare steps after scripted ones", () => {
    const driver = makeFixtureDriver([immediateServerTrace(10), immediateServerTrace(1)]);
    driver.runScriptedInteraction([{ kind: "open", requestEpoch: 10, sourceEpoch: 10 }]);
    const synthesized = driver.type("bare");
    expect(synthesized.step.requestEpoch).toBe(1);
    expect(synthesized.server.requestEpoch).toBe(1);
  });

  it("fails loud when a step has no correlatable server trace epoch", () => {
    const driver = makeFixtureDriver([immediateServerTrace(1)]);
    driver.open();
    expect(() => driver.type()).toThrow(/no WSP1 server InteractionTrace/);
  });

  it("teardown releases the host exactly once and the released host refuses steps", () => {
    const inner = new FixtureLapceHost({ clockMs: sequenceClock(0) });
    let releases = 0;
    const countingHost: LapceHost = {
      hostKind: inner.hostKind,
      automationPath: inner.automationPath,
      usedSleepForReadiness: inner.usedSleepForReadiness,
      runScriptedStep: (step) => inner.runScriptedStep(step),
      release: () => {
        releases += 1;
        inner.release();
      },
    };
    const driver = makeFixtureDriver([immediateServerTrace(1)], countingHost);
    driver.type();
    driver.teardown();
    driver.teardown(); // idempotent: the host releases exactly once
    expect(releases).toBe(1);
    expect(() =>
      countingHost.runScriptedStep({ kind: "type", requestEpoch: 99, sourceEpoch: 99 }),
    ).toThrow(/released/);
    expect(() => driver.type()).toThrow(/torn down/);
  });

  it("a released FixtureLapceHost refuses further steps loud (idempotent release)", () => {
    const host = new FixtureLapceHost({ clockMs: sequenceClock(0) });
    host.release();
    host.release();
    expect(() => host.runScriptedStep({ kind: "type", requestEpoch: 1, sourceEpoch: 1 })).toThrow(
      /released/,
    );
  });

  function makeFixtureDriver(traces?: readonly InteractionTrace[], host?: LapceHost) {
    return new LapceInteractionDriver({
      host: host ?? new FixtureLapceHost({ clockMs: sequenceClock(0) }),
      serverTraces: traces ?? [1, 2, 3, 4, 5].map((epoch) => immediateServerTrace(epoch)),
      versions: PINNED_LAPCE_VERSION_MANIFEST,
      stallThreshold: STALL_THRESHOLD,
      immediacyBound: IMMEDIACY_BOUND,
      protocolSmokePassed: true,
      receiptBasis: fixtureRun().receiptBasis,
    });
  }
});

describe("RealLapceHost — fail-closed real path (WSP1L.1/WSP1L.3)", () => {
  it("constructs only from a provenance-complete capture session", () => {
    expect(() => new RealLapceHost(realCaptureSession())).not.toThrow();
    const valid = realCaptureSession();
    const broken: Partial<RealLapceCaptureSession>[] = [
      { lapceClientVersion: "   " },
      { launchStamps: [] },
      {
        launchStamps: [
          parseLaunchStampValue({
            phase: "launch_refused",
            atUnixMs: 1,
            serverUri: "",
            workspaceRoot: "",
            documentLanguages: ["vue"],
            refusal: "no discovery source",
          }),
        ],
      },
      { uiStamps: [] },
      { clockAnchor: undefined },
    ];
    const reasons = [
      /pinned lapce-client identity/,
      /launch stamp/,
      /server_launch_issued/,
      /UI stage observations/,
      /clock anchor/,
    ];
    broken.forEach((override, index) => {
      expect(() => new RealLapceHost({ ...valid, ...override })).toThrow(reasons[index]);
    });
  });

  it("replays only observed stamps through the recorded clock anchor", () => {
    const host = new RealLapceHost(realCaptureSession());
    const record = host.runScriptedStep({ kind: "type", requestEpoch: 10, sourceEpoch: null });
    expect(record.stamps.map((stamp) => stamp.stage)).toEqual([
      "input_dispatched",
      "decoded",
      "applied",
      "painted",
    ]);
    // Anchor-mapped, never synthesized: stamp Unix 1758000000012 with the anchor
    // (1758000000000 -> timeline 1000) lands at timeline 1012.
    expect(record.stamps[0]!.atMs).toBe(1_012);
    expect(record.stamps[3]!.atMs).toBe(1_070);
    expect(host.launchStamps[0]!.phase).toBe("server_launch_issued");
  });

  it("refuses a scripted step the capture does not cover", () => {
    const host = new RealLapceHost(realCaptureSession());
    expect(() =>
      host.runScriptedStep({ kind: "navigate", requestEpoch: 11, sourceEpoch: null }),
    ).toThrow(/no UI stamps for step 'navigate'/);
  });

  it("release drops the retained capture and refuses further steps", () => {
    const host = new RealLapceHost(realCaptureSession());
    host.release();
    host.release(); // idempotent
    expect(() =>
      host.runScriptedStep({ kind: "type", requestEpoch: 10, sourceEpoch: null }),
    ).toThrow(/released/);
  });

  it("records the real host kind and real automation path on its runs", () => {
    const run = realCaptureRun();
    expect(run.hostKind).toBe(LAPCE_REAL_HOST);
    expect(run.automationPath).toEqual(REAL_LAPCE_AUTOMATION_PATH);
    expect(run.usedSleepForReadiness).toBe(false);
    expect(run.timelines[0]!.inputToPaintMs).toEqual({ status: "measured", value: 58, unit: "ms" });
  });
});

describe("real-client claims, pinned manifest and package surface", () => {
  it("never counts the fixture host as a real-client paint claim", () => {
    const verdict = assertRealLapceProductClaim(fixtureRun());
    expect(verdict.admissible).toBe(false);
    expect(verdict.reason).toMatch(/instrumented fixture host/);
    expect(verdict.reason).toMatch(GUI_INSTRUMENTATION_UNAVAILABLE_REASON);
  });

  it("refuses fixture timestamps relabeled with the real hostKind (WSP1L.3, AC-OWNER)", () => {
    // The wrong-complete trap: flipping hostKind alone promotes fixture-synthesized
    // stamps to a real-client paint claim. The gate must refuse it — a real-client
    // claim requires the real automation path, not a relabeled fixture record.
    const relabeled = fixtureRun({ hostKind: LAPCE_REAL_HOST });
    const verdict = assertRealLapceProductClaim(relabeled);
    expect(verdict.admissible).toBe(false);
    expect(verdict.reason).toMatch(/relabeled/);
    expect(verdict.reason).toMatch(/instrumented-fixture-host/);
    // The same defense on the certification surface: a certified fixture run is
    // explicit that it carries no real-client evidence.
    const certified = certifyRun(fixtureRun({ hostKind: LAPCE_REAL_HOST }));
    expect(certified.certified).toBe(true);
    if (certified.certified) {
      expect(certified.realClientEvidence).toBe(false);
    }
    expect(carriesRealClientEvidence(fixtureRun({ hostKind: LAPCE_REAL_HOST }))).toBe(false);
  });

  it("refuses the dual relabel: real hostKind AND real automation path labels on fixture stamps (F11)", () => {
    // Matching labels are not provenance: with BOTH labels flipped the run still
    // has no recorded driven-client capture behind it, so it carries no
    // real-client evidence and stays inadmissible as a product claim.
    const dual = fixtureRun({
      hostKind: LAPCE_REAL_HOST,
      automationPath: REAL_LAPCE_AUTOMATION_PATH,
    });
    const certified = certifyRun(dual);
    expect(certified.certified).toBe(true);
    if (certified.certified) {
      expect(certified.realClientEvidence).toBe(false);
    }
    expect(carriesRealClientEvidence(dual)).toBe(false);
    const verdict = assertRealLapceProductClaim(dual);
    expect(verdict.admissible).toBe(false);
    expect(verdict.reason).toMatch(/without a recorded driven-client capture provenance/);
  });

  it("refuses a capture provenance copied onto stamps that are not the recorded ones", () => {
    const recorded = loadRecordedLapceCapture(
      path.join(wsp1lProducts, "real-client-capture.v1.json"),
    );
    const forged = fixtureRun({
      hostKind: LAPCE_REAL_HOST,
      automationPath: REAL_LAPCE_AUTOMATION_PATH,
      versions: recorded.versions,
      captureProvenance: recorded.provenance,
      // Fixture timelines, and fixture stamps pretending to be observations.
      observedUiStamps: [
        {
          kind: "type",
          requestEpoch: 2,
          sourceEpoch: null,
          stage: "input_dispatched",
          atUnixMs: 1,
        },
        { kind: "type", requestEpoch: 2, sourceEpoch: null, stage: "painted", atUnixMs: 2 },
      ],
    });
    expect(carriesRealClientEvidence(forged)).toBe(false);
    const verdict = assertRealLapceProductClaim(forged);
    expect(verdict.admissible).toBe(false);
    expect(verdict.reason).toMatch(/cannot carry its provenance/);
  });

  it("hand-written capture lines never carry a real-client paint claim (WSP1L-ARCH)", () => {
    // REAL_CAPTURE_STDERR_LINES is a CONTRACT fixture: it proves the parsers
    // and the fail-closed host, but no recorded driven-client capture stands
    // behind it, so it is inadmissible as client paint evidence.
    const run = realCaptureRun();
    const verdict = assertRealLapceProductClaim(run);
    expect(verdict.admissible).toBe(false);
    expect(verdict.reason).toMatch(/without a recorded driven-client capture provenance/);
    expect(carriesRealClientEvidence(run)).toBe(false);
    const certified = certifyRun(run);
    expect(certified.certified).toBe(true);
    if (certified.certified) {
      expect(certified.realClientEvidence).toBe(false);
    }

    // Same lines, but with the lapce-client identity pinned and a measured
    // input-to-paint: still inadmissible — the missing piece is provenance,
    // not labels or numbers.
    const relabeledLines = realCaptureRun({
      versions: REAL_CAPTURE_VERSIONS,
      automationPath: REAL_LAPCE_AUTOMATION_PATH,
    });
    const pinned = assertRealLapceProductClaim(relabeledLines);
    expect(pinned.admissible).toBe(false);
    expect(pinned.reason).toMatch(/without a recorded driven-client capture provenance/);
  });

  it("admits a real-client claim only from a digest-verified recorded driven capture", () => {
    const recorded = loadRecordedLapceCapture(
      path.join(wsp1lProducts, "real-client-capture.v1.json"),
    );
    const host = new RealLapceHost(recorded.session);
    const driver = new LapceInteractionDriver({
      host,
      serverTraces: recorded.serverTraces,
      versions: recorded.versions,
      stallThreshold: STALL_THRESHOLD,
      immediacyBound: IMMEDIACY_BOUND,
      protocolSmokePassed: true,
      receiptBasis: {
        sourceRevisions: `recorded-capture:${recorded.provenance.sessionId}`,
        projectConfiguration: "tests/workspace-responsiveness/WSP1L/fixtures/ws",
        engineIdentity: "verter-lsp (pinned manifest)",
        hostIdentity: `real-lapce:${recorded.provenance.lapceClientVersion}`,
        completenessState: "partial",
      },
      capture: { provenance: recorded.provenance, uiStamps: recorded.session.uiStamps },
    });
    const run = driver.runScriptedInteraction(recorded.steps);
    expect(run.hostKind).toBe(LAPCE_REAL_HOST);
    expect(run.captureProvenance).toEqual(recorded.provenance);

    const verdict = assertRealLapceProductClaim(run);
    expect(verdict.admissible).toBe(true);
    expect(verdict.reason).toMatch(/recorded driven-client capture/);
    expect(verdict.reason).toContain(recorded.provenance.lapceClientVersion);

    const certified = certifyRun(run);
    expect(certified.certified).toBe(true);
    if (certified.certified) {
      expect(certified.realClientEvidence).toBe(true);
    }

    // The recorded run is a real driven capture: every step timeline is
    // client-observed, and each of the five interaction kinds was performed.
    expect(run.timelines.map((timeline) => timeline.step.kind)).toEqual([
      "open",
      "open",
      "navigate",
      "type",
      "complete",
      "close",
    ]);
    expect(
      run.timelines.filter((timeline) => timeline.inputToPaintMs.status === "measured").length,
    ).toBeGreaterThan(0);
    driver.teardown();

    // The recorded run, but with the lapce-client identity unrecorded in its
    // manifest: provenance alone is not enough, the pin must name the build.
    const unpinned = assertRealLapceProductClaim({ ...run, versions: UNPINED_CLIENT_VERSIONS });
    expect(unpinned.admissible).toBe(false);
    expect(unpinned.reason).toMatch(/pinned lapce-client identity/);

    // The recorded run with no measured input-to-paint anywhere: provenance
    // and pin alone are not enough without a measured headline timeline.
    const noPaint = assertRealLapceProductClaim({ ...run, timelines: [] });
    expect(noPaint.admissible).toBe(false);
    expect(noPaint.reason).toMatch(/none were captured/);
  });

  it("refuses a tampered capture artifact loud (content digest mismatch)", () => {
    const raw = JSON.parse(
      readFileSync(path.join(wsp1lProducts, "real-client-capture.v1.json"), "utf8"),
    );
    const tampered = { ...raw, capturedLines: [...raw.capturedLines, "verter ui-stamp {}"] };
    const tamperedPath = path.join(os.tmpdir(), `wsp1l-tampered-${Date.now()}.json`);
    writeFileSync(tamperedPath, JSON.stringify(tampered));
    try {
      expect(() => loadRecordedLapceCapture(tamperedPath)).toThrow(/content digest mismatch/);
    } finally {
      rmSync(tamperedPath, { force: true });
    }
  });

  it("certifies hermetic runs as non-real-client evidence only (WSP1L-AC-RESOURCE)", () => {
    const verdict = certifyRun(fixtureRun());
    expect(verdict.certified).toBe(true);
    if (verdict.certified) {
      expect(verdict.realClientEvidence).toBe(false);
    }
    expect(carriesRealClientEvidence(realCaptureRun())).toBe(false);
  });

  it("pins the shipped volt/server/provider versions and the recorded lapce-client build", () => {
    const manifest = JSON.parse(
      readFileSync(path.join(wsp1lProducts, "version-manifest.v1.json"), "utf8"),
    );
    expect(manifest.items).toEqual(PINNED_LAPCE_VERSION_MANIFEST.items);
    expect(versionsMatchPinnedManifest(PINNED_LAPCE_VERSION_MANIFEST).ok).toBe(true);
    const lapce = manifest.items.find((item: { item: string }) => item.item === "lapce-client");
    expect(lapce.status).toBe("pinned");
    expect(lapce.version).toMatch(/^0\.4\.6/);
    expect(lapce.reason).toMatch(/WSP1L instrumentation patch/);
  });

  it("exports ./lapce from compiled dist, not TypeScript source", () => {
    const pkg = JSON.parse(
      readFileSync(path.join(repoRoot, "packages/dx-harness/package.json"), "utf8"),
    ) as { exports: { "./lapce": { import: string; types: string; default: string } } };
    expect(pkg.exports["./lapce"].import).toBe("./dist/lapce/index.js");
    expect(pkg.exports["./lapce"].types).toBe("./dist/lapce/index.d.ts");
    expect(pkg.exports["./lapce"].default).toBe("./dist/lapce/index.js");
  });

  it("records the automation path and its perturbation on every run (WSP1L.1)", () => {
    const run = fixtureRun();
    expect(run.automationPath.kind).toBe("instrumented-fixture-host");
    expect(run.automationPath.perturbation).toMatch(/no GUI, no sleeps/);
    expect(run.automationPath.recordedAs).toBe("WSP1L.1 fixture path");
  });
});
