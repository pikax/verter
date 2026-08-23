#!/usr/bin/env node

import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { PassThrough } from "node:stream";
import { spawn, spawnSync } from "node:child_process";
import {
  buildGateLaneCommandPlan,
  buildCanonicalSurface1FilterExpr,
  buildTrybuildExclusionFilterExpr,
  canonicalGateLaneTranscriptSegments,
  createGateRunSupervisor,
  deriveGateLaneLayout,
  minimizeProvenanceRoots,
  orchestrateGateLanes,
  parsePosixProcessForestRss,
  parseWindowsProcessForestRss,
  pidAlive,
  processForestFromSnapshot,
  provenanceSweep,
  reduceGateLaneReceipts,
  extractNextestTerminalFailures,
} from "./gate-internals.mjs";
import { splitGateLogSurfaces } from "./triage-gate-internals.mjs";

const MiB = 1024 ** 2;

const completeSurfaceReceipt = (overrides = {}) => ({
  laneId: "surface-1",
  hardFailure: false,
  exitCode: null,
  failures: [],
  toleratedOccurred: false,
  coverage: { parseable: true, complete: true },
  output: "surface-output\n",
  ...overrides,
});

const completeShippedReceipt = (overrides = {}) => ({
  laneId: "shipped-cfg",
  hardFailure: false,
  exitCode: null,
  failures: [],
  check: { status: "ok", output: "check-output\n" },
  contract: { status: "ok", parseable: true, complete: true, output: "contract-output\n" },
  parity: { complete: true, matches: true },
  ...overrides,
});

// Architecture-plan test 1: layout roots are exact, pairwise-disjoint, and command construction stays
// byte-for-byte delegated to the existing builders. The front archive/list cardinality is asserted by the
// bounded production-wiring test in gate-selftest.mjs; this plan contains exactly one Surface invocation.
{
  const runnerTarget = join(tmpdir(), "verter-gate-block-b-layout", "target");
  const gateDir = join(runnerTarget, "gate-work");
  const layout = deriveGateLaneLayout(runnerTarget, gateDir);
  assert.equal(layout.surface1.targetDir, resolve(runnerTarget, "lanes", "surface-1", "target"));
  assert.equal(
    layout.shippedCfg.targetDir,
    resolve(runnerTarget, "lanes", "shipped-cfg", "target"),
  );
  const roots = [
    layout.surface1.targetDir,
    layout.surface1.workDir,
    layout.surface1.extractDir,
    layout.surface1.outputFile,
    layout.shippedCfg.targetDir,
    layout.shippedCfg.workDir,
    layout.shippedCfg.outputFile,
  ];
  assert.ok(roots.every(isAbsolute), "every lane root is absolute");
  assert.equal(new Set(roots.map((root) => root.toLowerCase())).size, roots.length);
  for (let i = 0; i < roots.length; i++) {
    for (let j = i + 1; j < roots.length; j++) {
      const left = relative(roots[i], roots[j]);
      const right = relative(roots[j], roots[i]);
      assert.equal(
        (left !== "" && left !== ".." && !left.startsWith(`..${sep}`) && !isAbsolute(left)) ||
          (right !== "" && right !== ".." && !right.startsWith(`..${sep}`) && !isAbsolute(right)),
        false,
        `mutable roots must not contain each other: ${roots[i]} / ${roots[j]}`,
      );
    }
  }
  assert.throws(
    () => deriveGateLaneLayout(runnerTarget, join(tmpdir(), "outside-runner")),
    /contained/i,
  );

  // `expectedSurface1Filter` is built independently of `buildCanonicalSurface1FilterExpr()` — it composes
  // the same `and not package(verter_shipped_cfg_contract)` wrapper as a literal here, over the (separately
  // pinned, in gate-selftest.mjs GB13.2) trybuild-exclusion arms. Asserting against
  // `buildCanonicalSurface1FilterExpr()`'s own return value is a self-referential tautology that cannot
  // catch a regression inside that function itself (e.g. a dropped `verter_shipped_cfg_contract` exclusion,
  // which would let that package's tests run twice — once under Surface 1's dev profile, once under the
  // shipped-cfg lane's `no-debug-assertions` profile).
  const expectedSurface1Filter = `(${buildTrybuildExclusionFilterExpr()}) and not package(verter_shipped_cfg_contract)`;
  assert.equal(
    buildCanonicalSurface1FilterExpr(),
    expectedSurface1Filter,
    "buildCanonicalSurface1FilterExpr must wrap the trybuild exclusion with the shipped-cfg-contract exclusion",
  );

  const commandPlan = buildGateLaneCommandPlan({
    archiveFile: join(gateDir, "nextest.tar.zst"),
    surfaceExtractDir: layout.surface1.extractDir,
    repoRealpath: join(tmpdir(), "verter-repo"),
    filterExpr: expectedSurface1Filter,
    exhaustive: true,
    testThreads: 7,
    shippedCheckTimingsEnabled: true,
    shippedContractTimingsEnabled: true,
  });
  assert.deepEqual(commandPlan.surface1.args, [
    "nextest",
    "run",
    "--archive-file",
    join(gateDir, "nextest.tar.zst"),
    "--extract-to",
    layout.surface1.extractDir,
    "--extract-overwrite",
    "--workspace-remap",
    join(tmpdir(), "verter-repo"),
    "-E",
    expectedSurface1Filter,
    "--no-fail-fast",
    "--test-threads",
    "7",
  ]);
  assert.deepEqual(commandPlan.shippedCfg.checkArgs, [
    "check",
    "--workspace",
    "--all-targets",
    "--profile",
    "no-debug-assertions",
    "--timings",
  ]);
  assert.deepEqual(commandPlan.shippedCfg.contractArgs, [
    "nextest",
    "run",
    "-p",
    "verter_shipped_cfg_contract",
    "--cargo-profile",
    "no-debug-assertions",
    "--timings",
    "--no-fail-fast",
    "--test-threads",
    "7",
  ]);
}

