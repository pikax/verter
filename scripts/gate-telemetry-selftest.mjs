#!/usr/bin/env node
/**
 * @ai-generated - Exercises the canonical gate's report-only telemetry helpers without running Cargo.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  buildNextestArchiveArgs,
  buildShippedCfgCheckArgs,
  buildShippedCfgContractArgs,
  cargoTimingArtifactPaths,
  collectCargoTimingCapabilities,
  collectNextestTestTimings,
  createGateTelemetry,
  createGateTelemetryReporter,
  formatGateTelemetryText,
  prepareCargoTimingArtifact,
  recordGateAggregateForestPeak,
  recordGatePhase,
  reduceGateLaneReceipts,
  runBoundedVersionProbe,
  snapshotCargoTimingArtifact,
  summarizeGateTelemetry,
  summarizeNextestTimings,
} from "./gate-internals.mjs";

const suites = [
  { "binary-id": "bin_a", "package-name": "crate_a" },
  { "binary-id": "bin_b", "package-name": "crate_a" },
  { "binary-id": "bin_c", "package-name": "crate_b" },
];

test("nextest timing families preserve process counts", () => {
  const text = [
    "  TRY 1 FAIL [   8.000s] (1/5) bin_a pkg::shared::retried",
    "  TRY 2 PASS [   0.500s] (1/5) bin_a pkg::shared::retried",
    "        SLOW [>  1.000s] (2/5) bin_a pkg::shared::slow_then_pass",
    "        PASS [   1.250s] (2/5) bin_a pkg::shared::slow_then_pass",
    "        PASS [          ] (3/5) bin_b pkg::shared::untimed",
    "        PASS [   2.000s] (4/5) bin_b pkg::other::timed",
    "        FAIL [   0.250s] (5/5) bin_c pkg::shared::failed",
  ].join("\n");

  const timings = collectNextestTestTimings(text);
  const report = summarizeNextestTimings(timings, suites, 50);

  assert.equal(
    timings.length,
    5,
    "the retry and SLOW progress row must not add process identities",
  );
  assert.equal(report.totalTests, 5);
  assert.equal(report.processCount, 5);
  assert.equal(report.timedCount, 4);
  assert.equal(report.count, report.timedCount, "legacy count remains the timed-count alias");
  assert.equal(report.totalSec, 4);
  assert.deepEqual(report.perCrate, report.perPackage, "crate is an additive package alias");
  assert.deepEqual(report.perPackage, [
    { key: "crate_a", processCount: 4, timedCount: 3, count: 3, totalSec: 3.75 },
    { key: "crate_b", processCount: 1, timedCount: 1, count: 1, totalSec: 0.25 },
  ]);
  assert.deepEqual(report.perBinary, [
    { key: "bin_b", processCount: 2, timedCount: 1, count: 1, totalSec: 2 },
    { key: "bin_a", processCount: 2, timedCount: 2, count: 2, totalSec: 1.75 },
    { key: "bin_c", processCount: 1, timedCount: 1, count: 1, totalSec: 0.25 },
  ]);
  assert.equal(report.perFamily.length, 4);
  assert.equal(report.topFamilies.length, 4);
  assert.deepEqual(
    report.perFamily.find((row) => row.key === "bin_b pkg::shared"),
    { key: "bin_b pkg::shared", processCount: 1, timedCount: 0, count: 0, totalSec: 0 },
  );
  assert.notEqual(
    report.perFamily.find((row) => row.key === "bin_a pkg::shared")?.key,
    report.perFamily.find((row) => row.key === "bin_b pkg::shared")?.key,
    "same-named families in different binaries stay distinct",
  );
});

test("cargo timing argv and artifact identities stay distinct", () => {
  const archiveBase = {
    buildJobs: 4,
    cargoProfile: null,
    archiveFile: "C:/artifact/nextest.tar.zst",
    runnerTarget: "C:/artifact/target",
    features: ["verter_session/bf2-authoritative"],
  };
  assert.deepEqual(buildNextestArchiveArgs({ ...archiveBase, timingsEnabled: true }), [
    "nextest",
    "archive",
    "--workspace",
    "--build-jobs",
    "4",
    "--timings",
    "--features",
    "verter_session/bf2-authoritative",
    "--archive-file",
    "C:/artifact/nextest.tar.zst",
    "--target-dir",
    "C:/artifact/target",
    "--zstd-level",
    "-7",
  ]);
  assert.deepEqual(buildNextestArchiveArgs({ ...archiveBase, timingsEnabled: false }), [
    "nextest",
    "archive",
    "--workspace",
    "--build-jobs",
    "4",
    "--features",
    "verter_session/bf2-authoritative",
    "--archive-file",
    "C:/artifact/nextest.tar.zst",
    "--target-dir",
    "C:/artifact/target",
    "--zstd-level",
    "-7",
  ]);
  assert.deepEqual(buildShippedCfgCheckArgs({ timingsEnabled: true }), [
    "check",
    "--workspace",
    "--all-targets",
    "--profile",
    "no-debug-assertions",
    "--timings",
  ]);
  assert.deepEqual(buildShippedCfgCheckArgs({ timingsEnabled: false }), [
    "check",
    "--workspace",
    "--all-targets",
    "--profile",
    "no-debug-assertions",
  ]);
  assert.deepEqual(
    buildShippedCfgContractArgs({
      timingsEnabled: true,
      exhaustive: true,
      testThreads: 4,
    }),
    [
      "nextest",
      "run",
      "-p",
      "verter_shipped_cfg_contract",
      "--cargo-profile",
      "no-debug-assertions",
      "--timings",
      "--no-fail-fast",
      "--test-threads",
      "4",
    ],
  );
  assert.deepEqual(
    buildShippedCfgContractArgs({
      timingsEnabled: false,
      exhaustive: false,
      testThreads: 4,
    }),
    [
      "nextest",
      "run",
      "-p",
      "verter_shipped_cfg_contract",
      "--cargo-profile",
      "no-debug-assertions",
      "--test-threads",
      "4",
    ],
    "the local capability-unavailable control preserves selection but stays fail-fast",
  );

  const capabilities = collectCargoTimingCapabilities((command, args) => ({
    available: command === "cargo",
    stdout: args[0] === "check" ? "Options:\n  --timings" : "Compilation:\n  --timings[=<FMTS>]",
  }));
  assert.deepEqual(capabilities, {
    devArchive: { supported: true, error: null },
    shippedCheck: { supported: true, error: null },
    shippedContract: { supported: true, error: null },
  });

  const paths = cargoTimingArtifactPaths("C:/artifact/target", "C:/artifact/gate-work");
  assert.equal(paths.source, join("C:/artifact/target", "cargo-timings", "cargo-timing.html"));
  assert.deepEqual(Object.keys(paths.destinations), [
    "dev-archive",
    "shipped-check",
    "shipped-contract",
  ]);
  assert.equal(new Set(Object.values(paths.destinations)).size, 3);
  assert.ok(paths.destinations["dev-archive"].endsWith("dev-nextest-archive.html"));
  assert.ok(paths.destinations["shipped-check"].endsWith("shipped-cfg-check.html"));
  assert.ok(paths.destinations["shipped-contract"].endsWith("shipped-cfg-contract.html"));
});

test("cargo timing snapshots reject stale and missing reports without throwing", () => {
  const root = mkdtempSync(join(tmpdir(), "verter-gate-cargo-timing-"));
  try {
    const runnerTarget = join(root, "target");
    const gateDir = join(root, "gate-work");
    const paths = cargoTimingArtifactPaths(runnerTarget, gateDir);
    mkdirSync(join(runnerTarget, "cargo-timings"), { recursive: true });

    const missingCapture = prepareCargoTimingArtifact({
      source: paths.source,
      destination: paths.destinations["dev-archive"],
      now: () => 1000,
    });
    const missing = snapshotCargoTimingArtifact(missingCapture);
    assert.equal(missing.available, false);
    assert.equal(missing.status, "missing");

    const staleCapture = prepareCargoTimingArtifact({
      source: paths.source,
      destination: paths.destinations["shipped-check"],
      now: () => 20_000,
    });
    writeFileSync(paths.source, "stale");
    const stale = snapshotCargoTimingArtifact(staleCapture, {
      statFn: (path) => {
        const real = statSync(path);
        return { size: real.size, mtimeMs: 1_000, isFile: () => real.isFile() };
      },
    });
    assert.equal(stale.available, false);
    assert.equal(stale.status, "stale");
    assert.equal(
      statSync(paths.source).isFile(),
      true,
      "the helper never broadly deletes timing dirs",
    );

    writeFileSync(paths.source, "unchanged-old-report");
    const oldSourceMtimeMs = statSync(paths.source).mtimeMs;
    const failedClearCapture = prepareCargoTimingArtifact({
      source: paths.source,
      destination: join(gateDir, "cargo-timings", "failed-clear.html"),
      now: () => oldSourceMtimeMs + 1_000,
      rmFileFn: (path) => {
        if (path === paths.source) throw new Error("synthetic source clear refusal");
        rmSync(path, { force: true });
      },
    });
    const failedClear = snapshotCargoTimingArtifact(failedClearCapture);
    assert.equal(failedClearCapture.sourceCleared, false);
    assert.ok(failedClear.warnings.includes("source-clear-failed"));
    assert.equal(
      failedClear.available,
      false,
      "an unchanged old report cannot become fresh merely because its mtime is within tolerance",
    );
    assert.equal(failedClear.status, "stale");

    const changedIdentityCapture = prepareCargoTimingArtifact({
      source: paths.source,
      destination: join(gateDir, "cargo-timings", "failed-clear-changed.html"),
      now: () => oldSourceMtimeMs + 1_000,
      rmFileFn: (path) => {
        if (path === paths.source) throw new Error("synthetic source clear refusal");
        rmSync(path, { force: true });
      },
    });
    writeFileSync(paths.source, "new-report-with-strongly-changed-content-identity");
    const changedIdentity = snapshotCargoTimingArtifact(changedIdentityCapture);
    assert.equal(changedIdentity.available, true, changedIdentity.error);
    assert.equal(changedIdentity.status, "fresh");
    assert.equal(
      readFileSync(changedIdentityCapture.destination, "utf8"),
      "new-report-with-strongly-changed-content-identity",
    );

    const destinations = [];
    for (const [phaseId, destination] of Object.entries(paths.destinations)) {
      const capture = prepareCargoTimingArtifact({
        source: paths.source,
        destination,
        now: () => Date.now() - 10,
      });
      writeFileSync(paths.source, `<html>${phaseId}</html>`);
      const snap = snapshotCargoTimingArtifact(capture);
      assert.equal(snap.available, true, snap.error || phaseId);
      assert.equal(snap.status, "fresh");
      assert.equal(readFileSync(destination, "utf8"), `<html>${phaseId}</html>`);
      destinations.push(snap.relativePath);
    }
    assert.equal(new Set(destinations).size, 3, "each producing command keeps a distinct snapshot");

    const copyFailureCapture = prepareCargoTimingArtifact({
      source: paths.source,
      destination: join(gateDir, "cargo-timings", "copy-failure.html"),
      now: () => Date.now() - 10,
    });
    writeFileSync(paths.source, "fresh");
    const copyFailure = snapshotCargoTimingArtifact(copyFailureCapture, {
      copyFileFn: () => {
        throw new Error("synthetic copy refusal");
      },
    });
    assert.equal(copyFailure.available, false);
    assert.equal(copyFailure.status, "copy-failed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("version probes obey the parent deadline, hard-kill a trapping child, and leave no survivor", () => {
  let childPid = null;
  try {
    const trappingChild = [
      'process.on("SIGTERM", () => {});',
      "setTimeout(() => process.exit(0), 1_500);",
      "setInterval(() => {}, 1_000);",
    ].join("");
    const startedAtMs = Date.now();
    const trapped = runBoundedVersionProbe(process.execPath, ["-e", trappingChild], {
      timeoutMs: 150,
    });
    const elapsedMs = Date.now() - startedAtMs;
    childPid = trapped.pid;

    assert.equal(trapped.available, false);
    assert.equal(trapped.error, "timeout");
    assert.ok(Number.isInteger(childPid), "the timed-out spawn must report its direct child PID");
    assert.ok(elapsedMs < 1_000, `trapping child exceeded the hard wall bound (${elapsedMs}ms)`);
    assert.throws(
      () => process.kill(childPid, 0),
      (error) => error?.code === "ESRCH",
      `timed-out probe process ${childPid} survived`,
    );

    const calls = [];
    const clamped = runBoundedVersionProbe("synthetic-tool", ["--version"], {
      deadlineMs: 10_050,
      now: () => 10_000,
      timeoutMs: 2_000,
      spawnSyncFn: (_command, _args, options) => {
        calls.push(options);
        return { status: 0, stdout: "synthetic 1.0", stderr: "", error: null };
      },
    });
    assert.equal(clamped.available, true);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].timeout, 50, "the per-probe budget is clamped to parent time remaining");
    assert.equal(calls[0].killSignal, "SIGKILL", "timeout termination must be unignorable");

    let expiredSpawned = false;
    const expired = runBoundedVersionProbe("synthetic-tool", ["--version"], {
      deadlineMs: 10_000,
      now: () => 10_001,
      spawnSyncFn: () => {
        expiredSpawned = true;
        return { status: 0, stdout: "wrong", stderr: "", error: null };
      },
    });
    assert.equal(expired.available, false);
    assert.equal(expired.error, "timeout");
    assert.equal(expiredSpawned, false, "an expired parent deadline must refuse before spawn");
  } finally {
    if (Number.isInteger(childPid)) {
      try {
        process.kill(childPid, "SIGKILL");
      } catch {
        // Expected after the probe reaps its direct child.
      }
    }
  }
});

test("gate telemetry fake clock preserves whole timing and paired RSS peak", () => {
  let current = 1_000;
  const telemetry = createGateTelemetry({
    mode: "gate",
    now: () => current,
    startedUtc: "2026-08-22T10:00:00.000Z",
    expectedPhaseIds: ["dev-archive", "surface-1", "shipped-contract"],
  });
  recordGatePhase(telemetry, "dev-archive", {
    status: "ok",
    startedAtMs: 1_050,
    durationMs: 200,
    peakRssBytes: 900,
    peakRssProcessCount: 7,
  });
  recordGatePhase(telemetry, "surface-1", {
    status: "failed",
    startedAtMs: 1_300,
    durationMs: 300,
    peakRssBytes: 1_500,
    peakRssProcessCount: 4,
  });
  recordGatePhase(telemetry, "shipped-contract", {
    status: "ok",
    startedAtMs: 1_650,
    durationMs: 100,
    peakRssBytes: 1_500,
    peakRssProcessCount: 2,
  });
  current = 1_800;
  const summary = summarizeGateTelemetry(telemetry, {
    terminalReached: true,
    exitCode: 1,
  });

  assert.equal(summary.schema, "verter-gate-telemetry/v1");
  assert.equal(summary.schemaVersion, 1);
  assert.equal(
    summary.completeness,
    "complete",
    "a fully measured red gate is still complete telemetry",
  );
  assert.equal(summary.whole.elapsedMs, 800);
  assert.deepEqual(summary.whole.containedChildTreePeak, {
    phaseId: "shipped-contract",
    rssBytes: 1_500,
    processCount: 2,
  });
  assert.deepEqual(
    summary.phases.map((phase) => phase.id),
    ["dev-archive", "surface-1", "shipped-contract"],
  );
  assert.match(formatGateTelemetryText(summary), /whole elapsed 0\.800s/);
  assert.match(formatGateTelemetryText(summary), /shipped-contract/);
});

test("partial gate telemetry is never complete", () => {
  let current = 10;
  const telemetry = createGateTelemetry({
    mode: "gate",
    now: () => current,
    expectedPhaseIds: ["build-prerequisite", "surface-1", "teardown"],
  });
  recordGatePhase(telemetry, "build-prerequisite", {
    status: "aborted",
    durationMs: 5,
  });
  current = 20;
  const summary = summarizeGateTelemetry(telemetry, {
    terminalReached: true,
    exitCode: 124,
  });
  assert.equal(summary.completeness, "partial");
  assert.equal(summary.terminal.reached, true);
  assert.equal(summary.phases.find((row) => row.id === "surface-1").status, "not-run");
  assert.notEqual(summary.completeness, "complete");
});

test("Block B same-snapshot aggregate RSS outranks each lane-local phase peak", () => {
  const telemetry = createGateTelemetry({
    mode: "gate",
    now: () => 100,
    expectedPhaseIds: ["surface-1", "shipped-check"],
  });
  recordGatePhase(telemetry, "surface-1", {
    status: "ok",
    durationMs: 25,
    peakRssBytes: 70,
    peakRssProcessCount: 2,
  });
  recordGatePhase(telemetry, "shipped-check", {
    status: "ok",
    durationMs: 20,
    peakRssBytes: 60,
    peakRssProcessCount: 3,
  });
  recordGateAggregateForestPeak(telemetry, {
    aggregatePeakRssBytes: 110,
    aggregatePeakProcessCount: 5,
    aggregatePeakPerLane: {
      "surface-1": { rssBytes: 70, processCount: 2 },
      "shipped-cfg": { rssBytes: 40, processCount: 3 },
      "invalid-bytes": { rssBytes: -1, processCount: 1 },
      "invalid-count": { rssBytes: 1, processCount: Number.NaN },
      "legacy-numeric": 9,
    },
  });
  const summary = summarizeGateTelemetry(telemetry, {
    terminalReached: true,
    exitCode: 0,
    endMs: 125,
  });
  assert.deepEqual(summary.whole.containedChildTreePeak, {
    phaseId: "supervisor-aggregate",
    observation: "supervisor-same-snapshot",
    rssBytes: 110,
    processCount: 5,
    laneContributions: {
      "surface-1": { rssBytes: 70, processCount: 2 },
      "shipped-cfg": { rssBytes: 40, processCount: 3 },
    },
  });
  assert.equal(summary.phases.find((row) => row.id === "surface-1").peakRssBytes, 70);
  assert.equal(summary.phases.find((row) => row.id === "shipped-check").peakRssBytes, 60);
});

test("Block B cargo timing capture reads each producing lane target and preserves destinations", () => {
  const telemetry = createGateTelemetry({ expectedPhaseIds: [], now: () => 0 });
  const preparations = [];
  const reporter = createGateTelemetryReporter({
    telemetry,
    deadlineMs: 1_000,
    now: () => 0,
    targetState: "empty",
    resources: {},
    env: {},
    runnerTarget: "C:/runner/front-target",
    gateDir: "C:/runner/gate-work",
    warnFn: () => {},
    logFn: () => {},
    collectCargoTimingCapabilitiesFn: () => ({
      devArchive: { supported: true, error: null },
      shippedCheck: { supported: true, error: null },
      shippedContract: { supported: true, error: null },
    }),
    collectEnvironmentFingerprintFn: () => ({}),
    prepareCargoTimingArtifactFn: (capture) => {
      preparations.push(capture);
      return {
        ...capture,
        relativePath: `cargo-timings/${capture.destination.split(/[\\/]/).at(-1)}`,
        warnings: [],
      };
    },
  });
  reporter.collectStartup();
  reporter.beginCargoTiming("dev-archive", "C:/runner/front-target");
  reporter.beginCargoTiming("shipped-check", "C:/runner/lanes/shipped-cfg/target");
  reporter.beginCargoTiming("shipped-contract", "C:/runner/lanes/shipped-cfg/target");

  assert.deepEqual(
    preparations.map(({ source }) => source),
    [
      join("C:/runner/front-target", "cargo-timings", "cargo-timing.html"),
      join("C:/runner/lanes/shipped-cfg/target", "cargo-timings", "cargo-timing.html"),
      join("C:/runner/lanes/shipped-cfg/target", "cargo-timings", "cargo-timing.html"),
    ],
  );
  assert.deepEqual(
    preparations.map(({ destination }) => destination.split(/[\\/]/).at(-1)),
    ["dev-nextest-archive.html", "shipped-cfg-check.html", "shipped-cfg-contract.html"],
  );
});

test("Block B filesystem timing fixture keeps front, shipped-check, and shipped-contract identities", () => {
  const root = mkdtempSync(join(tmpdir(), "verter-gate-lane-timings-"));
  try {
    const frontTarget = join(root, "front-target");
    const shippedTarget = join(root, "lanes", "shipped-cfg", "target");
    const gateDir = join(root, "gate-work");
    const frontPaths = cargoTimingArtifactPaths(frontTarget, gateDir);
    const shippedPaths = cargoTimingArtifactPaths(shippedTarget, gateDir);
    const capture = (paths, phaseId, body) => {
      const prepared = prepareCargoTimingArtifact({
        source: paths.source,
        destination: paths.destinations[phaseId],
      });
      mkdirSync(join(paths.source, ".."), { recursive: true });
      writeFileSync(paths.source, body);
      const artifact = snapshotCargoTimingArtifact(prepared);
      assert.equal(artifact.available, true, `${phaseId} snapshot is available`);
    };

    capture(frontPaths, "dev-archive", "front-dev");
    capture(shippedPaths, "shipped-check", "shipped-check");
    capture(shippedPaths, "shipped-contract", "shipped-contract");

    assert.equal(readFileSync(frontPaths.destinations["dev-archive"], "utf8"), "front-dev");
    assert.equal(readFileSync(shippedPaths.destinations["shipped-check"], "utf8"), "shipped-check");
    assert.equal(
      readFileSync(shippedPaths.destinations["shipped-contract"], "utf8"),
      "shipped-contract",
    );
    assert.notEqual(frontPaths.source, shippedPaths.source);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("GB17.8 local cancellation marks a live check aborted and unadmitted contract not-run", () => {
  const telemetry = createGateTelemetry({
    mode: "gate",
    now: () => 100,
    expectedPhaseIds: ["surface-1", "shipped-check", "shipped-contract"],
  });
  recordGatePhase(telemetry, "surface-1", {
    status: "failed",
    durationMs: 25,
  });
  recordGatePhase(telemetry, "shipped-check", {
    status: "aborted",
    durationMs: 20,
    detail: "SURFACE_1_FAIL_FAST",
  });
  const summary = summarizeGateTelemetry(telemetry, {
    terminalReached: true,
    exitCode: 1,
    endMs: 150,
  });

  assert.equal(summary.completeness, "partial");
  assert.equal(summary.terminal.exitCode, 1);
  assert.equal(summary.phases.find((row) => row.id === "shipped-check").status, "aborted");
  assert.equal(summary.phases.find((row) => row.id === "shipped-contract").status, "not-run");
  assert.notEqual(summary.completeness, "complete");
});

test("production telemetry orchestration cannot mutate the canonical gate verdict", async () => {
  assert.equal(
    typeof createGateTelemetryReporter,
    "function",
    "the test must drive the reporter composed by the production gate",
  );

  const telemetry = createGateTelemetry({
    now: () => 1_000,
    expectedPhaseIds: ["surface-1"],
  });
  const laneReceipts = {
    surface: {
      hardFailure: true,
      failures: [{ surface: "nextest", name: "cases::real_failure" }],
      toleratedOccurred: false,
      coverage: { parseable: true, complete: true },
    },
    shipped: {
      hardFailure: false,
      failures: [],
      check: { status: "ok" },
      contract: { status: "ok", parseable: true, complete: true },
      parity: { complete: true, matches: true },
    },
  };
  const receiptsBeforeReporting = structuredClone(laneReceipts);
  const verdictBeforeReporting = reduceGateLaneReceipts(laneReceipts);
  const warnings = [];
  let versionProbeCalls = 0;
  const reporter = createGateTelemetryReporter({
    telemetry,
    deadlineMs: 1_050,
    now: () => 1_000,
    targetState: "empty",
    resources: { buildJobs: 4, testThreads: 4 },
    env: {},
    runnerTarget: "C:/synthetic/target",
    gateDir: "C:/synthetic/gate-work",
    warnFn: (message) => warnings.push(message),
    logFn: () => {},
    runVersionProbeFn: () => {
      versionProbeCalls++;
      return { available: false, stdout: "", error: "timeout" };
    },
    collectCargoTimingCapabilitiesFn: (runProbe) => {
      runProbe("cargo", ["check", "--help"]);
      return {
        devArchive: { supported: true, error: null },
        shippedCheck: { supported: true, error: null },
        shippedContract: { supported: true, error: null },
      };
    },
    collectEnvironmentFingerprintFn: () => {
      throw new Error("synthetic fingerprint failure");
    },
    prepareCargoTimingArtifactFn: () => ({
      source: "source",
      destination: "destination",
      relativePath: "cargo-timings/dev-nextest-archive.html",
      sourceCleared: true,
      warnings: [],
    }),
    snapshotCargoTimingArtifactFn: (capture) => ({
      phaseId: null,
      available: false,
      status: "copy-failed",
      relativePath: capture.relativePath,
      error: "synthetic timing report failure",
      warnings: [],
    }),
  });

  reporter.collectStartup();
  const timingCapture = reporter.beginCargoTiming("dev-archive");
  reporter.finishCargoTiming("dev-archive", timingCapture);
  reporter.recordPhase("surface-1", { status: "failed", durationMs: 10 });
  const summary = summarizeGateTelemetry(telemetry, {
    terminalReached: true,
    exitCode: 1,
    endMs: 1_020,
  });

  assert.equal(versionProbeCalls, 1, "the production startup path exercised the failing probe");
  assert.deepEqual(
    laneReceipts,
    receiptsBeforeReporting,
    "reporting failures must not mutate the exact receipts used by the gate verdict",
  );
  assert.deepEqual(reduceGateLaneReceipts(laneReceipts), verdictBeforeReporting);
  assert.equal(summary.completeness, "partial");
  assert.ok(summary.warnings.some((warning) => warning.includes("version probe")));
  assert.ok(summary.warnings.some((warning) => warning.includes("fingerprint")));
  assert.ok(summary.warnings.some((warning) => warning.includes("cargo timing dev-archive")));
  assert.ok(warnings.length >= 3, "every injected reporting failure is warned to the operator");
});

test("schema v1 remains additive to legacy nextest timing fields", () => {
  const timings = collectNextestTestTimings(
    "        PASS [   0.125s] (1/1) bin_a pkg::family::one",
  );
  const report = summarizeNextestTimings(timings, suites, 50);
  const telemetry = createGateTelemetry({
    expectedPhaseIds: ["surface-1"],
    now: () => 50,
  });
  telemetry.environment = { bounded: true };
  telemetry.lanes = { overlapBoundary: "post-list", replayOrder: ["surface-1"] };
  telemetry.nextest.surface1 = report;
  recordGatePhase(telemetry, "surface-1", { status: "ok", durationMs: 25 });
  const schema = summarizeGateTelemetry(telemetry, {
    terminalReached: true,
    exitCode: 0,
    endMs: 75,
  });

  assert.equal(schema.schemaVersion, 1);
  assert.equal(schema.environment.bounded, true);
  assert.equal(schema.lanes.overlapBoundary, "post-list");
  assert.equal(schema.nextest.surface1.totalTests, 1);
  assert.equal(schema.nextest.surface1.count, 1);
  assert.equal(schema.nextest.surface1.timedCount, 1);
  assert.equal(schema.nextest.surface1.totalSec, 0.125);
  assert.ok(Array.isArray(schema.nextest.surface1.perPackage));
  assert.ok(Array.isArray(schema.nextest.surface1.perBinary));
  assert.ok(Array.isArray(schema.nextest.surface1.topFamilies));
});
