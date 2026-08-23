#!/usr/bin/env node

import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  EXIT_MEMORY,
  DEFAULT_GATE_BUILD_JOBS,
  DEFAULT_GATE_TEST_THREADS,
  GATE_BUILD_JOBS_12_MIN_MEMORY_LIMIT_BYTES,
  GATE_BUILD_JOBS_8_MIN_MEMORY_LIMIT_BYTES,
  IS_WINDOWS,
  MEMORY_KILL_GRACE_MS,
  buildCargoEnv,
  deriveGateResourceLimits,
  deriveGateLaneResourceSplit,
  mapStepReason,
  parseMemorySize,
  parsePosixProcessForestRss,
  parsePosixProcessTableRss,
  parseWindowsProcessForestRss,
  parseWindowsProcessTableRss,
  runContainedStep,
} from "./gate-internals.mjs";

const MiB = 1024 ** 2;
const GiB = 1024 ** 3;

assert.equal(parseMemorySize("12288MiB"), 12 * GiB);
assert.equal(parseMemorySize("12GiB"), 12 * GiB);
assert.equal(parseMemorySize("1.5GiB"), 1.5 * GiB);
assert.throws(() => parseMemorySize("0"), /positive/);
assert.throws(() => parseMemorySize("12GB"), /MiB|GiB/);

assert.equal(DEFAULT_GATE_BUILD_JOBS, 12);
assert.equal(DEFAULT_GATE_TEST_THREADS, 12);
assert.equal(GATE_BUILD_JOBS_12_MIN_MEMORY_LIMIT_BYTES, 16 * GiB);
assert.equal(GATE_BUILD_JOBS_8_MIN_MEMORY_LIMIT_BYTES, 12 * GiB);
for (const [cpuCount, expected] of [
  [1, 1],
  [4, 4],
  [8, 8],
  [12, 12],
  [32, 12],
]) {
  assert.deepEqual(deriveGateResourceLimits({ cpuCount, totalMemBytes: 128 * GiB }), {
    buildJobs: expected,
    testThreads: expected,
    memoryLimitBytes: 64 * GiB,
  });
}
assert.deepEqual(deriveGateResourceLimits({ cpuCount: 32, totalMemBytes: 24 * GiB }), {
  buildJobs: 8,
  testThreads: 12,
  memoryLimitBytes: 12 * GiB,
});
assert.deepEqual(deriveGateResourceLimits({ cpuCount: 32, totalMemBytes: 16 * GiB }), {
  buildJobs: 4,
  testThreads: 12,
  memoryLimitBytes: 8 * GiB,
});
for (const [memoryLimitBytes, expectedBuildJobs] of [
  [8 * GiB, 4],
  [12 * GiB, 8],
  [16 * GiB, 12],
]) {
  assert.deepEqual(
    deriveGateResourceLimits({
      cpuCount: 32,
      totalMemBytes: 128 * GiB,
      memoryLimitBytes,
    }),
    { buildJobs: expectedBuildJobs, testThreads: 12, memoryLimitBytes },
  );
}
assert.deepEqual(
  deriveGateResourceLimits({
    cpuCount: 2,
    totalMemBytes: 24 * GiB,
    buildJobs: 17,
    testThreads: 19,
    memoryLimitBytes: 8 * GiB,
  }),
  { buildJobs: 17, testThreads: 19, memoryLimitBytes: 8 * GiB },
);
assert.deepEqual(
  deriveGateResourceLimits({
    cpuCount: 32,
    totalMemBytes: 128 * GiB,
    buildJobs: 3,
  }),
  { buildJobs: 3, testThreads: 12, memoryLimitBytes: 64 * GiB },
);
assert.deepEqual(
  deriveGateResourceLimits({
    cpuCount: 32,
    totalMemBytes: 128 * GiB,
    testThreads: 5,
  }),
  { buildJobs: 12, testThreads: 5, memoryLimitBytes: 64 * GiB },
);
for (const bad of [0, -1, 1.5, Number.NaN]) {
  assert.throws(
    () => deriveGateResourceLimits({ cpuCount: 32, buildJobs: bad }),
    /positive integer/,
  );
  assert.throws(
    () => deriveGateResourceLimits({ cpuCount: 32, testThreads: bad }),
    /positive integer/,
  );
}