// `shippedTestThreads`, when given, sizes ONLY the shipped-cfg lane's `--test-threads` — proving the two
// concurrently-running lanes (Surface 1 and shipped-cfg) are sized INDEPENDENTLY, the fix for the lane
// resource partition (a shared `testThreads` value applied to both concurrent lanes independently
// requests 2x that value from the host for the whole overlap window). A discriminating check: the two
// `--test-threads` values in the plan differ, and each traces to its own input.
{
  const commandPlan = buildGateLaneCommandPlan({
    archiveFile: join(tmpdir(), "verter-gate-split", "nextest.tar.zst"),
    surfaceExtractDir: join(tmpdir(), "verter-gate-split", "extract"),
    repoRealpath: join(tmpdir(), "verter-repo"),
    filterExpr: "test(x)",
    exhaustive: false,
    testThreads: 6,
    shippedTestThreads: 2,
  });
  assert.ok(commandPlan.surface1.args.includes("--test-threads"));
  assert.equal(
    commandPlan.surface1.args[commandPlan.surface1.args.indexOf("--test-threads") + 1],
    "6",
  );
  assert.equal(
    commandPlan.shippedCfg.contractArgs[
      commandPlan.shippedCfg.contractArgs.indexOf("--test-threads") + 1
    ],
    "2",
  );
}

// Architecture-plan test 10: both lanes are admitted together; only a local Surface hard-failure cancels
// shipped-cfg, and fixed receipt slots make reduction independent of promise completion order.
{
  const starts = [];
  const cancels = [];
  let releaseShipped;
  const shippedWait = new Promise((resolve) => {
    releaseShipped = resolve;
  });
  const local = await orchestrateGateLanes({
    exhaustive: false,
    runSurfaceLane: async () => {
      starts.push("surface-1");
      return completeSurfaceReceipt({
        hardFailure: true,
        failures: [{ surface: "nextest", name: "surface failure" }],
      });
    },
    runShippedLane: async () => {
      starts.push("shipped-cfg");
      return shippedWait;
    },
    cancelLane: async (laneId, reason) => {
      cancels.push([laneId, reason]);
      releaseShipped(
        completeShippedReceipt({
          hardFailure: true,
          check: { status: "cancelled", output: "" },
          contract: { status: "not-run", parseable: false, complete: false, output: "" },
          parity: { complete: false, matches: false },
        }),
      );
    },
  });
  assert.deepEqual(starts, ["surface-1", "shipped-cfg"]);
  assert.deepEqual(cancels, [["shipped-cfg", "SURFACE_1_FAIL_FAST"]]);
  assert.equal(local.shipped.check.status, "cancelled");
  assert.equal(reduceGateLaneReceipts(local).coverageDisposition, "cancelled-by-local-fail-fast");

  const reverseEvents = [];
  let releaseSurface;
  const surfaceWait = new Promise((resolve) => {
    releaseSurface = resolve;
  });
  const exhaustiveRun = orchestrateGateLanes({
    exhaustive: true,
    runSurfaceLane: async () => {
      reverseEvents.push("surface-start");
      return surfaceWait;
    },
    runShippedLane: async () => {
      reverseEvents.push("shipped-start");
      reverseEvents.push("shipped-finish");
      queueMicrotask(() => {
        reverseEvents.push("surface-finish");
        releaseSurface(
          completeSurfaceReceipt({
            hardFailure: true,
            failures: [{ surface: "nextest", name: "surface failure" }],
          }),
        );
      });
      return completeShippedReceipt({
        hardFailure: true,
        failures: [{ surface: "nextest", name: "shipped failure" }],
      });
    },
    cancelLane: async (...args) => cancels.push(args),
  });
  const exhaustiveReceipts = await exhaustiveRun;
  assert.deepEqual(reverseEvents, [
    "surface-start",
    "shipped-start",
    "shipped-finish",
    "surface-finish",
  ]);
  assert.equal(cancels.length, 1, "exhaustive Surface failure never adds a lane cancellation");
  const exhaustiveDecision = reduceGateLaneReceipts(exhaustiveReceipts);
  assert.deepEqual(
    exhaustiveDecision.failures.map((failure) => failure.name),
    ["surface failure", "shipped failure"],
  );

  let shippedFirstCancelCount = 0;
  const shippedFirst = await orchestrateGateLanes({
    exhaustive: false,
    runSurfaceLane: async () => completeSurfaceReceipt(),
    runShippedLane: async () =>
      completeShippedReceipt({
        hardFailure: true,
        failures: [{ surface: "check", name: "shipped-first failure" }],
      }),
    cancelLane: async () => {
      shippedFirstCancelCount++;
    },
  });
  assert.equal(shippedFirstCancelCount, 0, "shipped-first failure never cancels Surface");
  assert.equal(reduceGateLaneReceipts(shippedFirst).verdict, "FAIL");

  const infrastructureCancels = [];
  await orchestrateGateLanes({
    exhaustive: true,
    runSurfaceLane: async () => completeSurfaceReceipt({ exitCode: 127 }),
    runShippedLane: async () => completeShippedReceipt(),
    cancelLane: async (...args) => infrastructureCancels.push(args),
  });
  await orchestrateGateLanes({
    exhaustive: true,
    runSurfaceLane: async () => completeSurfaceReceipt(),
    runShippedLane: async () => completeShippedReceipt({ exitCode: 127 }),
    cancelLane: async (...args) => infrastructureCancels.push(args),
  });
  assert.deepEqual(infrastructureCancels, [
    ["shipped-cfg", "SURFACE_1_INFRASTRUCTURE"],
    ["surface-1", "SHIPPED_CFG_INFRASTRUCTURE"],
  ]);
}

// `concurrent: false` — the one-core-ceiling case (`deriveGateLaneResourceSplit` reports `concurrent:
// false` when either axis's total is < 2, because a lane cannot run cargo/nextest with 0 build jobs or 0
// test threads, so the numeric shares alone cannot bound the combined demand). The genuinely discriminating
// property: `runShippedLane` must not even be INVOKED until `runSurfaceLane` has settled — proving the two
// lanes' cargo/nextest invocations never overlap in wall-clock, not merely that they are labeled serial.
{
  const starts = [];
  let releaseSurface;
  const surfaceWait = new Promise((resolve) => {
    releaseSurface = resolve;
  });
  const serialRun = orchestrateGateLanes({
    exhaustive: false,
    concurrent: false,
    runSurfaceLane: async () => {
      starts.push("surface-1");
      return surfaceWait;
    },
    runShippedLane: async () => {
      starts.push("shipped-cfg");
      return completeShippedReceipt();
    },
    cancelLane: async () => {
      throw new Error("cancelLane must not be called on a clean serial run");
    },
  });
  // Yield a few microtasks: if the implementation regressed to admitting both lanes together, shipped-cfg
  // would already have started here even though surface has not resolved yet.
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(starts, ["surface-1"], "shipped-cfg must not start before surface-1 settles");
  releaseSurface(completeSurfaceReceipt());
  const serial = await serialRun;
  assert.deepEqual(starts, ["surface-1", "shipped-cfg"]);
  assert.equal(serial.shipped.check.status, "ok");

  // A local Surface hard-failure must cancel shipped-cfg WITHOUT ever invoking `runShippedLane` at all —
  // the serial equivalent of the concurrent path's mid-flight cancellation, but stronger: there is no
  // process to cancel because none was ever started.
  const fastFailStarts = [];
  const fastFailCancels = [];
  const fastFail = await orchestrateGateLanes({
    exhaustive: false,
    concurrent: false,
    runSurfaceLane: async () => {
      fastFailStarts.push("surface-1");
      return completeSurfaceReceipt({
        hardFailure: true,
        failures: [{ surface: "nextest", name: "serial surface failure" }],
      });
    },
    runShippedLane: async () => {
      fastFailStarts.push("shipped-cfg");
      return completeShippedReceipt();
    },
    cancelLane: async (laneId, reason) => {
      fastFailCancels.push([laneId, reason]);
    },
  });
  assert.deepEqual(fastFailStarts, ["surface-1"], "shipped-cfg must never start after a fail-fast hard failure");
  assert.deepEqual(fastFailCancels, [["shipped-cfg", "SURFACE_1_FAIL_FAST"]]);
  assert.equal(fastFail.shipped, null);
  assert.equal(reduceGateLaneReceipts(fastFail).verdict, "FAIL");
}

