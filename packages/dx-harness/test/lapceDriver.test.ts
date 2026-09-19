/**
 * WSP1L — Real-Lapce UI instrumentation and interaction driver.
 *
 * Hermetic proof of the charter's discriminating boundaries: a stalled UI
 * thread is detected while the server timeline shows immediate completion
 * (AC1); two runs of the same scripted interaction compare within a recorded
 * noise bound (AC2); a protocol-smoke-only run is never certified (AC3). The
 * fixture host is deterministic and is never counted as a real Lapce client.
 */
import { readFileSync } from "node:fs";
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
  assertCertified,
  assertRealLapceProductClaim,
  buildUiTimeline,
  certifyRun,
  compareScriptedTimelines,
  detectUiStallWithImmediateServer,
  versionsMatchPinnedManifest,
  type LapceUiRun,
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
 * detector treats >= 1 ms gaps as blocked), nothing blocked.
 */
function immediateServerTrace(epoch: number, method = "textDocument/completion"): InteractionTrace {
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
    sourceEpoch: epoch,
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

  it("fails loud when a step has no correlatable server trace epoch", () => {
    const driver = makeFixtureDriver([immediateServerTrace(1)]);
    driver.open();
    expect(() => driver.type()).toThrow(/no WSP1 server InteractionTrace/);
  });

  function makeFixtureDriver(traces?: readonly InteractionTrace[]) {
    return new LapceInteractionDriver({
      host: new FixtureLapceHost({ clockMs: sequenceClock(0) }),
      serverTraces: traces ?? [1, 2, 3, 4, 5].map((epoch) => immediateServerTrace(epoch)),
      versions: PINNED_LAPCE_VERSION_MANIFEST,
      stallThreshold: STALL_THRESHOLD,
      immediacyBound: IMMEDIACY_BOUND,
      protocolSmokePassed: true,
      receiptBasis: fixtureRun().receiptBasis,
    });
  }
});

describe("real-client claims, pinned manifest and package surface", () => {
  it("never counts the fixture host as a real-client paint claim", () => {
    const verdict = assertRealLapceProductClaim(fixtureRun());
    expect(verdict.admissible).toBe(false);
    expect(verdict.reason).toMatch(/instrumented fixture host/);
    expect(verdict.reason).toMatch(GUI_INSTRUMENTATION_UNAVAILABLE_REASON);
  });

  it("admits only the real-Lapce host with a measured input-to-paint", () => {
    const real = fixtureRun({ hostKind: LAPCE_REAL_HOST });
    expect(assertRealLapceProductClaim(real).admissible).toBe(true);
    const realNoPaint = fixtureRun({
      hostKind: LAPCE_REAL_HOST,
      timelines: [],
    });
    const noTimeline = assertRealLapceProductClaim(realNoPaint);
    expect(noTimeline.admissible).toBe(false);
    expect(noTimeline.reason).toMatch(/none were captured/);
  });

  it("pins the shipped volt/server/provider versions and records lapce-client unavailable", () => {
    const manifest = JSON.parse(
      readFileSync(path.join(wsp1lProducts, "version-manifest.v1.json"), "utf8"),
    );
    expect(manifest.items).toEqual(PINNED_LAPCE_VERSION_MANIFEST.items);
    expect(versionsMatchPinnedManifest(PINNED_LAPCE_VERSION_MANIFEST).ok).toBe(true);
    const lapce = manifest.items.find((item: { item: string }) => item.item === "lapce-client");
    expect(lapce.version).toBeNull();
    expect(lapce.status).toBe("unrecorded");
    expect(lapce.reason).toMatch(/unavailable, not guessed/);
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