// deriveGateLaneResourceSplit — Surface 1 and the shipped-cfg lane normally run CONCURRENTLY after
// archive/list, so sizing both lanes to the SAME ceiling independently (the historical bug) requests 2x
// that ceiling from the host for the whole overlap window. The discriminating property this proves: for
// every ceiling >= 2, the two lanes' NUMERIC shares SUM to exactly the ceiling on both axes (never exceed
// it, never silently drop below it), Surface 1 always holds the majority (or equal, at the smallest
// splittable ceiling) share, the shipped-cfg lane never drops below 1, and `concurrent` is reported true —
// the caller is authorized to admit both lanes together because their combined demand genuinely fits. The
// reported real-world case — an 8-core host that logged "cargo build jobs=8, test threads=8" for BOTH
// lanes — is asserted directly.
assert.deepEqual(deriveGateLaneResourceSplit({ buildJobs: 8, testThreads: 8 }), {
  surface: { buildJobs: 6, testThreads: 6 },
  shippedCfg: { buildJobs: 2, testThreads: 2 },
  concurrent: true,
});
for (let total = 2; total <= 32; total++) {
  const split = deriveGateLaneResourceSplit({ buildJobs: total, testThreads: total });
  assert.equal(
    split.surface.buildJobs + split.shippedCfg.buildJobs,
    total,
    `build-jobs shares must sum to the ceiling (total=${total})`,
  );
  assert.equal(
    split.surface.testThreads + split.shippedCfg.testThreads,
    total,
    `test-threads shares must sum to the ceiling (total=${total})`,
  );
  assert.ok(split.shippedCfg.buildJobs >= 1 && split.shippedCfg.testThreads >= 1);
  assert.ok(split.surface.buildJobs >= split.shippedCfg.buildJobs);
  assert.ok(split.surface.testThreads >= split.shippedCfg.testThreads);
  assert.equal(split.concurrent, true, `ceiling=${total} must authorize concurrent lane execution`);
}
// A single-core ceiling cannot give both lanes their own numeric share >= 1 while also bounding the SUM to
// 1 — a lane cannot run cargo/nextest with 0 build jobs or 0 test threads. Rather than bless that as a
// real 2x-the-ceiling concurrent request (the exact defect this function exists to prevent), the split
// reports `concurrent: false`: the caller must run the two lanes SERIALLY (see `orchestrateGateLanes`'s
// `concurrent` option), so the ceiling is honored by scheduling, not by an impossible numeric partition.
assert.deepEqual(deriveGateLaneResourceSplit({ buildJobs: 1, testThreads: 1 }), {
  surface: { buildJobs: 1, testThreads: 1 },
  shippedCfg: { buildJobs: 1, testThreads: 1 },
  concurrent: false,
});
// A ceiling unsplittable on ONLY ONE axis still forces serial execution — the two lanes overlap in
// wall-clock as a unit, so a fine test-threads split does not cure a build-jobs oversubscription (or vice
// versa).
assert.equal(deriveGateLaneResourceSplit({ buildJobs: 1, testThreads: 8 }).concurrent, false);
assert.equal(deriveGateLaneResourceSplit({ buildJobs: 8, testThreads: 1 }).concurrent, false);
// The two axes split independently — a build-heavy, test-light ceiling does not cross-contaminate.
assert.deepEqual(deriveGateLaneResourceSplit({ buildJobs: 12, testThreads: 4 }), {
  surface: { buildJobs: 9, testThreads: 3 },
  shippedCfg: { buildJobs: 3, testThreads: 1 },
  concurrent: true,
});
for (const bad of [0, -1, 1.5, Number.NaN]) {
  assert.throws(
    () => deriveGateLaneResourceSplit({ buildJobs: bad, testThreads: 8 }),
    /positive integer/,
  );
  assert.throws(
    () => deriveGateLaneResourceSplit({ buildJobs: 8, testThreads: bad }),
    /positive integer/,
  );
}

const cargoEnv = buildCargoEnv(
  { PATH: "/usr/bin", CARGO_BUILD_JOBS: "99" },
  "/tmp/verter-gate-target",
  false,
  4,
);
assert.equal(cargoEnv.CARGO_BUILD_JOBS, "4");
const defaultCargoEnv = buildCargoEnv(
  { PATH: "/usr/bin", CARGO_BUILD_JOBS: "99" },
  "/tmp/verter-gate-target",
  false,
  deriveGateResourceLimits({ cpuCount: 32, totalMemBytes: 128 * GiB }).buildJobs,
);
assert.equal(defaultCargoEnv.CARGO_BUILD_JOBS, "12");