// Architecture-plan test 11: complete coverage is a separate green fence. Missing, cancelled, unrun,
// unparsable, or parity-incomplete receipts cannot PASS; a fully measured red exhaustive run stays red.
{
  const green = reduceGateLaneReceipts({
    surface: completeSurfaceReceipt(),
    shipped: completeShippedReceipt(),
  });
  assert.equal(green.verdict, "PASS");
  assert.equal(green.coverageComplete, true);

  const tolerated = reduceGateLaneReceipts({
    surface: completeSurfaceReceipt({ toleratedOccurred: true }),
    shipped: completeShippedReceipt(),
  });
  assert.equal(tolerated.verdict, "PASS-WITH-TOLERATED");

  const incompleteCases = [
    { surface: null, shipped: completeShippedReceipt() },
    {
      surface: completeSurfaceReceipt({ coverage: { parseable: false, complete: false } }),
      shipped: completeShippedReceipt(),
    },
    {
      surface: completeSurfaceReceipt(),
      shipped: completeShippedReceipt({ check: { status: "cancelled", output: "" } }),
    },
    {
      surface: completeSurfaceReceipt(),
      shipped: completeShippedReceipt({
        contract: { status: "not-run", parseable: false, complete: false, output: "" },
      }),
    },
    {
      surface: completeSurfaceReceipt(),
      shipped: completeShippedReceipt({ parity: { complete: false, matches: false } }),
    },
  ];
  for (const receipts of incompleteCases) {
    const decision = reduceGateLaneReceipts(receipts);
    assert.equal(decision.coverageComplete, false);
    assert.equal(decision.verdict, "FAIL");
    assert.ok(decision.failures.some((failure) => failure.surface === "gate/incomplete"));
  }

  const measuredRed = reduceGateLaneReceipts({
    surface: completeSurfaceReceipt({
      hardFailure: true,
      failures: [{ surface: "nextest", name: "known Windows product failure" }],
    }),
    shipped: completeShippedReceipt(),
  });
  assert.equal(measuredRed.coverageComplete, true);
  assert.equal(measuredRed.coverageDisposition, "complete");
  assert.equal(measuredRed.verdict, "FAIL");

  const setup = reduceGateLaneReceipts({
    surface: completeSurfaceReceipt({ exitCode: 127 }),
    shipped: completeShippedReceipt(),
  });
  assert.equal(setup.verdict, null);
  assert.equal(setup.exitCode, 127);

  const aggregateAbort = reduceGateLaneReceipts({
    surface: completeSurfaceReceipt({ exitCode: 123 }),
    shipped: completeShippedReceipt({ exitCode: 123 }),
  });
  assert.equal(aggregateAbort.verdict, null);
  assert.equal(aggregateAbort.exitCode, 123);
  assert.equal(aggregateAbort.coverageDisposition, "aborted");
}

// Architecture-plan test 13: output completion order cannot affect the canonical replay order, and each
// parseable lane body occurs exactly once under its established header.
{
  const segments = canonicalGateLaneTranscriptSegments({
    surface: completeSurfaceReceipt({ output: "surface-unique\n" }),
    shipped: completeShippedReceipt({
      check: { status: "ok", output: "check-unique\n" },
      contract: {
        status: "ok",
        parseable: true,
        complete: true,
        output: "contract-unique\n",
      },
    }),
  });
  assert.deepEqual(
    segments.map((segment) => segment.phaseId),
    ["surface-1", "shipped-check", "shipped-contract"],
  );
  const transcript = segments.map(({ header, output }) => `${header}\n${output}`).join("");
  assert.ok(transcript.indexOf("surface-unique") < transcript.indexOf("check-unique"));
  assert.ok(transcript.indexOf("check-unique") < transcript.indexOf("contract-unique"));
  for (const marker of ["surface-unique", "check-unique", "contract-unique"]) {
    assert.equal(transcript.split(marker).length - 1, 1);
  }

  const parseableSegments = canonicalGateLaneTranscriptSegments({
    surface: completeSurfaceReceipt({
      output:
        "        FAIL [   0.010s] surface_bin cases::surface::x\n" +
        "     Summary [   1.000s] 1 tests run: 0 passed, 1 failed, 0 skipped\n",
    }),
    shipped: completeShippedReceipt({
      check: { status: "ok", output: "check output cannot enter a nextest segment\n" },
      contract: {
        status: "ok",
        parseable: true,
        complete: true,
        output:
          "        FAIL [   0.010s] shipped_bin cases::shipped::x\n" +
          "     Summary [   1.000s] 1 tests run: 0 passed, 1 failed, 0 skipped\n",
      },
    }),
  });
  const parseableTranscript =
    parseableSegments.map(({ header, output }) => `[gate] ${header}\n${output}`).join("") +
    "[gate][error] VERDICT: FAIL — 2 non-tolerated failure(s):\n";
  const parsed = splitGateLogSurfaces(parseableTranscript);
  assert.equal(extractNextestTerminalFailures(parsed.surface1).failures[0].binaryId, "surface_bin");
  assert.equal(
    extractNextestTerminalFailures(parsed.shippedCfg).failures[0].binaryId,
    "shipped_bin",
  );
}

class FakeChild extends EventEmitter {
  constructor(pid) {
    super();
    this.pid = pid;
    this.stdout = new PassThrough();
    this.stderr = new PassThrough();
    this.exitCode = null;
    this.signalCode = null;
    this.closed = false;
  }

  finish(code = 0, signal = null) {
    if (this.closed) return;
    this.closed = true;
    this.exitCode = code;
    this.signalCode = signal;
    this.stdout.end();
    this.stderr.end();
    queueMicrotask(() => this.emit("close", code, signal));
  }
}

// Block A fix: the reap selector refuses a replacement root and keeps exact detached seeds.
{
  const registration = {
    pid: 100,
    rootIdentity: "original-root",
    childClosed: true,
    ownedIdentities: [{ pid: 101, identity: "owned-child" }],
  };
  const replacement = processForestFromSnapshot(
    {
      ok: true,
      rows: new Map([
        [100, { pid: 100, parentPid: 1, identity: "replacement-root" }],
        [101, { pid: 101, parentPid: 1, identity: "owned-child" }],
        [102, { pid: 102, parentPid: 101, identity: "owned-grandchild" }],
      ]),
    },
    registration,
  );
  assert.equal(replacement.rootIdentityMismatch, true);
  assert.deepEqual(
    replacement.rows.map(({ pid }) => pid).sort((a, b) => a - b),
    [101, 102],
  );
  assert.equal(
    replacement.rows.some(({ pid }) => pid === 100),
    false,
    "a reused root PID is never adopted by the reaper",
  );
}

function scriptedHarness(overrides = {}) {
  let tick = null;
  let nextPid = 41000;
  const children = [];
  const events = [];
  const spawnFn =
    overrides.spawnFn ||
    (() => {
      const child = new FakeChild(nextPid++);
      children.push(child);
      events.push(`spawn:${child.pid}`);
      return child;
    });
  const reapRegisteredForestFn =
    overrides.reapRegisteredForestFn ||
    (async (registration, reason) => {
      events.push(`reap:${registration.tokenId}:${reason}`);
      registration.child.finish(null, "SIGKILL");
      return { reaped: true, confirmedDead: true, wasLive: true };
    });
  const supervisor = createGateRunSupervisor({
    deadlineMs: 10_000,
    stallMs: 1_000,
    memoryLimitBytes: 1_000 * MiB,
    memoryPollMs: 10,
    memorySampleFailureLimit: 3,
    now: () => 0,
    setIntervalFn(callback) {
      tick = callback;
      return Symbol("timer");
    },
    clearIntervalFn() {},
    spawnFn,
    processIdentityFn: (pid) => `fake-start-${pid}`,
    processAliveFn: (pid) => children.some((child) => child.pid === pid && !child.closed),
    reapRegisteredForestFn,
    sampleProcessForestRssFn: () => ({
      ok: true,
      rssBytes: 0,
      processCount: 0,
      perRoot: [],
      perLane: {},
    }),
    ...overrides,
  });
  return {
    supervisor,
    children,
    events,
    tick: async () => {
      assert.ok(tick, "the supervisor owns one watchdog timer");
      await tick();
    },
  };
}

const step = (name) => ({
  name,
  command: "fake-command",
  args: [name],
  cwd: process.cwd(),
  env: process.env,
  mirrorOutput: false,
});

// Block A fix: closed roots retain exact observed descendants, while a reused root PID is refused.
{
  for (const parse of [parsePosixProcessForestRss, parseWindowsProcessForestRss]) {
    const separator = parse === parsePosixProcessForestRss ? " " : "\t";
    const closed = parse(
      [
        [101, 1, 20, "child-start"].join(separator),
        [102, 101, 30, "grandchild-start"].join(separator),
      ].join("\n"),
      [
        {
          tokenId: 1,
          laneId: "surface-1",
          pid: 100,
          identity: "root-start",
          closed: true,
          ownedIdentities: [{ pid: 101, identity: "child-start" }],
        },
      ],
    );
    assert.equal(closed.ok, true);
    assert.equal(closed.processCount, 2);
    assert.equal(closed.rssBytes, (20 + 30) * (parse === parsePosixProcessForestRss ? 1024 : 1));
    assert.deepEqual(
      closed.perRoot[0].identities.map(({ pid }) => pid),
      [101, 102],
    );

    const reused = parse([[100, 1, 10, "replacement-start"].join(separator)].join("\n"), [
      { tokenId: 2, laneId: "shipped-cfg", pid: 100, identity: "original-start" },
    ]);
    assert.equal(reused.ok, false);
    assert.match(reused.error, /identity/i);
  }
}

// Block A fix: admission binds the root token to the spawn-time process identity.
{
  const h = scriptedHarness();
  const run = h.supervisor.runStep("surface-1", step("identity-bound"));
  assert.equal(h.supervisor.snapshotTelemetry().active[0].rootIdentity, "fake-start-41000");
  h.children[0].finish(0);
  assert.equal((await run).code, 0);
  await h.supervisor.closeAndReapAll("TEST_DONE");
}

// Block A fix: an unavailable admission identity is an immediate infrastructure refusal and fence.
{
  let reapCalls = 0;
  const h = scriptedHarness({
    processIdentityFn: () => "",
    reapRegisteredForestFn: async () => {
      reapCalls += 1;
      return { reaped: true, confirmedDead: true, wasLive: true };
    },
  });
  const run = h.supervisor.runStep("surface-1", step("identity-unavailable"));
  const denied = await h.supervisor.runStep("shipped-cfg", step("fenced-after-identity-failure"));
  assert.equal(denied.reason, "CANCELLED");
  assert.equal(h.children.length, 1);
  h.children[0].finish(0);
  const result = await run;
  assert.equal(result.reason, "MEMORY_MONITOR");
  assert.match(result.memorySampleFailureDetail, /no checkable process identity/i);
  assert.equal(result.reapConfirmedDead, false);
  assert.equal(reapCalls, 0, "an uncheckable live root is never signalled by numeric PID");
  await h.supervisor.closeAndReapAll("TEST_DONE");
}

// Block A fix: a root already terminal before identity publication is never signalled by numeric PID.
{
  let reapCalls = 0;
  let sweepCalls = 0;
  const h = scriptedHarness({
    processIdentityFn: () => "",
    processAliveFn: () => false,
    reapRegisteredForestFn: async () => {
      reapCalls += 1;
      return { reaped: true, confirmedDead: true, wasLive: true };
    },
    provenanceSweepFn: async () => {
      sweepCalls += 1;
    },
  });
  const run = h.supervisor.runStep("surface-1", {
    ...step("terminal-before-identity"),
    targetDir: "must-not-sweep",
  });
  h.children[0].finish(0);
  assert.equal((await run).reason, "");
  assert.equal((await h.supervisor.closeAndReapAll("TEST_DONE")).confirmedDead, true);
  assert.equal(reapCalls, 0);
  assert.equal(sweepCalls, 0);
}