// `ps -axo pid=,ppid=,rss=` rows: root (100), a direct child (101), a GRANDCHILD (102, ppid=101, not
// ppid=100) proving the walk is recursive rather than one hop deep, and an unrelated process (103) that
// must stay excluded. This is also the exact shape of the bug this tree walk fixes: nextest reassigns each
// executing test to its OWN process group, so a process-group-membership sum (the prior implementation)
// would miss 101/102 entirely despite them being real descendants — only parent-pid ancestry is reliable.
const posix = parsePosixProcessTableRss(
  [" 100 1 1024", " 101 100 2048", " 102 101 4096", " 103 999 8192"].join("\n"),
  100,
);
assert.deepEqual(posix, { ok: true, rssBytes: 7 * MiB, processCount: 3 });

const windows = parseWindowsProcessTableRss(
  ["100\t1\t1048576", "101\t100\t2097152", "102\t101\t4194304", "999\t1\t8388608"].join("\n"),
  100,
);
assert.deepEqual(windows, { ok: true, rssBytes: 7 * MiB, processCount: 3 });

// One snapshot must account for all registered roots without double counting or silently dropping a
// still-live root. This is the pure parser authority used by the gate-owned multi-lane supervisor.
const forestRoots = [
  { tokenId: 1, laneId: "surface-1", pid: 100 },
  { tokenId: 2, laneId: "shipped-cfg", pid: 200 },
];
const posixForest = parsePosixProcessForestRss(
  ["100 1 1024", "101 100 2048", "200 1 4096", "201 200 8192", "999 1 16384"].join("\n"),
  forestRoots,
);
assert.equal(posixForest.ok, true);
assert.equal(posixForest.rssBytes, 15 * MiB);
assert.equal(posixForest.processCount, 4);
assert.deepEqual(posixForest.perLane, {
  "surface-1": { rssBytes: 3 * MiB, processCount: 2 },
  "shipped-cfg": { rssBytes: 12 * MiB, processCount: 2 },
});
const windowsForest = parseWindowsProcessForestRss(
  [
    "100\t1\t1048576",
    "101\t100\t2097152",
    "200\t1\t4194304",
    "201\t200\t8388608",
    "999\t1\t16777216",
  ].join("\n"),
  forestRoots,
);
assert.equal(windowsForest.ok, true);
assert.equal(windowsForest.rssBytes, 15 * MiB);
assert.equal(windowsForest.processCount, 4);
assert.deepEqual(windowsForest.perLane, posixForest.perLane);

const targetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-"));
try {
  const result = await runContainedStep({
    cmd: process.execPath,
    args: ["-e", "setInterval(() => {}, 1000)"],
    cwd: process.cwd(),
    env: process.env,
    phase: "test",
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    targetDir,
    memoryLimitBytes: 1 * MiB,
    memoryPollMs: 50,
  });
  assert.equal(result.reason, "MEMORY");
  assert.equal(result.reapConfirmedDead, true);
  assert.ok(result.peakRssBytes > result.memoryLimitBytes);
  assert.equal(mapStepReason(result), EXIT_MEMORY);

  const unavailable = await runContainedStep({
    cmd: process.execPath,
    args: ["-e", "setInterval(() => {}, 1000)"],
    cwd: process.cwd(),
    env: process.env,
    phase: "test",
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    targetDir,
    memoryLimitBytes: 1 * GiB,
    memoryPollMs: 50,
    memorySampleFailureLimit: 2,
    memorySampler: () => ({ ok: false, detail: "injected sampler outage" }),
  });
  assert.equal(unavailable.reason, "MEMORY_MONITOR");
  assert.equal(unavailable.reapConfirmedDead, true);
  assert.equal(mapStepReason(unavailable), EXIT_MEMORY);
} finally {
  rmSync(targetDir, { recursive: true, force: true });
}

// MEMORY_KILL_GRACE_MS is the constant runContainedStep's reapNow uses for a MEMORY / MEMORY_MONITOR reap
// instead of the (far slower) TIMEOUT/STALL killGraceMs default (5000ms). Guard the constant itself first —
// cheap, and catches an accidental widen even before the timing scenario below runs.
assert.ok(
  MEMORY_KILL_GRACE_MS > 0 && MEMORY_KILL_GRACE_MS < 1000,
  `MEMORY_KILL_GRACE_MS (${MEMORY_KILL_GRACE_MS}ms) must stay far below the 5000ms TIMEOUT/STALL default`,
);