// Block A fix2: exitCode/signalCode are terminal before stdio close and must never publish a replacement.
for (const terminal of [
  { name: "exit-before-close", exitCode: 0, signalCode: null, closeCode: 0, closeSignal: null },
  {
    name: "signal-before-close",
    exitCode: null,
    signalCode: "SIGTERM",
    closeCode: null,
    closeSignal: "SIGTERM",
  },
]) {
  let reapCalls = 0;
  let sweepCalls = 0;
  const h = scriptedHarness({
    processIdentityFn: null,
    processAliveFn: () => true,
    sampleProcessForestRssFn: (roots) => ({
      ok: true,
      rssBytes: 1,
      processCount: 1,
      perRoot: roots.map((root) => ({
        tokenId: root.tokenId,
        laneId: root.laneId,
        rssBytes: 1,
        processCount: 1,
        identities: [{ pid: root.pid, identity: "replacement-start" }],
      })),
      perLane: { "surface-1": { rssBytes: 1, processCount: 1 } },
    }),
    reapRegisteredForestFn: async () => {
      reapCalls += 1;
      return { reaped: true, confirmedDead: true, wasLive: true };
    },
    provenanceSweepFn: async () => {
      sweepCalls += 1;
      return { matched: 1, signalled: 1, identityMismatches: 0 };
    },
  });
  const run = h.supervisor.runStep("surface-1", {
    ...step(terminal.name),
    targetDir: "must-not-sweep-replacement",
  });
  h.children[0].exitCode = terminal.exitCode;
  h.children[0].signalCode = terminal.signalCode;
  await h.tick();
  const publishedIdentity = h.supervisor.snapshotTelemetry().active[0]?.rootIdentity || "";
  h.children[0].finish(terminal.closeCode, terminal.closeSignal);
  const receipt = await run;
  await h.supervisor.closeAndReapAll("TEST_DONE");

  assert.equal(publishedIdentity, "", `${terminal.name} must not publish a replacement identity`);
  assert.equal(reapCalls, 0, `${terminal.name} must not authorize numeric reaping`);
  assert.equal(sweepCalls, 0, `${terminal.name} must not authorize provenance signalling`);
  assert.equal(
    receipt.code,
    terminal.closeSignal ? 128 : 0,
    `${terminal.name} preserves its receipt`,
  );
  assert.equal(receipt.signalName, terminal.closeSignal || "");
}

// Block A fix: provenance never signals a PID whose exact identity changed.
{
  const targetDir =
    process.platform === "win32" ? "C:\\synthetic\\gate-target" : "/synthetic/gate-target";
  const command = `cargo test --target-dir ${targetDir}`;
  const signals = [];
  let currentIdentity = "replacement-start";
  const options = {
    listProcessesFn: () => [{ pid: 55123, cmd: command, identity: "original-start" }],
    processIdentityFn: () => currentIdentity,
    signalProcessFn: (pid, signal) => signals.push([pid, signal]),
    delayFn: async () => {},
  };
  const refused = await provenanceSweep(targetDir, 1, options);
  assert.equal(refused.identityMismatches > 0, true);
  assert.deepEqual(signals, []);

  currentIdentity = "original-start";
  const control = await provenanceSweep(targetDir, 1, options);
  assert.equal(control.identityMismatches, 0);
  assert.deepEqual(signals, [
    [55123, "SIGTERM"],
    [55123, "SIGKILL"],
  ]);
}

// Block B fix: historical nested registrations collapse under retained ownership umbrellas, while a
// path-segment sibling remains an independent provenance sweep authority.
{
  const posixRoots = [
    "/synthetic/gate-runner",
    "/synthetic/gate-runner/lanes/surface-1",
    "/synthetic/gate-runner/lanes/surface-1/target",
    "/synthetic/gate-runner/",
    "/synthetic/gate-runner2",
    "/synthetic/gate-runner2/lanes/shipped-cfg",
  ];
  assert.deepEqual(minimizeProvenanceRoots(posixRoots, { windows: false }), [
    "/synthetic/gate-runner",
    "/synthetic/gate-runner2",
  ]);
  const windowsRoots = [
    "C:\\synthetic\\gate-runner",
    "C:\\synthetic\\gate-runner\\lanes\\surface-1",
    "c:\\SYNTHETIC\\GATE-RUNNER\\lanes\\surface-1\\target",
    "C:\\synthetic\\gate-runner\\",
    "C:\\synthetic\\gate-runner2",
    "c:\\synthetic\\GATE-RUNNER2\\lanes\\shipped-cfg",
  ];
  assert.deepEqual(minimizeProvenanceRoots(windowsRoots, { windows: true }), [
    "C:\\synthetic\\gate-runner",
    "C:\\synthetic\\gate-runner2",
  ]);
  assert.throws(
    () => minimizeProvenanceRoots(["/synthetic/gate-runner", ""], { windows: false }),
    /non-empty path/,
  );

  const umbrella = resolve(tmpdir(), "verter-gate-provenance-minimizer", "gate-runner");
  const sibling = resolve(tmpdir(), "verter-gate-provenance-minimizer", "gate-runner2");
  const swept = [];
  const h = scriptedHarness({
    ownershipRoots: [umbrella, join(umbrella, "lanes", "surface-1"), umbrella],
    provenanceSweepFn: async (targetDir) => {
      swept.push(targetDir);
      return { matched: 0, signalled: 0, identityMismatches: 0 };
    },
  });
  for (const targetDir of [
    join(umbrella, "lanes", "surface-1", "target"),
    join(umbrella, "lanes", "surface-1", "target", "nested"),
    umbrella,
    sibling,
    join(sibling, "lanes", "shipped-cfg", "target"),
    sibling,
  ]) {
    const run = h.supervisor.runStep("surface-1", { ...step(targetDir), targetDir });
    h.children.at(-1).finish(0);
    assert.equal((await run).code, 0);
  }
  const closeStartedMs = Date.now();
  await h.supervisor.closeAndReapAll("TEST_DONE");
  const closeDurationMs = Date.now() - closeStartedMs;
  assert.deepEqual(swept.sort(), [sibling, umbrella].sort());
  assert.equal(swept.length, 2, "each retained umbrella/sibling root is swept exactly once");
  assert.equal(
    swept.includes(resolve(tmpdir(), "verter-gate-provenance-minimizer")),
    false,
    "minimization must never synthesize a broader unrelated root",
  );
  assert.ok(closeDurationMs < 250, `clean deduplicated close took ${closeDurationMs}ms`);
  process.stderr.write(`gate lane clean deduplicated close: ${closeDurationMs}ms\n`);
}

// Architecture-plan test 2: one process-table fixture, multiple disjoint roots, exact aggregation.
{
  const roots = [
    { tokenId: 1, laneId: "surface-1", pid: 100 },
    { tokenId: 2, laneId: "shipped-cfg", pid: 200 },
    { tokenId: 3, laneId: "closed", pid: 999, closed: true },
  ];
  const posix = parsePosixProcessForestRss(
    ["100 1 10", "101 100 20", "102 101 30", "200 1 40", "201 200 50", "777 1 900"].join("\n"),
    roots,
  );
  assert.equal(posix.ok, true);
  assert.equal(posix.rssBytes, 150 * 1024);
  assert.equal(posix.processCount, 5);
  assert.deepEqual(
    Object.fromEntries(posix.perRoot.map((row) => [row.tokenId, [row.rssBytes, row.processCount]])),
    { 1: [60 * 1024, 3], 2: [90 * 1024, 2] },
  );
  assert.deepEqual(posix.perLane, {
    "surface-1": { rssBytes: 60 * 1024, processCount: 3 },
    "shipped-cfg": { rssBytes: 90 * 1024, processCount: 2 },
  });

  const windows = parseWindowsProcessForestRss(
    [
      "100\t1\t10240",
      "101\t100\t20480",
      "102\t101\t30720",
      "200\t1\t40960",
      "201\t200\t51200",
      "777\t1\t999999",
    ].join("\n"),
    roots,
  );
  assert.equal(windows.ok, true);
  assert.equal(windows.rssBytes, 153600);
  assert.equal(windows.processCount, 5);
  assert.deepEqual(windows.perLane, posix.perLane);

  for (const parse of [parsePosixProcessForestRss, parseWindowsProcessForestRss]) {
    const fixture =
      parse === parsePosixProcessForestRss ? "100 1 1\n101 100 1" : "100\t1\t1\n101\t100\t1";
    const overlap = parse(fixture, [
      { tokenId: 1, laneId: "parent", pid: 100 },
      { tokenId: 2, laneId: "child", pid: 101 },
    ]);
    assert.equal(overlap.ok, false);
    assert.match(overlap.error, /overlap/i);
    const missingLive = parse(fixture, [{ tokenId: 9, laneId: "missing", pid: 999 }]);
    assert.equal(missingLive.ok, false);
    assert.match(missingLive.error, /missing/i);
    const missingClosed = parse(fixture, [
      { tokenId: 9, laneId: "closed", pid: 999, closed: true },
    ]);
    assert.equal(missingClosed.ok, true);
    assert.equal(missingClosed.processCount, 0);
  }
}

// Architecture-plan tests 3 and 4: aggregate ceiling and same-snapshot peak semantics.
{
  const h = scriptedHarness({
    memoryLimitBytes: 100 * MiB,
    sampleProcessForestRssFn(roots) {
      const values = [60, 60];
      const perRoot = roots.map((root, index) => ({
        ...root,
        rssBytes: values[index] * MiB,
        processCount: index + 1,
      }));
      return {
        ok: true,
        rssBytes: perRoot.reduce((sum, row) => sum + row.rssBytes, 0),
        processCount: perRoot.reduce((sum, row) => sum + row.processCount, 0),
        perRoot,
        perLane: Object.fromEntries(
          perRoot.map((row) => [
            row.laneId,
            { rssBytes: row.rssBytes, processCount: row.processCount },
          ]),
        ),
      };
    },
  });
  const p1 = h.supervisor.runStep("surface-1", step("surface"));
  const p2 = h.supervisor.runStep("shipped-cfg", step("shipped"));
  await h.tick();
  const [r1, r2] = await Promise.all([p1, p2]);
  assert.equal(r1.reason, "MEMORY");
  assert.equal(r2.reason, "MEMORY");
  assert.equal(r1.reapConfirmedDead, true);
  assert.equal(r2.reapConfirmedDead, true);
  assert.equal(h.events.filter((event) => event.startsWith("reap:")).length, 2);
  const receipt = h.supervisor.snapshotTelemetry();
  assert.equal(receipt.aggregatePeakRssBytes, 120 * MiB);
  assert.equal(receipt.aggregatePeakProcessCount, 3);
  assert.equal(receipt.perLane["surface-1"].peakRssBytes, 60 * MiB);
  assert.equal(receipt.perLane["shipped-cfg"].peakRssBytes, 60 * MiB);

  let sampleIndex = 0;
  const sequences = [
    [70, 10],
    [10, 70],
  ];
  const peakHarness = scriptedHarness({
    memoryPollMs: 0,
    sampleProcessForestRssFn(roots) {
      const values = sequences[Math.min(sampleIndex++, sequences.length - 1)];
      const perRoot = roots.map((root, index) => ({
        ...root,
        rssBytes: values[index] * MiB,
        processCount: index + 1,
      }));
      return {
        ok: true,
        rssBytes: 80 * MiB,
        processCount: 3,
        perRoot,
        perLane: Object.fromEntries(
          perRoot.map((row) => [
            row.laneId,
            { rssBytes: row.rssBytes, processCount: row.processCount },
          ]),
        ),
      };
    },
  });
  const q1 = peakHarness.supervisor.runStep("surface-1", step("surface-peak"));
  const q2 = peakHarness.supervisor.runStep("shipped-cfg", step("shipped-peak"));
  await peakHarness.tick();
  await peakHarness.tick();
  const peakReceipt = peakHarness.supervisor.snapshotTelemetry();
  assert.equal(peakReceipt.aggregatePeakRssBytes, 80 * MiB);
  assert.equal(peakReceipt.aggregatePeakProcessCount, 3);
  assert.equal(peakReceipt.perLane["surface-1"].peakRssBytes, 70 * MiB);
  assert.equal(peakReceipt.perLane["shipped-cfg"].peakRssBytes, 70 * MiB);
  peakHarness.children.forEach((child) => child.finish(0));
  await Promise.all([q1, q2]);
  await peakHarness.supervisor.closeAndReapAll("TEST_DONE");
}