// Discriminating timing check: a MEMORY-triggered reap must escalate to SIGKILL materially faster than a
// TIMEOUT/STALL reap using the SAME killGraceMs. Both scenarios below run a child that TRAPS SIGTERM (only
// SIGKILL can end it) and pass the SAME slow explicit killGraceMs — proving the MEMORY path does NOT fall
// back to killGraceMs (it must use the separate, short memoryKillGraceMs instead). If reapNow ever regresses
// to sharing one grace value across all four reap reasons again, the MEMORY scenario's elapsed time would
// rise to match the TIMEOUT scenario's and both assertions below would fail.
//
// Windows' reapTree branch always force-kills immediately (`taskkill /T /F`) and never waits out
// killGraceMs at all (a pre-existing, unrelated property of the Windows tree-kill primitive), so the slow
// TIMEOUT baseline this comparison depends on does not exist there. Skip on Windows rather than assert a
// property Windows was never guaranteed to have; MEMORY_KILL_GRACE_MS's value-level guard above already
// covers Windows.
if (!IS_WINDOWS) {
  const SLOW_SHARED_GRACE_MS = 1500;
  const trapArgs = ["-e", "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000);"];

  const timeoutTargetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-timeout-"));
  let timeoutElapsedMs;
  try {
    const startMs = Date.now();
    const timeoutResult = await runContainedStep({
      cmd: process.execPath,
      args: trapArgs,
      cwd: process.cwd(),
      env: process.env,
      phase: "test",
      deadlineMs: Date.now() + 200, // trips almost immediately
      stallMs: 60_000,
      targetDir: timeoutTargetDir,
      killGraceMs: SLOW_SHARED_GRACE_MS,
    });
    timeoutElapsedMs = Date.now() - startMs;
    assert.equal(timeoutResult.reason, "TIMEOUT");
    assert.equal(timeoutResult.reapConfirmedDead, true);
    assert.ok(
      timeoutElapsedMs >= SLOW_SHARED_GRACE_MS * 0.8,
      `TIMEOUT reap of a SIGTERM-trapping child completed in ${timeoutElapsedMs}ms — expected to respect ` +
        `the ~${SLOW_SHARED_GRACE_MS}ms killGraceMs it was given`,
    );
  } finally {
    rmSync(timeoutTargetDir, { recursive: true, force: true });
  }

  const memGraceTargetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-memgrace-"));
  try {
    const startMs = Date.now();
    const memGraceResult = await runContainedStep({
      cmd: process.execPath,
      args: trapArgs,
      cwd: process.cwd(),
      env: process.env,
      phase: "test",
      deadlineMs: Date.now() + 30_000,
      stallMs: 30_000,
      targetDir: memGraceTargetDir,
      memoryLimitBytes: 1 * MiB,
      memoryPollMs: 50,
      // Deliberately the SAME slow value used above — the assertions below only pass if MEMORY ignores it.
      killGraceMs: SLOW_SHARED_GRACE_MS,
    });
    const memElapsedMs = Date.now() - startMs;
    assert.equal(memGraceResult.reason, "MEMORY");
    assert.equal(memGraceResult.reapConfirmedDead, true);
    assert.ok(
      memElapsedMs < SLOW_SHARED_GRACE_MS,
      `MEMORY-triggered reap took ${memElapsedMs}ms — expected the short memoryKillGraceMs ` +
        `(~${MEMORY_KILL_GRACE_MS}ms), not the slow killGraceMs (${SLOW_SHARED_GRACE_MS}ms) passed to this call`,
    );
    assert.ok(
      timeoutElapsedMs - memElapsedMs >= 400,
      `MEMORY reap (${memElapsedMs}ms) must escalate materially faster than the TIMEOUT reap ` +
        `(${timeoutElapsedMs}ms) using the identical killGraceMs value`,
    );
  } finally {
    rmSync(memGraceTargetDir, { recursive: true, force: true });
  }
}

// Real multi-process tree: the direct child spawns its own child (a grandchild relative to
// runContainedStep), which allocates. Proves the memory ceiling aggregates RSS across a REAL process tree
// (not just a single sampled pid) and that the reap kills the whole tree together — the synthetic
// single-process scenarios above cannot exercise the tree-aggregation/tree-kill path.
const multiTargetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-multiproc-"));
try {
  // Fill with real random bytes, not a zeroed/constant-fill Buffer.alloc: an all-zero (or all-one-byte)
  // page is trivially compressible/dedupable by the OS and may never actually raise resident RSS, which
  // would make this scenario time out instead of tripping the ceiling. Random content forces real growth.
  const grandchildScript =
    "const cr=require('crypto');let b=[];" +
    "setInterval(()=>{b.push(cr.randomBytes(1024*1024));},10);"; // ~1MiB/10ms real allocator
  const parentScript =
    "const{spawn}=require('child_process');" +
    `spawn(process.execPath,['-e',${JSON.stringify(grandchildScript)}],{stdio:'ignore'});` +
    "setInterval(()=>{},1000);";
  const multiResult = await runContainedStep({
    cmd: process.execPath,
    args: ["-e", parentScript],
    cwd: process.cwd(),
    env: process.env,
    phase: "test",
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    targetDir: multiTargetDir,
    memoryLimitBytes: 200 * MiB,
    memoryPollMs: 50,
  });
  assert.equal(multiResult.reason, "MEMORY");
  assert.equal(multiResult.reapConfirmedDead, true);
  assert.ok(
    multiResult.memoryProcessCount >= 2,
    "expected the sampled tree to include the grandchild allocator (>=2 processes), got " +
      `${multiResult.memoryProcessCount}`,
  );
} finally {
  rmSync(multiTargetDir, { recursive: true, force: true });
}

// peakRssProcessCount must report the process count OBSERVED AT the peak sample, not whichever sample
// happened to run last. A run that peaks early (several concurrent processes) and then winds down to one
// straggler before completing normally is exactly the shape a parallel build takes: peak RSS lands mid-run
// with several rustc alive, and the final samples before archiving finishes see only one. Pre-fix,
// `memoryProcessCount` was overwritten on every tick, so a non-aborting run's LAST sample's count (1) was
// reported alongside the (correct) MAX rss (from the high-count sample) — this scripted sampler sequence
// reproduces that exact shape and asserts the pairing is now correct.
const pairingTargetDir = mkdtempSync(join(tmpdir(), "verter-gate-memory-selftest-pairing-"));
try {
  let sampleCall = 0;
  const scriptedSampler = () => {
    sampleCall += 1;
    // First sample: the (higher) peak, with 2 processes alive. Every later sample: LOWER rss, 1 process —
    // simulates parallel rustc winding down to a single straggler well before the child exits.
    if (sampleCall === 1) {
      return { ok: true, rssBytes: 100 * MiB, processCount: 2 };
    }
    return { ok: true, rssBytes: 10 * MiB, processCount: 1 };
  };
  const pairingResult = await runContainedStep({
    // A short-lived child that exits normally well after a few poll ticks, so the step completes via the
    // ordinary `close` path rather than a MEMORY abort — this test is about the NON-abort reporting path.
    cmd: process.execPath,
    args: ["-e", "setTimeout(()=>{}, 250)"],
    cwd: process.cwd(),
    env: process.env,
    phase: "test",
    deadlineMs: Date.now() + 30_000,
    stallMs: 30_000,
    targetDir: pairingTargetDir,
    memoryLimitBytes: 200 * MiB, // never crossed by either scripted sample
    memoryPollMs: 20,
    memorySampler: scriptedSampler,
    // Keep this report-only sampler deterministic: native admission-identity latency is covered by the
    // lane supervisor's real-child tests, not by this synthetic peak/last-sample pairing fixture.
    processIdentityFn: (pid) => `scripted-start-${pid}`,
  });
  assert.equal(pairingResult.reason, "");
  assert.equal(pairingResult.peakRssBytes, 100 * MiB);
  assert.equal(
    pairingResult.peakRssProcessCount,
    2,
    "peakRssProcessCount must be the process count from the PEAK sample (2), not the last sample's " +
      `count (1) — got ${pairingResult.peakRssProcessCount}`,
  );
  // memoryProcessCount is deliberately the LAST sample's count (1 here) — it answers "what was alive most
  // recently", a different question from peakRssProcessCount, and callers reporting "peak RSS across N
  // process(es)" must use peakRssProcessCount, not this field.
  assert.equal(pairingResult.memoryProcessCount, 1);
} finally {
  rmSync(pairingTargetDir, { recursive: true, force: true });
}

process.stderr.write("gate memory self-test passed\n");