// Architecture-plan test 5: the deadline is absolute for the gate, never rebased at later admission.
{
  let now = 0;
  const h = scriptedHarness({ deadlineMs: 100, now: () => now });
  const first = h.supervisor.runStep("surface-1", step("early"));
  now = 70;
  const later = h.supervisor.runStep("shipped-cfg", step("late"));
  now = 99;
  await h.tick();
  assert.equal(
    h.children.some((child) => child.closed),
    false,
  );
  now = 100;
  await h.tick();
  const [r1, r2] = await Promise.all([first, later]);
  assert.equal(r1.reason, "TIMEOUT");
  assert.equal(r2.reason, "TIMEOUT");
}

// Architecture-plan test 6: stall is one aggregate progress vector over only live registrations.
{
  let now = 0;
  const h = scriptedHarness({ stallMs: 100, now: () => now });
  const stuck = h.supervisor.runStep("surface-1", step("stuck"));
  const progressing = h.supervisor.runStep("shipped-cfg", step("progressing"));
  await h.tick();
  now = 80;
  h.children[1].stdout.write("progress");
  await h.tick();
  now = 179;
  await h.tick();
  assert.equal(
    h.children.some((child) => child.closed),
    false,
  );
  now = 180;
  await h.tick();
  const [r1, r2] = await Promise.all([stuck, progressing]);
  assert.equal(r1.reason, "STALL");
  assert.equal(r2.reason, "STALL");

  now = 0;
  const completed = scriptedHarness({ stallMs: 100, now: () => now });
  const live = completed.supervisor.runStep("surface-1", step("still-stuck"));
  const noisy = completed.supervisor.runStep("shipped-cfg", step("finishes"));
  await completed.tick();
  now = 50;
  completed.children[1].stdout.write("last-noise");
  completed.children[1].finish(0);
  await noisy;
  await completed.tick();
  now = 149;
  await completed.tick();
  assert.equal(completed.children[0].closed, false);
  now = 150;
  await completed.tick();
  assert.equal((await live).reason, "STALL");
}

// Architecture-plan test 7: close fences admission before any awaited snapshot/reap work.
{
  let releaseClose;
  let reachedFence;
  const fenceReached = new Promise((resolve) => {
    reachedFence = resolve;
  });
  const fenceRelease = new Promise((resolve) => {
    releaseClose = resolve;
  });
  const events = [];
  const h = scriptedHarness({
    beforeCloseSnapshot: async () => {
      events.push("fence");
      reachedFence();
      await fenceRelease;
      events.push("released");
    },
    reapRegisteredForestFn: async (registration, reason) => {
      events.push(`reap:${registration.tokenId}:${reason}`);
      registration.child.finish(null, "SIGKILL");
      return { reaped: true, confirmedDead: true, wasLive: true };
    },
  });
  const live = h.supervisor.runStep("surface-1", step("before-close"));
  const live2 = h.supervisor.runStep("shipped-cfg", step("also-before-close"));
  const closing = (async () => {
    const receipt = await h.supervisor.closeAndReapAll("SIGNAL");
    events.push("release");
    return receipt;
  })();
  await fenceReached;
  const denied = await h.supervisor.runStep("shipped-cfg", step("after-close"));
  assert.equal(denied.reason, "CANCELLED");
  assert.equal(h.children.length, 2, "admission fence prevents spawn");
  releaseClose();
  const closeReceipt = await closing;
  assert.equal(closeReceipt.confirmedDead, true);
  const closedResults = await Promise.all([live, live2]);
  assert.ok(closedResults.every((result) => result.reason === "CANCELLED"));
  assert.deepEqual(events.slice(0, 2), ["fence", "released"]);
  assert.match(events[2], /^reap:/);
  assert.equal(events.at(-1), "release", "external ownership release occurs only after close/reap");
}

// Architecture-plan test 8: reverse completion and lane reuse are keyed by unique registration tokens.
{
  const h = scriptedHarness();
  const older = h.supervisor.runStep("same-lane", step("older"));
  const newer = h.supervisor.runStep("same-lane", step("newer"));
  const oldToken = h.supervisor.snapshotTelemetry().active[0].tokenId;
  const newToken = h.supervisor.snapshotTelemetry().active[1].tokenId;
  assert.notEqual(oldToken, newToken);
  h.children[1].finish(0);
  assert.equal((await newer).code, 0);
  assert.deepEqual(
    h.supervisor.snapshotTelemetry().active.map((entry) => entry.tokenId),
    [oldToken],
  );
  const reused = h.supervisor.runStep("same-lane", step("reused"));
  const active = h.supervisor.snapshotTelemetry().active;
  assert.equal(active.length, 2);
  assert.notEqual(active[1].tokenId, oldToken);
  h.children[0].finish(0);
  assert.equal((await older).code, 0);
  assert.deepEqual(
    h.supervisor.snapshotTelemetry().active.map((entry) => entry.tokenId),
    [active[1].tokenId],
  );
  h.children[2].finish(0);
  assert.equal((await reused).code, 0);
  await h.supervisor.closeAndReapAll("TEST_DONE");
}

// Lane cancellation is an admission fence scoped to only that lane.
{
  const h = scriptedHarness();
  const cancelled = h.supervisor.runStep("surface-1", step("cancel-me"));
  const other = h.supervisor.runStep("shipped-cfg", step("keep-me"));
  const receipt = await h.supervisor.cancelLane("surface-1", "LOCAL_FAIL_FAST");
  assert.equal(receipt.confirmedDead, true);
  assert.equal((await cancelled).reason, "CANCELLED");
  const denied = await h.supervisor.runStep("surface-1", step("denied"));
  assert.equal(denied.reason, "CANCELLED");
  assert.equal(h.children.length, 2);
  assert.equal(h.children[1].closed, false);
  h.children[1].finish(0);
  assert.equal((await other).code, 0);
  await h.supervisor.closeAndReapAll("TEST_DONE");
}

async function waitUntil(predicate, label, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.fail(`timed out waiting for ${label}`);
}

async function waitDead(pid, label) {
  await waitUntil(() => !pidAlive(pid), `${label} pid ${pid} to die`, 10_000);
}

function exactEmergencyKill(pids) {
  for (const pid of new Set(pids.filter((value) => Number.isInteger(value) && value > 1))) {
    if (!pidAlive(pid)) continue;
    try {
      if (process.platform === "win32") {
        spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
      } else {
        process.kill(pid, "SIGKILL");
      }
    } catch {
      // Best effort for known test-owned identities only.
    }
  }
}

// Architecture-plan test 3 native half: neither allocator is individually over the aggregate ceiling.
{
  const dir = mkdtempSync(join(tmpdir(), "verter-gate-lane-memory-"));
  const aggregateLimitBytes = 256 * MiB;
  const files = [join(dir, "surface.json"), join(dir, "shipped.json")];
  const knownPids = [];
  const allocator = String.raw`
    const { writeFileSync } = require("node:fs");
    const { randomBytes } = require("node:crypto");
    writeFileSync(process.argv[1], JSON.stringify({ pid: process.pid }));
    const held = [];
    setInterval(() => held.push(randomBytes(512 * 1024)), 50);
  `;
  const supervisor = createGateRunSupervisor({
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    memoryLimitBytes: aggregateLimitBytes,
    memoryPollMs: 50,
  });
  try {
    const surface = supervisor.runStep("surface-1", {
      name: "real surface allocator",
      command: process.execPath,
      args: ["-e", allocator, files[0]],
      cwd: process.cwd(),
      env: process.env,
      mirrorOutput: false,
    });
    const shipped = supervisor.runStep("shipped-cfg", {
      name: "real shipped allocator",
      command: process.execPath,
      args: ["-e", allocator, files[1]],
      cwd: process.cwd(),
      env: process.env,
      mirrorOutput: false,
    });
    await waitUntil(() => files.every(existsSync), "both allocator receipts");
    knownPids.push(...files.map((file) => JSON.parse(readFileSync(file, "utf8")).pid));
    const [r1, r2] = await Promise.all([surface, shipped]);
    assert.equal(r1.reason, "MEMORY");
    assert.equal(r2.reason, "MEMORY");
    assert.equal(r1.reapConfirmedDead, true);
    assert.equal(r2.reapConfirmedDead, true);
    const telemetry = supervisor.snapshotTelemetry();
    assert.ok(telemetry.aggregatePeakRssBytes >= aggregateLimitBytes);
    assert.ok(telemetry.perLane["surface-1"].peakRssBytes < aggregateLimitBytes);
    assert.ok(telemetry.perLane["shipped-cfg"].peakRssBytes < aggregateLimitBytes);
    await Promise.all(knownPids.map((pid) => waitDead(pid, "allocator")));
  } finally {
    await supervisor.closeAndReapAll("TEST_FINALLY");
    exactEmergencyKill(knownPids);
    rmSync(dir, { recursive: true, force: true });
  }
}

// Block A fix native half: a sampled detached descendant remains owned after its root exits.
{
  const dir = mkdtempSync(join(tmpdir(), "verter-gate-lane-root-exit-"));
  const receiptFile = join(dir, "tree.json");
  const knownPids = [];
  const parent = String.raw`
    const { spawn } = require("node:child_process");
    const { writeFileSync } = require("node:fs");
    const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
      detached: true,
      stdio: "ignore",
    });
    writeFileSync(process.argv[1], JSON.stringify({ root: process.pid, child: child.pid }));
    setTimeout(() => process.exit(0), process.platform === "win32" ? 4000 : 1000);
  `;
  const unrelated = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    stdio: "ignore",
  });
  knownPids.push(unrelated.pid);
  const supervisor = createGateRunSupervisor({
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    memoryLimitBytes: 1_000 * MiB,
    memoryPollMs: 50,
  });
  try {
    const run = supervisor.runStep("surface-1", {
      name: "root exits before close",
      command: process.execPath,
      args: ["-e", parent, receiptFile],
      cwd: process.cwd(),
      env: process.env,
      mirrorOutput: false,
    });
    await waitUntil(() => existsSync(receiptFile), "root-exit child-tree receipt");
    const receipt = JSON.parse(readFileSync(receiptFile, "utf8"));
    knownPids.push(receipt.root, receipt.child);
    assert.equal((await run).code, 0);
    assert.equal(pidAlive(receipt.child), true);
    const telemetry = supervisor.snapshotTelemetry();
    assert.ok(Array.isArray(telemetry.forests));
    const forest = telemetry.forests.find((entry) => entry.rootPid === receipt.root);
    assert.ok(forest, "the closed root retains a forest registration");
    assert.ok(forest.ownedIdentityCount >= 1, "the detached descendant has an exact identity");
    const closeReceipt = await supervisor.closeAndReapAll("TEST_DONE");
    assert.equal(closeReceipt.confirmedDead, true);
    await waitDead(receipt.child, "detached descendant after root close");
    assert.equal(pidAlive(unrelated.pid), true, "unregistered process remains alive");
  } finally {
    await supervisor.closeAndReapAll("TEST_FINALLY");
    exactEmergencyKill(knownPids);
    rmSync(dir, { recursive: true, force: true });
  }
}

// Architecture-plan test 9: external close reaps every exact root and its detached-PGID descendant.
{
  const dir = mkdtempSync(join(tmpdir(), "verter-gate-lane-close-"));
  const files = [join(dir, "surface.json"), join(dir, "shipped.json")];
  const knownPids = [];
  const parent = String.raw`
    const { spawn } = require("node:child_process");
    const { writeFileSync } = require("node:fs");
    const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
      detached: process.platform !== "win32",
      stdio: "ignore",
    });
    writeFileSync(process.argv[1], JSON.stringify({ root: process.pid, child: child.pid }));
    setInterval(() => {}, 1000);
  `;
  const unrelated = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    stdio: "ignore",
  });
  knownPids.push(unrelated.pid);
  const supervisor = createGateRunSupervisor({
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    memoryLimitBytes: 1_000 * MiB,
    memoryPollMs: 100,
  });
  try {
    const runs = files.map((file, index) =>
      supervisor.runStep(index === 0 ? "surface-1" : "shipped-cfg", {
        name: `real tree ${index}`,
        command: process.execPath,
        args: ["-e", parent, file],
        cwd: process.cwd(),
        env: process.env,
        mirrorOutput: false,
      }),
    );
    await waitUntil(() => files.every(existsSync), "both child-tree receipts");
    const receipts = files.map((file) => JSON.parse(readFileSync(file, "utf8")));
    knownPids.push(...receipts.flatMap((receipt) => [receipt.root, receipt.child]));
    const closeReceipt = await supervisor.closeAndReapAll("SIGNAL");
    assert.equal(closeReceipt.confirmedDead, true);
    const results = await Promise.all(runs);
    assert.ok(results.every((result) => result.reason === "CANCELLED"));
    await Promise.all(
      receipts.flatMap((receipt) => [
        waitDead(receipt.root, "registered root"),
        waitDead(receipt.child, "registered descendant"),
      ]),
    );
    assert.equal(pidAlive(unrelated.pid), true, "unregistered process remains alive");
  } finally {
    await supervisor.closeAndReapAll("TEST_FINALLY");
    exactEmergencyKill(knownPids);
    rmSync(dir, { recursive: true, force: true });
  }
}

process.stdout.write("gate lane supervisor self-test passed\n");
